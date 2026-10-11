//! 回归后台任务 owner 的进程级硬容量与 RAII 槽位恢复。

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// 不可让出的 blocking worker 必须持续占槽，满额后不得调用新任务构造器。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unyielding_blocking_tasks_apply_stable_backpressure_until_terminal() {
    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        Arc::clone(&capacity),
    );
    let release = Arc::new(AtomicBool::new(false));
    for _ in 0..BACKGROUND_TASK_CAPACITY {
        let task_release = Arc::clone(&release);
        owner
            .spawn_blocking("capacity-blocking-test", move |_shutdown| {
                while !task_release.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            })
            .expect("each bounded slot registers once");
    }
    let rejected_builder_called = Arc::new(AtomicBool::new(false));
    let rejected_probe = Arc::clone(&rejected_builder_called);

    let error = owner
        .spawn_blocking("capacity-overflow-test", move |_shutdown| {
            rejected_probe.store(true, Ordering::Release);
        })
        .expect_err("the thirty-third task must receive stable backpressure");

    assert_eq!(error, BACKGROUND_TASK_CAPACITY_ERROR);
    assert!(!rejected_builder_called.load(Ordering::Acquire));
    assert_eq!(owner.owned_task_count(), BACKGROUND_TASK_CAPACITY);
    assert_eq!(
        capacity.owned.load(Ordering::Acquire),
        BACKGROUND_TASK_CAPACITY
    );

    release.store(true, Ordering::Release);
    owner.shutdown_with_timeout(Duration::from_secs(2)).await;
    assert_eq!(owner.owned_task_count(), 0);
    assert_eq!(capacity.owned.load(Ordering::Acquire), 0);

    let recovered_owner = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        capacity,
    );
    recovered_owner
        .spawn("capacity-recovered-test", |_shutdown| async {})
        .expect("a terminal batch releases capacity for later work");
    recovered_owner
        .shutdown_with_timeout(Duration::from_secs(1))
        .await;
    assert_eq!(recovered_owner.capacity.owned.load(Ordering::Acquire), 0);
}

/// 任务构造器 panic 时预约必须自动归还，owner 仍可继续使用。
#[tokio::test]
async fn panicking_spawn_builder_releases_reserved_slot() {
    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        Arc::clone(&capacity),
    );
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = owner.spawn(
            "capacity-panic-test",
            |_shutdown| -> std::future::Pending<()> { panic!("expected capacity builder panic") },
        );
    }));

    assert!(result.is_err());
    assert_eq!(capacity.owned.load(Ordering::Acquire), 0);
    owner
        .spawn("capacity-after-panic-test", |_shutdown| async {})
        .expect("the owner remains usable after builder panic");
    owner.shutdown_with_timeout(Duration::from_secs(1)).await;
    assert_eq!(capacity.owned.load(Ordering::Acquire), 0);
}

/// 异步任务构造器允许同步重入同一 owner，登记锁不得覆盖外部构造器调用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_spawn_builder_can_reenter_the_same_owner() {
    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner = Arc::new(BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        capacity,
    ));
    let registration_owner = Arc::clone(&owner);

    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::task::spawn_blocking(move || {
            let nested_owner = Arc::clone(&registration_owner);
            registration_owner.spawn("reentrant-outer-test", move |mut shutdown| {
                nested_owner
                    .spawn("reentrant-inner-test", |mut nested_shutdown| async move {
                        nested_shutdown.cancelled().await;
                    })
                    .expect("nested task registers while outer builder is running");
                async move {
                    shutdown.cancelled().await;
                }
            })
        }),
    )
    .await
    .expect("reentrant registration cannot deadlock")
    .expect("registration thread completes")
    .expect("outer task registers");

    assert_eq!(owner.owned_task_count(), 2);
    owner.shutdown_with_timeout(Duration::from_secs(1)).await;
    assert_eq!(owner.owned_task_count(), 0);
}

/// 一个合作取消钩子 panic 不得阻断其它任务接收关闭，也不得让 Drop 再次 panic。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panicking_cancel_hook_is_isolated_from_shutdown_broadcast() {
    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        capacity,
    );
    let (blocking_started_tx, blocking_started_rx) = tokio::sync::oneshot::channel();
    owner
        .spawn_blocking_cancelable(
            "panicking-cancel-hook-test",
            || panic!("expected cancel hook panic"),
            move |shutdown| {
                let _ = blocking_started_tx.send(());
                while !shutdown.is_cancelled() {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
            },
        )
        .expect("cancelable blocking task registers");
    blocking_started_rx.await.expect("blocking task starts");

    let (async_finished_tx, async_finished_rx) = tokio::sync::oneshot::channel();
    owner
        .spawn(
            "shutdown-broadcast-after-panic-test",
            move |mut shutdown| async move {
                shutdown.cancelled().await;
                let _ = async_finished_tx.send(());
            },
        )
        .expect("second task registers");

    owner.shutdown_with_timeout(Duration::from_secs(1)).await;
    async_finished_rx
        .await
        .expect("later task still receives shutdown after hook panic");
    assert_eq!(owner.owned_task_count(), 0);
}

/// owner A 无需先回收已完成句柄；执行体终态应立即让共享容量可供 owner B 使用。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_tasks_release_shared_capacity_before_handle_reap() {
    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner_a = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        Arc::clone(&capacity),
    );
    let release = Arc::new(AtomicBool::new(false));
    for _ in 0..BACKGROUND_TASK_CAPACITY {
        let task_release = Arc::clone(&release);
        owner_a
            .spawn("cross-owner-capacity-a-test", move |_shutdown| async move {
                while !task_release.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
            })
            .expect("owner A fills each shared slot");
    }
    assert_eq!(
        capacity.owned.load(Ordering::Acquire),
        BACKGROUND_TASK_CAPACITY
    );

    release.store(true, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(1), async {
        while capacity.owned.load(Ordering::Acquire) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal task bodies release slots without owner A activity");
    assert_eq!(owner_a.owned_task_count(), BACKGROUND_TASK_CAPACITY);

    let owner_b = BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        capacity,
    );
    owner_b
        .spawn("cross-owner-capacity-b-test", |_shutdown| async {})
        .expect("owner B can reuse capacity before owner A reaps handles");
    owner_b.shutdown_with_timeout(Duration::from_secs(1)).await;
    owner_a.shutdown_with_timeout(Duration::from_secs(1)).await;
    assert_eq!(owner_a.owned_task_count(), 0);
}

/// shutdown 可在锁外构造器执行期间完成；迟到句柄仍须被取消、保留到终态并归还槽位。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_race_cancels_late_spawn_without_detaching_its_handle() {
    const TASK_NAME: &str = "late-spawn-shutdown-race-test";

    let capacity = Arc::new(BackgroundTaskCapacity::default());
    let owner = Arc::new(BackgroundTaskOwner::new_with_capacity(
        Arc::new(RootDiscoveryCoordinator::default()),
        Arc::clone(&capacity),
    ));
    let build_release = Arc::new(AtomicBool::new(false));
    let task_owner = Arc::clone(&owner);
    let task_release = Arc::clone(&build_release);
    let (build_started_tx, build_started_rx) = tokio::sync::oneshot::channel();
    let registration = tokio::task::spawn_blocking(move || {
        task_owner.spawn(TASK_NAME, move |mut shutdown| {
            let _ = build_started_tx.send(());
            while !task_release.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            async move {
                shutdown.cancelled().await;
            }
        })
    });
    build_started_rx
        .await
        .expect("builder starts outside owner lock");

    tokio::time::timeout(
        Duration::from_millis(100),
        owner.shutdown_with_timeout(Duration::from_millis(50)),
    )
    .await
    .expect("shutdown is not blocked by the external builder");
    build_release.store(true, Ordering::Release);
    let error = registration
        .await
        .expect("registration thread completes")
        .expect_err("late adoption observes irreversible shutdown");
    assert_eq!(error, "background-task-owner-shutting-down");

    tokio::time::timeout(Duration::from_secs(1), async {
        while capacity.owned.load(Ordering::Acquire) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("aborted late future reaches terminal state and releases its slot");
    tokio::time::timeout(Duration::from_secs(1), async {
        while BackgroundTaskOwner::process_owns_task_named(TASK_NAME) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("process owner retains the late handle until its terminal state is observable");
    assert_eq!(owner.owned_task_count(), 0);
}
