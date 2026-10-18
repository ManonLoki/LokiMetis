//! 通过独立 D-Bus daemon 验证真实类型化协议、点击归属和系统失败，不显示桌面通知。

use super::*;
use ashpd::zbus::{self, interface, object_server::SignalEmitter, zvariant::OwnedValue};
use std::{io::{BufRead as _, BufReader}, process::{Child, Command, Stdio}, thread, time::Instant};

/// daemon 与测试同寿命；异常路径也会终止并回收隔离进程，不改用户会话环境。
struct PrivateBus { child: Child, address: String }
impl PrivateBus {
    /// 创建真实但不向外暴露的私有总线，地址只交给本测试的连接。
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("private dbus-daemon");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        assert!(!address.trim().is_empty());
        Self { child, address: address.trim().into() }
    }
    /// 客户端有限方法期限与生产相同，不因假服务卡住整个测试套件。
    async fn connection(&self) -> Connection {
        Builder::address(self.address.as_str()).unwrap().method_timeout(Duration::from_secs(2)).build().await.unwrap()
    }
}
impl Drop for PrivateBus {
    /// 不留下隔离 daemon 或僵尸进程。
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

/// 只记录回归所需外部调用事实，不复刻应用领域逻辑。
#[derive(Default)]
struct Observations { replacements: Vec<u32>, bodies: Vec<String>, calls: usize }
/// 测试用桌面通知服务，验证真实 D-Bus 参数并显式返回系统拒绝。
struct NotificationServer(Arc<Mutex<Observations>>);
#[interface(name = "org.freedesktop.Notifications", crate = "ashpd::zbus")]
impl NotificationServer {
    /// Linux 实际权限探测依赖服务可调用，不隐式发通知。
    fn get_capabilities(&self) -> Vec<&str> { vec!["body", "actions", "body-markup"] }

    /// 接收实际 Notify 参数并验证固定产品身份、动作和像素提示。
    #[allow(clippy::too_many_arguments)]
    fn notify(&self, app: &str, replacing: u32, icon: &str, title: &str, body: &str,
              actions: Vec<String>, hints: HashMap<String, OwnedValue>, timeout: i32) -> ashpd::zbus::fdo::Result<u32> {
        assert_eq!(app, "LokiMetris"); assert!(icon.is_empty()); assert_eq!(title, "通知");
        assert_eq!(actions, ["default", "LokiMetris"]); assert_eq!(timeout, 10_000);
        assert_eq!(String::try_from(hints["desktop-entry"].try_clone().unwrap()).unwrap(), "com.loki-metris.app");
        assert!(hints.contains_key("image-data"));
        assert!(bool::try_from(hints["suppress-sound"].try_clone().unwrap()).unwrap());
        let mut observed = self.0.lock().unwrap();
        observed.calls += 1; observed.replacements.push(replacing); observed.bodies.push(body.into());
        if body == "reject" { return Err(ashpd::zbus::fdo::Error::AccessDenied("test denial".into())); }
        if body == "no-reply" { return Err(ashpd::zbus::fdo::Error::NoReply("uncertain delivery".into())); }
        Ok(if replacing == 0 { 40 + observed.calls as u32 } else { replacing })
    }

    /// 信号采用系统协议格式，外来通知与未知动作不得恢复本应用。
    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action: &str) -> ashpd::zbus::Result<()>;
}

/// 正文即使包含类标记内容也只显示字面文本，不允许控制桌面通知外观。
#[test]
fn notification_body_is_plain_text() {
    assert_eq!(escape_markup("Codex <end> & done"), "Codex &lt;end&gt; &amp; done");
}

/// 真实协议 roundtrip 覆盖参数、同ID替换、系统拒绝、未知点击及退出回收。
#[test]
fn private_bus_delivers_notifications_and_filters_click_ownership() {
    futures::executor::block_on(async {
        let bus = PrivateBus::new();
        let observed = Arc::new(Mutex::new(Observations::default()));
        let server = Builder::address(bus.address.as_str()).unwrap().name("org.freedesktop.Notifications").unwrap()
            .serve_at("/org/freedesktop/Notifications", NotificationServer(observed.clone())).unwrap().build().await.unwrap();
        let session = Session::open(bus.connection().await).await.unwrap();
        let clicked = Arc::new(AtomicBool::new(false));
        let callback = clicked.clone();
        let (mut sender, mut receiver) = mpsc::channel(16);
        let actor = thread::spawn(move || futures::executor::block_on(run_session(&mut receiver, callback, Some(session))));
        for (id, body, expected) in [("same", "A < B & C", MilestoneOutcome::Accepted), ("same", "done", MilestoneOutcome::Accepted), ("other", "reject", MilestoneOutcome::Rejected), ("uncertain", "no-reply", MilestoneOutcome::Unknown)] {
            let (receipt, response) = oneshot::channel();
            sender.try_send(delivery(id, body, receipt)).unwrap();
            assert_eq!(response.await.unwrap(), expected);
        }
        let (replacements, bodies) = { let data = observed.lock().unwrap(); (data.replacements.clone(), data.bodies.clone()) };
        assert_eq!(replacements, [0, 41, 0, 0]); assert_eq!(bodies[0], "A &lt; B &amp; C");
        let (receipt, response) = oneshot::channel(); drop(response);
        sender.try_send(delivery("cancelled", "must not send", receipt)).unwrap();
        let (receipt, response) = oneshot::channel();
        let disabled = delivery("disabled", "must not send either", receipt);
        disabled.cancelled.store(true, Ordering::Release);
        sender.try_send(disabled).unwrap();
        assert_eq!(response.await.unwrap(), MilestoneOutcome::Rejected);
        let emitter = SignalEmitter::new(&server, "/org/freedesktop/Notifications").unwrap();
        NotificationServer::action_invoked(&emitter, 999, "default").await.unwrap();
        NotificationServer::action_invoked(&emitter, 41, "unknown").await.unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(!clicked.load(Ordering::Acquire));
        NotificationServer::action_invoked(&emitter, 41, "default").await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !clicked.load(Ordering::Acquire) && Instant::now() < deadline { thread::sleep(Duration::from_millis(5)); }
        assert!(clicked.load(Ordering::Acquire));
        drop(sender); actor.join().unwrap();
        assert_eq!(observed.lock().unwrap().calls, 4);
    });
}

/// 通知服务换所有者后重新绑定唯一名，不能把旧进程 ID 当作新服务的替换对象。
#[test]
fn private_bus_service_restart_discards_native_replacement_ids() {
    futures::executor::block_on(async {
        let bus = PrivateBus::new();
        let first = Arc::new(Mutex::new(Observations::default()));
        let server = Builder::address(bus.address.as_str()).unwrap().name("org.freedesktop.Notifications").unwrap()
            .serve_at("/org/freedesktop/Notifications", NotificationServer(first.clone())).unwrap().build().await.unwrap();
        let mut session = Some(Session::open(bus.connection().await).await.unwrap());
        let (receipt, response) = oneshot::channel();
        deliver(&mut session, delivery("same", "first", receipt)).await;
        assert_eq!(response.await.unwrap(), MilestoneOutcome::Accepted);
        let first_owner = session.as_ref().unwrap().owner.clone();
        server.release_name("org.freedesktop.Notifications").await.unwrap();
        let second = Arc::new(Mutex::new(Observations::default()));
        let replacement = Builder::address(bus.address.as_str()).unwrap().name("org.freedesktop.Notifications").unwrap()
            .serve_at("/org/freedesktop/Notifications", NotificationServer(second.clone())).unwrap().build().await.unwrap();
        let (receipt, response) = oneshot::channel();
        deliver(&mut session, delivery("same", "second", receipt)).await;
        assert_eq!(response.await.unwrap(), MilestoneOutcome::Accepted);
        assert_ne!(session.as_ref().unwrap().owner, first_owner);
        assert_eq!(first.lock().unwrap().replacements, [0]);
        assert_eq!(second.lock().unwrap().replacements, [0]);
        drop(replacement);
    });
}

/// 回归由显式期限信号控制，不让墙钟快慢决定协议预期。
fn delivery(id: &str, body: &str, receipt: oneshot::Sender<MilestoneOutcome>) -> Delivery {
    Delivery { id: id.into(), title: "通知".into(), body: body.into(), receipt,
        cancelled: Arc::new(AtomicBool::new(false)), expires: futures::future::pending().boxed() }
}

/// 已到期限的请求在任何连接/认证/Notify之前取消，丢失回执不会诱发确定失败重试。
#[test]
fn expired_delivery_never_connects_or_sends() {
    futures::executor::block_on(async {
        let (receipt, response) = oneshot::channel();
        let mut command = delivery("expired", "must not send", receipt);
        command.expires = futures::future::ready(()).boxed();
        let mut session = None;
        deliver(&mut session, command).await;
        assert!(session.is_none());
        assert!(response.await.is_err());
    });
}

/// 发送future在关闭前创建、关闭后才首次轮询，也必须零连接并确定拒绝。
#[gpui_kit::test]
fn disabling_notifications_revokes_delivery_before_future_first_poll(cx: &mut gpui_kit::TestAppContext) {
    let executor = cx.update(|cx| cx.background_executor().clone());
    let (events, _receiver) = std::sync::mpsc::sync_channel(1);
    let host = Host::new(events, executor).unwrap();
    host.set_delivery_allowed(true);
    let future = host.send("late".into(), "通知".into(), "must not send".into());
    host.set_delivery_allowed(false);
    cx.run_until_parked();
    assert_eq!(futures::executor::block_on(future), MilestoneOutcome::Rejected);
    assert!(host.pending.lock().unwrap().is_empty());
}

#[path = "linux_lifecycle_tests.rs"]
mod lifecycle;
