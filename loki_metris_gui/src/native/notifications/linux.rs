//! Linux 通知遵守 freedesktop D-Bus 协议；权限探测、提交与点击均不阻塞窗口线程。

use super::driver::Event;
use ashpd::zbus::{Connection, Proxy, connection::Builder, proxy::SignalStream, zvariant::Value};
use futures::{FutureExt as _, StreamExt as _, channel::{mpsc, oneshot}};
use gpui_kit::{BackgroundExecutor, Task};
use loki_metris_core::usage_milestones::{MilestoneOutcome, NotificationPermission};
use std::{collections::HashMap, sync::{Arc, Mutex, Weak, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Duration};

/// 队列只携带已获授权的文字和回执，桌面数值 ID 的所有权由唯一 worker 持有。
struct Delivery { id: String, title: String, body: String, receipt: oneshot::Sender<MilestoneOutcome>, cancelled: Arc<AtomicBool>, expires: futures::future::BoxFuture<'static, ()> }

/// 生命周期随原生通知 owner 释放；点击合并为一位，不受提交队列占用影响。
pub(super) struct Host {
    /// 向唯一 D-Bus worker 投递通知的有界邮箱（容量 16）。
    sender: Mutex<mpsc::Sender<Delivery>>,
    /// worker 收到本应用通知 default 动作后置位，前台轮询读取后清零。
    clicked: Arc<AtomicBool>,
    /// 尚未完成的提交取消令牌，最多 17 个；关闭或撤销时逐个置位。
    pending: Mutex<Vec<Weak<AtomicBool>>>,
    /// 当前是否允许提交：本地开关开启且服务可用。
    allowed: AtomicBool,
    /// 撤销代次；每次禁止提交时递增，登记过程中代次变化的请求立即取消。
    revocation: AtomicU64,
    /// 用于创建提交超时计时器的后台执行器。
    executor: BackgroundExecutor,
    /// 独占 D-Bus 连接与通知 ID 的后台 worker，随 Host 释放而取消。
    _worker: Task<()>,
}

impl Host {
    /// 建立有限邮箱，不在构造或启动时显示通知、修改权限或发送测试消息。
    pub fn new(_: std::sync::mpsc::SyncSender<Event>, executor: BackgroundExecutor) -> Option<Self> {
        let (sender, receiver) = mpsc::channel(16);
        let clicked = Arc::new(AtomicBool::new(false));
        let callback = clicked.clone();
        let worker = executor.spawn(async move {
            run(receiver, callback).await;
        });
        Some(Self { sender: Mutex::new(sender), clicked, pending: Mutex::new(Vec::new()), allowed: AtomicBool::new(false), revocation: AtomicU64::new(0), executor, _worker: worker })
    }

    /// 创建future时即冻结撤销代次并登记命令；晚轮询不能复活已关闭的请求，丢失系统回执保持未知。
    pub fn send(&self, id: String, title: String, body: String) -> impl std::future::Future<Output = MilestoneOutcome> + 'static + use<> {
        let response = self.enqueue(id, title, body);
        async move {
            match response {
                Ok(response) => response.await.unwrap_or(MilestoneOutcome::Unknown),
                Err(outcome) => outcome,
            }
        }
    }

    /// 同步接受与撤销共用pending锁；关闭后重开造成的代次变化也必须取消尚未登记完成的命令。
    fn enqueue(&self, id: String, title: String, body: String) -> Result<oneshot::Receiver<MilestoneOutcome>, MilestoneOutcome> {
        let revocation = self.revocation.load(Ordering::Acquire);
        if !self.allowed.load(Ordering::Acquire) { return Err(MilestoneOutcome::Rejected); }
        let (receipt, result) = oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let Ok(mut pending) = self.pending.lock() else { return Err(MilestoneOutcome::Rejected); };
            pending.retain(|token| token.strong_count() > 0);
            if pending.len() >= 17 { return Err(MilestoneOutcome::Rejected); }
            pending.push(Arc::downgrade(&cancelled));
            if !self.allowed.load(Ordering::Acquire) || self.revocation.load(Ordering::Acquire) != revocation {
                cancelled.store(true, Ordering::Release);
            }
        }
        {
            let Ok(mut sender) = self.sender.lock() else { return Err(MilestoneOutcome::Rejected); };
            let expires = self.executor.timer(Duration::from_secs(8)).boxed();
            if sender.try_send(Delivery { id, title, body, receipt, cancelled, expires }).is_err() { return Err(MilestoneOutcome::Rejected); }
        }
        Ok(result)
    }

    /// 已校验属于本应用的 default 动作在前台恢复窗口，未知通知不能触发。
    pub fn take_click(&self) -> bool { self.clicked.swap(false, Ordering::AcqRel) }

    /// 关闭意图立即撤销未提交命令；已经进入系统调用的回执仍按真实结果交回核心。
    pub fn cancel_pending(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            for token in pending.drain(..).filter_map(|token| token.upgrade()) { token.store(true, Ordering::Release); }
        }
    }

    /// 偏好或权限暂停立即拒收尚未开始的发送 future，重新允许不会复活已撤销命令。
    pub fn set_delivery_allowed(&self, allowed: bool) {
        self.allowed.store(allowed, Ordering::Release);
        if !allowed {
            self.revocation.fetch_add(1, Ordering::AcqRel);
            self.cancel_pending();
        }
    }
}

/// Linux 标准没有每应用授权查询；实际服务可调用才为可用，不代表横幅已经送达。
pub(super) async fn permission() -> NotificationPermission {
    async {
        let connection = connect().await?;
        let proxy = proxy(&connection).await?;
        let _: Vec<String> = proxy.call("GetCapabilities", &()).await?;
        Ok::<_, ashpd::zbus::Error>(())
    }.await.map_or(NotificationPermission::Unavailable, |()| NotificationPermission::Allowed)
}

/// 有限方法期限用于退出和错误恢复；不依赖额外全局命令或外部通知 CLI。
async fn connect() -> ashpd::zbus::Result<Connection> {
    Builder::session()?.method_timeout(Duration::from_secs(2)).build().await
}

/// 固定系统服务是唯一目的地，不接受用户提供的总线名或对象路径。
async fn proxy(connection: &Connection) -> ashpd::zbus::Result<Proxy<'_>> {
    Proxy::new(connection, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await
}

/// 点击和替换 ID 只属于同一唯一总线名，通知服务重启后不能操作其它应用的新 ID。
struct Session {
    /// 固定到 `owner` 唯一名的通知服务代理。
    proxy: Proxy<'static>,
    /// ActionInvoked 信号流，用于识别本应用通知的点击。
    actions: SignalStream<'static>,
    /// 通知服务当前的唯一总线名；服务重启后名字变化，旧会话随之作废。
    owner: String,
    /// 本应用通知 ID 到桌面数值 ID 的映射，只用于替换和点击归属。
    owned: HashMap<String, u32>,
}

/// 读取当前唯一服务所有者；后续调用固定到该唯一名，避免检查与提交之间的服务替换。
async fn service_owner(connection: &Connection) -> ashpd::zbus::Result<String> {
    let bus = ashpd::zbus::fdo::DBusProxy::new(connection).await?;
    Ok(bus.get_name_owner("org.freedesktop.Notifications".try_into()?).await?.to_string())
}

impl Session {
    /// 在第一条已授权提交时才连接；服务暂不可用不会杀死长期 owner，下一次提交可以恢复。
    async fn open(connection: Connection) -> ashpd::zbus::Result<Self> {
        let owner = service_owner(&connection).await?;
        let proxy = Proxy::new_owned(connection, owner.clone(), "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await?;
        let actions = proxy.receive_signal("ActionInvoked").await?;
        Ok(Self { proxy, actions, owner, owned: HashMap::new() })
    }
}

/// 提交只到当前服务，队列与ID映射均有限；连接失败允许后续请求重连。
async fn deliver(session: &mut Option<Session>, mut command: Delivery) {
    let expires = std::mem::replace(&mut command.expires, futures::future::pending().boxed());
    let result = futures::future::select(expires, deliver_inner(session, command).boxed()).await;
    let expired = matches!(result, futures::future::Either::Left(_));
    // 释放可能仍借用 session 的未完成投递；丢失回执映射为 Unknown，不能盲目重投。
    drop(result);
    if expired { *session = None; }
}

/// 检查取消后连接并投递；外层期限同时涵盖socket连接、认证、owner查询和Notify。
async fn deliver_inner(session: &mut Option<Session>, command: Delivery) {
    if command.receipt.is_canceled() { return; }
    if command.cancelled.load(Ordering::Acquire) { let _ = command.receipt.send(MilestoneOutcome::Rejected); return; }
    let reusable = if let Some(current) = session.as_ref() {
        service_owner(current.proxy.connection()).await.is_ok_and(|owner| owner == current.owner)
    } else { false };
    if !reusable {
        let connection = if let Some(current) = session.as_ref() {
            Ok(current.proxy.connection().clone())
        } else { connect().await };
        *session = match connection {
            Ok(connection) => Session::open(connection).await.ok(),
            Err(_) => None,
        };
    }
    let Some(current) = session.as_mut() else {
        let _ = command.receipt.send(MilestoneOutcome::Rejected);
        return;
    };
    // 关闭开关、退出或上层期限结束会丢弃回执；尚未触达系统的旧请求不能迟到发送。
    if command.receipt.is_canceled() { return; }
    if command.cancelled.load(Ordering::Acquire) { let _ = command.receipt.send(MilestoneOutcome::Rejected); return; }
    let replacing = current.owned.get(&command.id).copied().unwrap_or(0);
    let outcome = match notify(&current.proxy, replacing, &command.title, &command.body).await {
        Ok(native) if native != 0 => {
            if current.owned.len() >= 128 && !current.owned.contains_key(&command.id) { current.owned.clear(); }
            current.owned.insert(command.id, native);
            MilestoneOutcome::Accepted
        }
        Ok(_) => MilestoneOutcome::Rejected,
        Err(error) if definitely_rejected(&error) => MilestoneOutcome::Rejected,
        Err(_) => MilestoneOutcome::Unknown,
    };
    let _ = command.receipt.send(outcome);
}

/// 只有协议明确拒绝才允许核心重试；NoReply/TimedOut 可能已经显示，必须保持未知。
fn definitely_rejected(error: &ashpd::zbus::Error) -> bool {
    matches!(error, ashpd::zbus::Error::MethodError(name, _, _) if matches!(name.as_str(),
        "org.freedesktop.DBus.Error.AccessDenied" | "org.freedesktop.DBus.Error.AuthFailed"
        | "org.freedesktop.DBus.Error.InvalidArgs" | "org.freedesktop.DBus.Error.UnknownMethod"
        | "org.freedesktop.DBus.Error.UnknownInterface" | "org.freedesktop.DBus.Error.UnknownObject"
        | "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
        | "org.freedesktop.DBus.Error.NotSupported"))
}

/// 桌面 ID 映射最多128项，与现有瞬时邮箱上限一致；退出取消整个 worker 和信号订阅。
async fn run(mut receiver: mpsc::Receiver<Delivery>, clicked: Arc<AtomicBool>) {
    run_session(&mut receiver, clicked, None).await;
}

/// 同一协议循环支持隔离总线回归，测试不会替换 HOME 或访问用户通知服务。
async fn run_session(receiver: &mut mpsc::Receiver<Delivery>, clicked: Arc<AtomicBool>, mut session: Option<Session>) {
    loop {
        if session.is_none() {
            let Some(command) = receiver.next().await else { break; };
            deliver(&mut session, command).await;
            continue;
        }
        futures::select! {
            command = receiver.next().fuse() => {
                let Some(command) = command else { break; };
                deliver(&mut session, command).await;
            },
            signal = session.as_mut().unwrap().actions.next().fuse() => {
                let Some(signal) = signal else { session = None; continue; };
                let current = session.as_ref().unwrap();
                if let Ok((native, action)) = signal.body().deserialize::<(u32, String)>()
                    && signal.header().sender().is_some_and(|sender| sender.as_str() == current.owner)
                    && action == "default" && current.owned.values().any(|owned| *owned == native) {
                        clicked.store(true, Ordering::Release);
                    }
            },
        }
    }
}

/// 通知文字按纯文本发送；图标使用已批准像素，桌面 ID 与最终包入口一致。
async fn notify(proxy: &Proxy<'_>, replacing: u32, title: &str, body: &str) -> ashpd::zbus::Result<u32> {
    let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
    hints.insert("desktop-entry", Value::from("com.loki-metris.app"));
    hints.insert("suppress-sound", Value::from(true));
    hints.insert("image-data", Value::from((32i32, 32i32, 128i32, true, 8i32, 4i32, include_bytes!("../../../assets/tray.rgba").to_vec())));
    let actions = ["default", "LokiMetris"];
    proxy.call("Notify", &("LokiMetris", replacing, "", title, escape_markup(body), actions.as_slice(), hints, 10_000i32)).await
}

/// freedesktop 支持的标记只承载固定正文；Agent名称或数字中的字符不能成为桌面标记。
fn escape_markup(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
