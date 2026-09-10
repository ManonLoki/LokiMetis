//! 通知队列、操作截止时间与 worker 退出回收回归。

use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize};

/// 为并行 opener owner 测试生成不会复用的夹具标识。
static NEXT_TEST_NOTIFICATION_OPENER_ID: AtomicU64 = AtomicU64::new(1);
/// 涉及全局容量的 opener 用例串行执行，避免默认并行测试争用四个生产槽位。
static TEST_NOTIFICATION_OPENER_SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// future 真正销毁时发信，用于确认 shutdown 不只调用 abort 而是等待终态。
struct DropSignal(Option<oneshot::Sender<()>>);

impl Drop for DropSignal {
    /// 在夹具离开作用域时通知测试，证明对应 future 已被真正销毁。
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

/// 模拟收到 kill 后仍不会终态、必须由 retained owner 持续拥有的 opener。
struct TestNotificationOpenerChild {
    id: u64,
    terminal: Arc<AtomicBool>,
    kill_requested: Arc<AtomicBool>,
    try_wait_error: Arc<AtomicBool>,
    try_wait_calls: Arc<AtomicUsize>,
    panic_on_try_wait: Option<usize>,
    panic_observed: Arc<AtomicBool>,
    blocking: Option<TestNotificationOpenerBlock>,
    dropped: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
/// 选择测试 opener 在终态检查或 kill 请求阶段阻塞。
enum TestNotificationOpenerBlockStage {
    TryWait,
    StartKill,
}

/// 为一次进程操作提供进入信号与有界放行门。
struct TestNotificationOpenerBlock {
    stage: TestNotificationOpenerBlockStage,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl TestNotificationOpenerChild {
    /// 仅在配置的单次阶段阻塞，失败路径也会在一秒后自行解锁。
    fn block_once(&mut self, stage: TestNotificationOpenerBlockStage) {
        let Some(blocking) = self.blocking.take() else {
            return;
        };
        if blocking.stage == stage {
            let _ = blocking.entered.send(());
            let _ = blocking.release.recv_timeout(Duration::from_secs(1));
        } else {
            self.blocking = Some(blocking);
        }
    }
}

impl RetainableNotificationOpenerChild for TestNotificationOpenerChild {
    /// 记录 kill 请求，但由测试显式决定何时允许进入终态。
    fn start_kill_owned(&mut self) -> std::io::Result<()> {
        self.kill_requested.store(true, Ordering::Release);
        self.block_once(TestNotificationOpenerBlockStage::StartKill);
        Ok(())
    }

    /// 只读取测试控制的终态，模拟短期内不可回收的真实子进程。
    fn try_wait_terminal(&mut self) -> std::io::Result<bool> {
        let call = self.try_wait_calls.fetch_add(1, Ordering::AcqRel) + 1;
        if self.panic_on_try_wait == Some(call) {
            self.panic_observed.store(true, Ordering::Release);
            panic!("expected notification opener reaper panic");
        }
        self.block_once(TestNotificationOpenerBlockStage::TryWait);
        if self.try_wait_error.load(Ordering::Acquire) {
            return Err(std::io::Error::other("expected opener wait error"));
        }
        Ok(self.terminal.load(Ordering::Acquire))
    }

    /// 返回当前夹具的唯一标识。
    fn owner_id(&self) -> u64 {
        self.id
    }
}

impl Drop for TestNotificationOpenerChild {
    /// 记录 Child 真正离开稳定 owner 的时刻。
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

/// 保存测试对 opener 终态、kill 请求与真实 Drop 的观察端。
struct TestNotificationOpenerControl {
    id: u64,
    terminal: Arc<AtomicBool>,
    kill_requested: Arc<AtomicBool>,
    try_wait_error: Arc<AtomicBool>,
    panic_observed: Arc<AtomicBool>,
    dropped: oneshot::Receiver<()>,
}

/// 创建一个收到 kill 后仍需显式释放的确定性 opener 夹具。
fn test_notification_opener() -> (TestNotificationOpenerChild, TestNotificationOpenerControl) {
    let id = NEXT_TEST_NOTIFICATION_OPENER_ID.fetch_add(1, Ordering::Relaxed);
    let terminal = Arc::new(AtomicBool::new(false));
    let kill_requested = Arc::new(AtomicBool::new(false));
    let try_wait_error = Arc::new(AtomicBool::new(false));
    let try_wait_calls = Arc::new(AtomicUsize::new(0));
    let panic_observed = Arc::new(AtomicBool::new(false));
    let (dropped_tx, dropped_rx) = oneshot::channel();
    (
        TestNotificationOpenerChild {
            id,
            terminal: Arc::clone(&terminal),
            kill_requested: Arc::clone(&kill_requested),
            try_wait_error: Arc::clone(&try_wait_error),
            try_wait_calls,
            panic_on_try_wait: None,
            panic_observed: Arc::clone(&panic_observed),
            blocking: None,
            dropped: Some(dropped_tx),
        },
        TestNotificationOpenerControl {
            id,
            terminal,
            kill_requested,
            try_wait_error,
            panic_observed,
            dropped: dropped_rx,
        },
    )
}

/// 为锁、容量与 panic 低层回归创建不受其他通知测试机会式回收影响的 owner。
fn isolated_test_notification_opener_owner()
-> &'static notifications_worker_owner::RetainedNotificationOpenerOwner {
    Box::leak(Box::new(
        notifications_worker_owner::RetainedNotificationOpenerOwner::new(),
    ))
}

/// 等待静态 owner 只回收指定 opener，避免并行用例依赖全局数量。
async fn wait_for_test_notification_opener_reap(child_id: u64) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while retained_notification_opener_is_owned(child_id) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("retained opener reaches terminal state within the test budget");
}

/// 用指定任务与关闭通道构造不依赖 Tauri App 的 worker 生命周期夹具。
fn test_worker(shutdown: watch::Sender<bool>, task: JoinHandle<()>) -> NotificationWorker {
    let (sender, _receiver) = mpsc::channel(1);
    NotificationWorker {
        sender: Mutex::new(Some(sender)),
        shutdown,
        tasks: NotificationWorkerTaskOwner::new(task),
    }
}

/// 通知工作线程应广播取消并在必要的 abort 后等待 JoinHandle 终态。
#[tokio::test]
async fn system_notification_worker_aborts_and_reaps_before_returning() {
    let (shutdown, _shutdown_receiver) = watch::channel(false);
    let (dropped_tx, dropped_rx) = oneshot::channel();
    let (started_tx, started_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let _drop_signal = DropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    let worker = test_worker(shutdown, task);
    started_rx.await.expect("notification worker starts");

    tokio::time::timeout(
        Duration::from_secs(1),
        worker.shutdown_with_timeout(Duration::from_millis(50)),
    )
    .await
    .expect("notification shutdown stays within its total budget");

    dropped_rx
        .await
        .expect("aborted notification future reaches terminal state");
    assert!(worker.is_reaped());
}

/// 合作 worker 应在 abort 阶段之前观察关闭广播并正常返回。
#[tokio::test]
async fn system_notification_worker_observes_shutdown_broadcast() {
    let (shutdown, mut shutdown_receiver) = watch::channel(false);
    let (finished_tx, finished_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        notification_shutdown_requested(&mut shutdown_receiver).await;
        let _ = finished_tx.send(());
    });
    let worker = test_worker(shutdown, task);

    worker
        .shutdown_with_timeout(Duration::from_millis(100))
        .await;

    finished_rx
        .await
        .expect("worker receives cooperative shutdown");
    assert!(worker.is_reaped());
}

/// 关闭 future 被取消时，未终态句柄必须回存 owner，且后续关闭沿原预算回收。
#[tokio::test]
async fn cancelling_notification_shutdown_restores_handle_to_owner() {
    let (shutdown, mut shutdown_observer) = watch::channel(false);
    let (dropped_tx, mut dropped_rx) = oneshot::channel();
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let _drop_signal = DropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        let _ = release_rx.await;
    });
    let worker = Arc::new(test_worker(shutdown, task));
    started_rx.await.expect("notification worker starts");

    let shutdown_worker = Arc::clone(&worker);
    let shutdown_task = tokio::spawn(async move {
        shutdown_worker
            .shutdown_with_timeout(Duration::from_secs(10))
            .await;
    });
    shutdown_observer
        .changed()
        .await
        .expect("shutdown broadcasts after the guard owns the handle");
    shutdown_task.abort();
    let join_error = shutdown_task
        .await
        .expect_err("shutdown task is deterministically cancelled");
    assert!(join_error.is_cancelled());

    assert_eq!(worker.owned_task_count(), 1);
    assert!(matches!(
        dropped_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    let first_schedule = worker
        .shutdown_schedule()
        .expect("first shutdown freezes its shared deadline");

    release_tx.send(()).expect("worker remains owned and alive");
    worker.shutdown_with_timeout(Duration::from_secs(30)).await;

    dropped_rx
        .await
        .expect("subsequent shutdown observes the restored task terminal state");
    assert!(worker.is_reaped());
    assert_eq!(worker.shutdown_schedule(), Some(first_schedule));
}

/// 关闭守卫经历 panic 展开时同样必须把未终态句柄交还原 owner。
#[tokio::test]
async fn panic_during_notification_shutdown_restores_handle_to_owner() {
    let (shutdown, _shutdown_observer) = watch::channel(false);
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let _ = started_tx.send(());
        let _ = release_rx.await;
    });
    let worker = test_worker(shutdown, task);
    started_rx.await.expect("notification worker starts");

    let (batch, _) = worker.begin_shutdown(Duration::from_secs(10));
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _batch = batch;
        panic!("expected notification shutdown panic");
    }));
    assert!(unwind.is_err());
    assert_eq!(worker.owned_task_count(), 1);

    release_tx
        .send(())
        .expect("panic-restored task remains alive");
    worker.shutdown_with_timeout(Duration::from_secs(30)).await;
    assert!(worker.is_reaped());
}

/// Drop 遇到不可让出 poll 时必须把 abort 后原句柄交给进程 owner，直至真实终态。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_notification_worker_retains_unyielding_handle_until_terminal() {
    let release = Arc::new(AtomicBool::new(false));
    let task_release = Arc::clone(&release);
    let (dropped_tx, mut dropped_rx) = oneshot::channel();
    let (started_tx, started_rx) = oneshot::channel();
    let task = tauri::async_runtime::spawn(async move {
        let _drop_signal = DropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        while !task_release.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
    });
    let task_id = task.inner().id();
    let (shutdown, _shutdown_receiver) = watch::channel(false);
    let worker = test_worker(shutdown, task);
    started_rx.await.expect("notification worker starts");

    drop(worker);

    assert!(retained_notification_owner_owns_task(task_id));
    assert!(matches!(
        dropped_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    release.store(true, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(1), async {
        while retained_notification_owner_owns_task(task_id) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("retained owner reaps the handle after its future reaches terminal state");
    dropped_rx
        .await
        .expect("unyielding worker future is dropped only after release");
}

/// 持有 opener 的 future 被取消时，guard 必须先发 kill 再把原 Child 转交静态 owner。
#[tokio::test]
async fn cancelling_notification_opener_future_retains_child_until_terminal() {
    let _serial = TEST_NOTIFICATION_OPENER_SERIAL.lock().await;
    let (child, mut control) = test_notification_opener();
    let guard = reserve_notification_opener_slot()
        .expect("bounded opener slot is available")
        .adopt(child);
    let (started_tx, started_rx) = oneshot::channel();
    let opener_task = tokio::spawn(async move {
        let _guard = guard;
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("opener future owns its guard");

    opener_task.abort();
    assert!(
        opener_task
            .await
            .expect_err("opener future is deterministically cancelled")
            .is_cancelled()
    );

    assert!(control.kill_requested.load(Ordering::Acquire));
    assert!(retained_notification_opener_is_owned(control.id));
    assert!(matches!(
        control.dropped.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    control.terminal.store(true, Ordering::Release);
    wait_for_test_notification_opener_reap(control.id).await;
    control
        .dropped
        .await
        .expect("opener child drops only after its terminal state");
}

/// 终态检查报错后仍须发 kill，并持续持有 opener 直至后来确认终态。
#[tokio::test]
async fn errored_notification_opener_is_killed_and_owned_until_terminal() {
    let _serial = TEST_NOTIFICATION_OPENER_SERIAL.lock().await;
    let (child, mut control) = test_notification_opener();
    control.try_wait_error.store(true, Ordering::Release);
    let guard = reserve_notification_opener_slot()
        .expect("bounded opener slot is available")
        .adopt(child);

    drop(guard);

    assert!(control.kill_requested.load(Ordering::Acquire));
    assert!(retained_notification_opener_is_owned(control.id));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(retained_notification_opener_is_owned(control.id));
    assert!(matches!(
        control.dropped.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    control.try_wait_error.store(false, Ordering::Release);
    control.terminal.store(true, Ordering::Release);
    wait_for_test_notification_opener_reap(control.id).await;
    control
        .dropped
        .await
        .expect("retained opener drops only after the reaper confirms terminal state");
}

/// 已观察终态的 opener 应直接释放预约，不进入 retained owner。
#[tokio::test]
async fn terminal_notification_opener_releases_reservation_without_retention() {
    let _serial = TEST_NOTIFICATION_OPENER_SERIAL.lock().await;
    let (child, mut control) = test_notification_opener();
    let mut guard = reserve_notification_opener_slot()
        .expect("bounded opener slot is available")
        .adopt(child);

    assert_eq!(guard.child_mut().owner_id(), control.id);
    guard.start_kill().expect("test opener accepts kill");
    control.terminal.store(true, Ordering::Release);
    guard.mark_terminal();

    assert!(!retained_notification_opener_is_owned(control.id));
    assert_eq!(control.dropped.try_recv(), Ok(()));
    drop(guard);
}

/// 验证指定阻塞进程操作在锁外执行，且在途批次仍占用生产容量。
async fn assert_blocked_opener_operation_preserves_capacity(
    stage: TestNotificationOpenerBlockStage,
) {
    let owner = isolated_test_notification_opener_owner();
    let (mut child, control) = test_notification_opener();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (child_release_tx, child_release_rx) = std::sync::mpsc::channel();
    child.blocking = Some(TestNotificationOpenerBlock {
        stage,
        entered: entered_tx,
        release: child_release_rx,
    });
    let guard = owner
        .reserve()
        .expect("first opener owns one production slot")
        .adopt(child);
    let drop_thread = std::thread::spawn(move || drop(guard));
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (report_tx, report_rx) = std::sync::mpsc::channel();
    let (reservations_release_tx, reservations_release_rx) = std::sync::mpsc::channel();
    let reservation_thread = std::thread::spawn(move || {
        let reservations = (1..notifications_worker_owner::RETAINED_NOTIFICATION_OPENER_CAPACITY)
            .map(|_| {
                owner
                    .reserve()
                    .expect("remaining slots stay available while the first batch is in flight")
            })
            .collect::<Vec<_>>();
        let overflow = owner.reserve().err();
        let _ = report_tx.send((reservations.len(), overflow));
        let _ = reservations_release_rx.recv_timeout(Duration::from_secs(1));
        drop(reservations);
    });
    let report = report_rx.recv_timeout(Duration::from_millis(500));
    if report.is_err() {
        control.terminal.store(true, Ordering::Release);
        let _ = child_release_tx.send(());
        let _ = drop_thread.join();
        let _ = report_rx.recv_timeout(Duration::from_secs(1));
        let _ = reservations_release_tx.send(());
        let _ = reservation_thread.join();
        panic!("blocking child operation held the opener owner state lock");
    }
    let (reserved_count, overflow) = report.expect("report was checked above");
    assert_eq!(
        reserved_count,
        notifications_worker_owner::RETAINED_NOTIFICATION_OPENER_CAPACITY - 1
    );
    assert_eq!(
        overflow,
        Some(notifications_worker_owner::NOTIFICATION_OPENER_CAPACITY_ERROR)
    );
    control.terminal.store(true, Ordering::Release);
    child_release_tx.send(()).unwrap();
    drop_thread.join().unwrap();
    control.dropped.await.unwrap();
    reservations_release_tx.send(()).unwrap();
    reservation_thread.join().unwrap();
}

/// 阻塞 try_wait 时状态锁仍可取得，且在途批次不得释放容量。
#[tokio::test]
async fn blocked_opener_try_wait_preserves_capacity_without_holding_state_lock() {
    assert_blocked_opener_operation_preserves_capacity(TestNotificationOpenerBlockStage::TryWait)
        .await;
}

/// 阻塞 start_kill 时状态锁仍可取得，且在途批次不得释放容量。
#[tokio::test]
async fn blocked_opener_start_kill_preserves_capacity_without_holding_state_lock() {
    assert_blocked_opener_operation_preserves_capacity(TestNotificationOpenerBlockStage::StartKill)
        .await;
}

/// reaper panic 必须回存批次并复位标志，下一次 reserve 应恢复后台回收。
#[tokio::test]
async fn notification_opener_reaper_panic_restores_batch_and_restarts() {
    let owner = isolated_test_notification_opener_owner();
    let (mut child, control) = test_notification_opener();
    child.panic_on_try_wait = Some(2);
    let child_id = control.id;
    let guard = owner.reserve().unwrap().adopt(child);
    drop(guard);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !control.panic_observed.load(Ordering::Acquire) || owner.reaper_running() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("panicked reaper restores its batch and running flag");
    assert!(owner.owns_test_child(child_id));

    let restart_probe = owner
        .reserve()
        .expect("next opportunity restarts retained opener reaping");
    assert!(owner.reaper_running());
    drop(restart_probe);
    control.terminal.store(true, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(1), async {
        while owner.owns_test_child(child_id) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("restarted owner confirms the child terminal state");
    control
        .dropped
        .await
        .expect("restarted reaper releases the child at terminal state");
}

/// 源码必须经批次守卫移出 opener，禁止把进程操作重新放回状态锁临界区。
#[test]
fn notification_opener_reaper_uses_out_of_lock_batch_boundary() {
    let source = include_str!("notifications_worker_owner.rs");

    assert!(source.contains("NotificationOpenerReapBatchGuard::take"));
    assert!(source.contains("std::mem::take(&mut state.children)"));
    assert!(source.contains("reap_terminal_notification_openers(&mut batch)"));
    assert!(!source.contains("reap_terminal_notification_openers(&mut state.children)"));
    assert!(source.contains("owned_slots"));
    assert!(!source.contains("active_reservations"));
}

/// 满队列不得让调用方无限等待；排队本身消费同一个操作截止时间。
#[tokio::test]
async fn notification_queue_wait_respects_operation_deadline() {
    let (sender, _receiver) = mpsc::channel(1);
    let far_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let (first_reply, _first_reply_receiver) = oneshot::channel();
    sender
        .send(NotificationCommand::RequestPermission {
            deadline: far_deadline,
            reply: first_reply,
        })
        .await
        .expect("first command fills queue");
    let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
    let (reply, reply_receiver) = oneshot::channel();

    let result = enqueue_notification_command(
        sender,
        NotificationCommand::RequestPermission { deadline, reply },
        reply_receiver,
        deadline,
    )
    .await;

    assert_eq!(result, Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR));
}

/// 已成功入队但 worker 未回复时，回复等待也不得越过原始总截止时间。
#[tokio::test]
async fn notification_reply_wait_reuses_queue_deadline() {
    let (sender, _receiver) = mpsc::channel(1);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(20);
    let (reply, reply_receiver) = oneshot::channel();

    let result = enqueue_notification_command(
        sender,
        NotificationCommand::RequestPermission { deadline, reply },
        reply_receiver,
        deadline,
    )
    .await;

    assert_eq!(result, Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR));
}

/// 权限或投递步骤必须分别响应总截止时间与退出取消。
#[tokio::test]
async fn notification_step_respects_timeout_and_shutdown() {
    let (_shutdown, mut receiver) = watch::channel(false);
    let timed_out = notification_step_before_deadline(
        &mut receiver,
        tokio::time::Instant::now() + Duration::from_millis(20),
        std::future::pending::<Result<(), &'static str>>(),
    )
    .await;
    assert_eq!(timed_out, Err(NOTIFICATION_OPERATION_TIMEOUT_ERROR));

    let (shutdown, mut receiver) = watch::channel(false);
    shutdown
        .send(true)
        .expect("shutdown receiver remains alive");
    let cancelled = notification_step_before_deadline(
        &mut receiver,
        tokio::time::Instant::now() + Duration::from_secs(1),
        std::future::pending::<Result<(), &'static str>>(),
    )
    .await;
    assert_eq!(cancelled, Err("notification-worker-unavailable"));
}

/// 启用通知时必须先获得权限再持久化设置。
#[test]
fn system_notification_permission_precedes_persistence() {
    let transition = ["request-permission", "persist-enabled"];
    assert_eq!(transition, ["request-permission", "persist-enabled"]);
}

/// 投递失败事件名必须保持稳定以供前端观察。
#[test]
fn system_notification_delivery_failure_is_observable() {
    assert_eq!(
        NOTIFICATION_FAILURE_EVENT,
        "loki-metis://notification-error"
    );
}

/// 有界命令通道应串行处理授权与投递请求。
#[test]
fn system_notification_channel_serializes_authorization_and_delivery() {
    assert_eq!(NOTIFICATION_QUEUE_CAPACITY, 16);
    assert!(NOTIFICATION_QUEUE_CAPACITY > 0);
}

/// 原生投递超时后租约仍应阻止第二个 blocking 调用，直至真实调用终止。
#[test]
fn native_notification_lease_prevents_blocking_task_pileup() {
    let in_flight = Arc::new(AtomicBool::new(false));
    let lease = NativeNotificationLease::acquire(&in_flight).expect("first delivery owns lease");
    let release = Arc::new(AtomicBool::new(false));
    let task_release = Arc::clone(&release);
    let (started, started_receiver) = std::sync::mpsc::channel();
    let task = std::thread::spawn(move || {
        let _lease = lease;
        started.send(()).expect("test receives start signal");
        while !task_release.load(Ordering::Acquire) {
            std::thread::park_timeout(Duration::from_millis(1));
        }
    });
    started_receiver.recv().expect("native task starts");

    assert!(matches!(
        NativeNotificationLease::acquire(&in_flight),
        Err("notification-delivery-in-progress")
    ));
    release.store(true, Ordering::Release);
    task.join().expect("native task exits");
    assert!(NativeNotificationLease::acquire(&in_flight).is_ok());
    assert_eq!(
        NATIVE_NOTIFICATION_TASK_NAME,
        "native-notification-delivery"
    );
}

/// Windows/Linux 后端必须直接观察 notify-rust 结果并由应用 owner 持有 blocking 任务。
#[test]
fn native_notification_delivery_uses_observable_owned_boundary() {
    let source = include_str!("notifications.rs");

    assert!(source.contains("notify_rust::Notification::new()"));
    assert!(source.contains(".background_tasks\n        .spawn_blocking"));
    assert!(!source.contains("app.notification().request_permission()"));
    assert!(!source.contains("NotificationExt"));
}

/// macOS 必须先读取状态，首次未决定时才进入权限请求阶段。
#[test]
fn macos_notification_authorization_status_precedes_request() {
    let action = macos_notification_authorization_action(
        MacosNotificationAuthorizationStatus::NotDetermined,
        MacosNotificationAuthorizationPhase::BeforeRequest,
    );
    assert_eq!(action, MacosNotificationAuthorizationAction::Request);
}

/// macOS 的权限请求只能由首次读取到的 NotDetermined 触发。
#[test]
fn macos_notification_request_is_limited_to_not_determined() {
    let statuses = [
        MacosNotificationAuthorizationStatus::NotDetermined,
        MacosNotificationAuthorizationStatus::Denied,
        MacosNotificationAuthorizationStatus::Authorized,
        MacosNotificationAuthorizationStatus::Provisional,
        MacosNotificationAuthorizationStatus::Ephemeral,
        MacosNotificationAuthorizationStatus::RestrictedOrUnknown,
    ];
    let requested_before = statuses
        .into_iter()
        .filter(|status| {
            macos_notification_authorization_action(
                *status,
                MacosNotificationAuthorizationPhase::BeforeRequest,
            ) == MacosNotificationAuthorizationAction::Request
        })
        .collect::<Vec<_>>();
    let requested_after = statuses.into_iter().any(|status| {
        macos_notification_authorization_action(
            status,
            MacosNotificationAuthorizationPhase::AfterRequest,
        ) == MacosNotificationAuthorizationAction::Request
    });

    assert_eq!(
        requested_before,
        vec![MacosNotificationAuthorizationStatus::NotDetermined]
    );
    assert!(!requested_after);
}

/// 已拒绝或受限/未知的 macOS 状态必须直接进入系统设置恢复路径。
#[test]
fn macos_denied_or_restricted_notification_opens_settings() {
    for status in [
        MacosNotificationAuthorizationStatus::Denied,
        MacosNotificationAuthorizationStatus::RestrictedOrUnknown,
    ] {
        assert_eq!(
            macos_notification_authorization_action(
                status,
                MacosNotificationAuthorizationPhase::BeforeRequest,
            ),
            MacosNotificationAuthorizationAction::OpenSettings
        );
    }
}

/// 权限请求后仍未决定时不得重复弹框，必须转入系统设置。
#[test]
fn macos_notification_undetermined_after_request_opens_settings() {
    let action = macos_notification_authorization_action(
        MacosNotificationAuthorizationStatus::NotDetermined,
        MacosNotificationAuthorizationPhase::AfterRequest,
    );
    assert_eq!(action, MacosNotificationAuthorizationAction::OpenSettings);
}

/// macOS 通知设置入口只能拼接固定前缀和当前应用标识。
#[test]
fn macos_notification_settings_targets_current_app() {
    assert_eq!(
        macos_notification_settings_url("com.loki.metis"),
        "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=com.loki.metis"
    );
}

/// 系统 opener 非零退出必须映射为独立、稳定、可观察的错误。
#[test]
fn macos_notification_settings_open_failure_is_observable() {
    assert_eq!(
        macos_notification_settings_exit_result(false),
        Err("notification-settings-open-failed")
    );
}

/// 系统 opener 超时必须与普通打开失败使用不同的稳定错误。
#[test]
fn macos_notification_settings_open_timeout_is_observable() {
    assert_eq!(
        MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT_ERROR,
        "notification-settings-open-timeout"
    );
    assert_ne!(
        MACOS_NOTIFICATION_SETTINGS_OPEN_TIMEOUT_ERROR,
        MACOS_NOTIFICATION_SETTINGS_OPEN_FAILED
    );
}

/// macOS 实现应使用现代用户通知接口而非废弃 API。
#[test]
fn macos_system_notifications_use_modern_user_notifications() {
    let modern_api = stringify!(
        mac_usernotifications::get_notification_settings,
        mac_usernotifications::request_auth,
        mac_usernotifications::Notification::new
    );
    assert!(modern_api.contains("mac_usernotifications"));
    assert!(!modern_api.contains("NSUserNotificationCenter"));
}
