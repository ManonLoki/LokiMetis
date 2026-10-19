//! 真实私有通知服务覆盖开关、退出与授权重试；actor首轮由屏障控制以精确重现迟到发送。

use super::*;
use crate::{native::notifications::{Service, driver}, preferences::{Preferences, Writer}};
use gpui_kit::TestAppContext;
use std::{rc::Rc, sync::mpsc as std_mpsc, thread::JoinHandle};

/// 仅测试拥有的总线与Notify服务；屏障不替代生产协议，只控制worker首次取出请求的时刻。
struct NativeFixture {
    /// 测试私有总线。
    _bus: PrivateBus,
    /// 模拟通知服务的连接。
    _server: Connection,
    /// 模拟服务观察到的调用。
    observed: Arc<Mutex<Observations>>,
    /// 被测通知宿主。
    host: Rc<Host>,
    /// 放行 worker 取出首个请求的屏障。
    start: Option<std_mpsc::Sender<()>>,
    /// 模拟服务线程。
    worker: Option<JoinHandle<()>>,
}

impl NativeFixture {
    /// 连接到真实隔离daemon并装配正式run_session，所有通知文字和ID都只存在于此私有服务。
    fn new(cx: &mut TestAppContext) -> Self {
        let bus = PrivateBus::new();
        let observed = Arc::new(Mutex::new(Observations::default()));
        let (server, session) = futures::executor::block_on(async {
            let server = Builder::address(bus.address.as_str()).unwrap().name("org.freedesktop.Notifications").unwrap()
                .serve_at("/org/freedesktop/Notifications", NotificationServer(observed.clone())).unwrap().build().await.unwrap();
            let session = Session::open(bus.connection().await).await.unwrap();
            (server, session)
        });
        let executor = cx.update(|cx| cx.background_executor().clone());
        let (sender, mut receiver) = mpsc::channel(16);
        let clicked = Arc::new(AtomicBool::new(false));
        let callback = clicked.clone();
        let (start, gate) = std_mpsc::channel();
        let worker = thread::spawn(move || {
            gate.recv().unwrap();
            futures::executor::block_on(run_session(&mut receiver, callback, Some(session)));
        });
        let host = Rc::new(Host { sender: Mutex::new(sender), clicked, pending: Mutex::new(Vec::new()),
            allowed: AtomicBool::new(false), revocation: AtomicU64::new(0), _worker: executor.spawn(async {}), executor });
        Self { _bus: bus, _server: server, observed, host, start: Some(start), worker: Some(worker) }
    }

    /// 已发出关闭或重新授权意图后才允许正式actor观察排队命令。
    fn start(&mut self) {
        if let Some(start) = self.start.take() { start.send(()).unwrap(); }
    }

    /// Service保留真实开关/权限状态，宿主只替换为本测试持有的私有协议连接。
    fn service(&self, permission: NotificationPermission) -> (Service, Writer) {
        let writer = Writer::in_memory(Preferences::default());
        let service = Service::new(true, writer.clone());
        {
            let mut owner = service.owner.borrow_mut();
            owner.host = Some(self.host.clone());
            owner.state.host_available = true;
            owner.state.permission = permission;
        }
        self.host.set_delivery_allowed(permission == NotificationPermission::Allowed);
        (service, writer)
    }
}

impl Drop for NativeFixture {
    /// 即使断言失败也关闭邮箱并放行屏障，join只等待本测试有限的协议工作。
    fn drop(&mut self) {
        self.host.sender.lock().unwrap().close_channel();
        self.start();
        if let Some(worker) = self.worker.take() { worker.join().unwrap(); }
    }
}

/// 创建后关闭再开启不能复活旧future；随后新通知确实通过相同真实D-Bus服务。
#[gpui_kit::test]
fn disable_then_enable_does_not_revive_unpolled_delivery(cx: &mut TestAppContext) {
    let mut fixture = NativeFixture::new(cx);
    fixture.host.set_delivery_allowed(true);
    let executor = cx.update(|cx| cx.background_executor().clone());
    let host = fixture.host.clone();
    let old = driver::send(&host, executor.clone(), "old".into(), "通知".into(), "stale".into());
    fixture.host.set_delivery_allowed(false);
    fixture.host.set_delivery_allowed(true);
    fixture.start();
    assert_eq!(futures::executor::block_on(old), MilestoneOutcome::Rejected);
    assert_eq!(fixture.observed.lock().unwrap().calls, 0);
    let fresh = driver::send(&fixture.host, executor, "fresh".into(), "通知".into(), "fresh".into());
    assert_eq!(futures::executor::block_on(fresh), MilestoneOutcome::Accepted);
    assert_eq!(fixture.observed.lock().unwrap().bodies, ["fresh"]);
}

/// 退出即刻撤销已创建的发送，即使回执future仍被外部保留且worker尚未取出请求。
#[gpui_kit::test]
fn service_stop_revokes_pending_delivery_before_notify(cx: &mut TestAppContext) {
    let mut fixture = NativeFixture::new(cx);
    let (service, writer) = fixture.service(NotificationPermission::Allowed);
    let executor = cx.update(|cx| cx.background_executor().clone());
    let host = fixture.host.clone();
    let pending = driver::send(&host, executor, "queued".into(), "通知".into(), "must not send".into());
    service.stop();
    fixture.start();
    assert_eq!(futures::executor::block_on(pending), MilestoneOutcome::Rejected);
    assert_eq!(fixture.observed.lock().unwrap().calls, 0);
    writer.finish();
}

/// 显式重试成功必须同步恢复发送门禁，随后的Notify不能继续沿用暂停状态。
#[gpui_kit::test]
fn successful_permission_retry_reenables_native_delivery(cx: &mut TestAppContext) {
    let mut fixture = NativeFixture::new(cx);
    let (service, writer) = fixture.service(NotificationPermission::Unavailable);
    cx.update(|cx| service.retry_permission_using(|_| async { Ok(()) }, cx));
    cx.run_until_parked();
    assert_eq!(service.state().permission, NotificationPermission::Allowed);
    assert!(!service.state().busy);
    fixture.start();
    let executor = cx.update(|cx| cx.background_executor().clone());
    let sent = driver::send(&fixture.host, executor, "after-retry".into(), "通知".into(), "resumed".into());
    assert_eq!(futures::executor::block_on(sent), MilestoneOutcome::Accepted);
    assert_eq!(fixture.observed.lock().unwrap().bodies, ["resumed"]);
    service.stop();
    writer.finish();
}
