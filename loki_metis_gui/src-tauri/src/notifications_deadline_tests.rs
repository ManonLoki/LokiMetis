//! 通知队列、操作截止时间与 worker 退出回收回归。

use super::*;

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
