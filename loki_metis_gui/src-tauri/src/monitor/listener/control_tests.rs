//! Hook listener 的 shutdown、Drop 与最终 task owner 回归。

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// 模拟一次不让出执行器的 poll，用于证明 abort 不会被误当作已回收。
struct NonYieldingFuture {
    started: Option<tokio::sync::oneshot::Sender<()>>,
    release: Arc<AtomicBool>,
}

impl std::future::Future for NonYieldingFuture {
    /// 测试 future 只用于占住一次 poll，完成时不返回业务值。
    type Output = ();

    /// 首次 poll 发出已启动信号，然后等待测试显式释放。
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }
        while !self.release.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        std::task::Poll::Ready(())
    }
}

/// 构造带单一不让出任务的控制器并等待任务真正开始。
async fn non_yielding_control(release: Arc<AtomicBool>) -> HookListenerControl {
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&AiTool::ALL)));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (shutdown, _receiver) = watch::channel(false);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let task = tauri::async_runtime::spawn(NonYieldingFuture {
        started: Some(started_tx),
        release,
    });
    started_rx.await.expect("non-yielding task starts");
    HookListenerControl::new(policy, status, shutdown, vec![task])
}

/// 控制器必须同时唤醒合作任务并中止不合作任务。
#[tokio::test]
async fn shutdown_reaps_all_owned_hook_tasks() {
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&AiTool::ALL)));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (shutdown, receiver) = watch::channel(false);
    let mut cooperative_shutdown = HookListenerShutdown::new(receiver.clone());
    let cooperative = tauri::async_runtime::spawn(async move {
        cooperative_shutdown.cancelled().await;
    });
    let pending = tauri::async_runtime::spawn(std::future::pending::<()>());
    let control = HookListenerControl::new(policy, status, shutdown, vec![cooperative, pending]);

    control
        .shutdown_with_timeout(Duration::from_millis(10))
        .await;

    assert!(control.lock_tasks().tasks.is_empty());
    assert!(*receiver.borrow());
    assert!(
        hook_listener_policy(&control.policy)
            .enabled_tools
            .is_empty()
    );
}

/// abort 无法中断当前 poll 时，shutdown 必须有界移交并由 idle reaper 最终回收。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_transfers_non_yielding_task_to_idle_reaper() {
    let release = Arc::new(AtomicBool::new(false));
    let control = non_yielding_control(Arc::clone(&release)).await;
    let baseline = retained_hook_listener_task_count();

    tokio::time::timeout(
        Duration::from_secs(1),
        control.shutdown_with_timeout(Duration::from_millis(20)),
    )
    .await
    .expect("listener shutdown remains bounded");
    assert!(control.lock_tasks().tasks.is_empty());
    assert!(retained_hook_listener_task_count() >= baseline + 1);

    release.store(true, Ordering::Release);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while retained_hook_listener_task_count() > baseline && tokio::time::Instant::now() < deadline {
        tokio::task::yield_now().await;
    }
    assert!(retained_hook_listener_task_count() <= baseline);
}

/// 未先调用 shutdown 就 Drop control 时，也不能丢弃尚未终态的 Tokio 句柄。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drop_transfers_non_yielding_task_to_idle_reaper() {
    let release = Arc::new(AtomicBool::new(false));
    let control = non_yielding_control(Arc::clone(&release)).await;
    let baseline = retained_hook_listener_task_count();

    drop(control);
    assert!(retained_hook_listener_task_count() >= baseline + 1);
    release.store(true, Ordering::Release);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while retained_hook_listener_task_count() > baseline && tokio::time::Instant::now() < deadline {
        tokio::task::yield_now().await;
    }
    assert!(retained_hook_listener_task_count() <= baseline);
}
