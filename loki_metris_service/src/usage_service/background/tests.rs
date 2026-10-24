//! 后台任务真实终态与异常关闭回归。
use super::*;

impl BackgroundTaskOwner {
    /// 绑定发现协调器，使退出可先发出业务取消再等待任务。
    pub(crate) fn new(root_discovery: Arc<RootDiscoveryCoordinator>) -> Self {
        Self::with_runtime(root_discovery,tokio::runtime::Handle::current())
    }
}
    use loki_metris_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

    /// future 真正销毁时发信，证明 abort 之后 owner 等到了 JoinHandle 终态。
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        /// 测试守卫销毁时通知等待方，证明 future 已真正进入终态。
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    /// owner 关闭必须同时取消根发现、唤醒异步任务并回收句柄。
    #[tokio::test]
    async fn shutdown_cancels_root_discovery_and_reaps_owned_task() {
        let discovery = Arc::new(RootDiscoveryCoordinator::default());
        assert!(discovery.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        let owner = BackgroundTaskOwner::new(Arc::clone(&discovery));
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn("cooperative-test", move |mut shutdown| async move {
                shutdown.cancelled().await;
                let _ = finished_tx.send(());
            })
            .expect("task registers");

        owner.shutdown_with_timeout(Duration::from_secs(1)).await;

        assert!(discovery.is_cancel_requested());
        finished_rx.await.expect("owned task observes shutdown");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 不合作的异步任务在合作阶段后必须被中止，并在总时限内确认 future 已销毁。
    #[tokio::test]
    async fn shutdown_aborts_and_reaps_task_before_returning() {
        let discovery = Arc::new(RootDiscoveryCoordinator::default());
        let owner = BackgroundTaskOwner::new(discovery);
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn("pending-test", move |_shutdown| async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("task registers");
        started_rx.await.expect("task starts");

        tokio::time::timeout(
            Duration::from_secs(1),
            owner.shutdown_with_timeout(Duration::from_millis(50)),
        )
        .await
        .expect("shutdown stays bounded");
        dropped_rx
            .await
            .expect("aborted future reaches terminal state");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 已开始的 blocking 闭包必须通过关闭观察端合作返回，不能用 abort 冒充回收。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_cooperatively_reaps_started_blocking_task() {
        let discovery = Arc::new(RootDiscoveryCoordinator::default());
        let owner = BackgroundTaskOwner::new(discovery);
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn_blocking("blocking-test", move |shutdown| {
                while !shutdown.is_cancelled() {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                let _ = finished_tx.send(());
            })
            .expect("blocking task registers");

        owner.shutdown_with_timeout(Duration::from_secs(1)).await;

        finished_rx
            .await
            .expect("blocking task observes cooperative shutdown");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 关闭必须先触发阻塞任务登记的专用取消令牌，使深层循环无需轮询 watch。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_invokes_blocking_task_cancel_hook() {
        use loki_metris_core::ScanCancellation;

        let discovery = Arc::new(RootDiscoveryCoordinator::default());
        let owner = BackgroundTaskOwner::new(discovery);
        let cancellation = ScanCancellation::new();
        let shutdown_cancellation = cancellation.clone();
        let task_cancellation = cancellation.clone();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn_blocking_cancelable(
                "cancel-hook-test",
                move || shutdown_cancellation.cancel(),
                move |_shutdown| {
                    while !task_cancellation.is_cancelled() {
                        std::thread::park_timeout(Duration::from_millis(1));
                    }
                    let _ = finished_tx.send(());
                },
            )
            .expect("cancelable blocking task registers");

        owner.shutdown_with_timeout(Duration::from_secs(1)).await;

        assert!(cancellation.is_cancelled());
        finished_rx.await.expect("cancel hook reaches worker");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 无法在总时限内合作返回的 blocking 任务必须仍由 owner 保存真实句柄。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_retains_overdue_blocking_handle_until_it_finishes() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let discovery = Arc::new(RootDiscoveryCoordinator::default());
        let owner = BackgroundTaskOwner::new(discovery);
        let release = Arc::new(AtomicBool::new(false));
        let task_release = Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn_blocking("overdue-blocking-test", move |_shutdown| {
                let _ = started_tx.send(());
                while !task_release.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            })
            .expect("blocking task registers");
        started_rx.await.expect("blocking task starts");

        owner.shutdown_with_timeout(Duration::from_millis(20)).await;
        assert_eq!(owner.owned_task_count(), 1);

        release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            while owner.owned_task_count() != 0 {
                owner.shutdown_with_timeout(Duration::ZERO).await;
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("finished blocking handle is eventually reaped");
    }

    /// 关闭 future 被取消时，批次中尚未终态的 blocking 句柄必须交还原 owner。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_shutdown_future_restores_unfinished_handle_to_owner() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let owner = Arc::new(BackgroundTaskOwner::new(Arc::new(
            RootDiscoveryCoordinator::default(),
        )));
        let release = Arc::new(AtomicBool::new(false));
        let task_release = Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
        let cancelled_tx = Arc::new(Mutex::new(Some(cancelled_tx)));
        let task_cancelled_tx = Arc::clone(&cancelled_tx);
        owner
            .spawn_blocking_cancelable(
                "cancelled-shutdown-owner-test",
                move || {
                    if let Some(sender) = task_cancelled_tx
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        let _ = sender.send(());
                    }
                },
                move |_shutdown| {
                    let _ = started_tx.send(());
                    while !task_release.load(Ordering::Acquire) {
                        std::thread::park_timeout(Duration::from_millis(1));
                    }
                },
            )
            .expect("cancelable blocking task registers");
        started_rx.await.expect("blocking task starts");

        let shutdown_owner = Arc::clone(&owner);
        let shutdown_task = tokio::spawn(async move {
            shutdown_owner
                .shutdown_with_timeout(Duration::from_secs(10))
                .await;
        });
        cancelled_rx
            .await
            .expect("shutdown takes ownership and invokes cancellation");
        shutdown_task.abort();
        let join_error = shutdown_task
            .await
            .expect_err("shutdown task is deterministically cancelled");
        assert!(join_error.is_cancelled());
        assert_eq!(owner.owned_task_count(), 1);

        release.store(true, Ordering::Release);
        tokio::time::timeout(
            Duration::from_secs(1),
            owner.shutdown_with_timeout(Duration::ZERO),
        )
        .await
        .expect("restored handle is reaped without extending the original deadline");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 关闭批次建立后的 panic 必须经守卫恢复全部未终态句柄。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panic_during_shutdown_restores_unfinished_handle_to_owner() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let release = Arc::new(AtomicBool::new(false));
        let task_release = Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn_blocking("panicked-shutdown-owner-test", move |_shutdown| {
                let _ = started_tx.send(());
                while !task_release.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            })
            .expect("blocking task registers");
        started_rx.await.expect("blocking task starts");

        let (batch, _) = owner.begin_shutdown(Duration::from_secs(10));
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _batch = batch;
            panic!("expected shutdown panic");
        }));
        assert!(unwind.is_err());
        assert_eq!(owner.owned_task_count(), 1);

        release.store(true, Ordering::Release);
        tokio::time::timeout(
            Duration::from_secs(1),
            owner.shutdown_with_timeout(Duration::ZERO),
        )
        .await
        .expect("panic-restored handle is reaped without extending the original deadline");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// owner 异常 Drop 时未结束的 blocking 句柄必须转交进程级 owner 而非 detach。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drop_transfers_unfinished_blocking_task_to_process_owner() {
        const TASK_NAME: &str = "drop-retained-blocking-test";

        use std::sync::atomic::{AtomicBool, Ordering};

        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        let release = Arc::new(AtomicBool::new(false));
        let task_release = Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn_blocking(TASK_NAME, move |_shutdown| {
                let _ = started_tx.send(());
                while !task_release.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            })
            .expect("blocking task registers");
        started_rx.await.expect("blocking task starts");

        drop(owner);

        assert!(BackgroundTaskOwner::process_owns_task_named(TASK_NAME));
        release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            while BackgroundTaskOwner::process_owns_task_named(TASK_NAME) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("process owner eventually reaps the retained terminal task");
    }

    /// 已结束任务的 panic 必须被终态分类为可观察失败。
    #[tokio::test]
    async fn completed_task_panic_is_observed() {
        let result = tokio::spawn(async {
            panic!("expected background task panic");
        })
        .await;

        assert!(log_owned_task_join_result("panic-test", result));
    }

    /// 新任务登记应回收已完成句柄，避免后台任务表随短任务次数增长。
    #[tokio::test]
    async fn spawning_reaps_finished_handles_before_registration() {
        let owner = BackgroundTaskOwner::new(Arc::new(RootDiscoveryCoordinator::default()));
        owner
            .spawn("finished-test", |_shutdown| async {})
            .expect("finished task registers");
        tokio::time::timeout(Duration::from_secs(1), async {
            while !owner.lock_state().tasks[0].handle.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first task reaches terminal state");

        owner
            .spawn("pending-test", |_shutdown| async {
                std::future::pending::<()>().await;
            })
            .expect("second task registers after reaping");

        assert_eq!(owner.owned_task_count(), 1);
        owner.shutdown_with_timeout(Duration::from_millis(50)).await;
    }

