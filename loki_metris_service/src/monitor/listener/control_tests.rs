//! Hook listener 的 shutdown、Drop 与最终 task owner 回归。

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// 模拟一次不让出执行器的 poll，用于证明 abort 不会被误当作已回收。
struct NonYieldingFuture {
    /// 首次 poll 时发送开始信号。
    started: Option<tokio::sync::oneshot::Sender<()>>,
    /// 置位后 future 才完成。
    release: Arc<AtomicBool>,
}

/// 在 async future 真正销毁时通知测试，区分 abort 请求与终态确认。
struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for DropSignal {
    /// 发送一次销毁信号；接收端提前离开时忽略错误。
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
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
    let task = tokio::spawn(NonYieldingFuture {
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
    let cooperative = tokio::spawn(async move {
        cooperative_shutdown.cancelled().await;
    });
    let pending = tokio::spawn(std::future::pending::<()>());
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
    let registration_id = control.lock_tasks().tasks[0].registration_id;

    tokio::time::timeout(
        Duration::from_secs(1),
        control.shutdown_with_timeout(Duration::from_millis(20)),
    )
    .await
    .expect("listener shutdown remains bounded");
    assert!(control.lock_tasks().tasks.is_empty());
    assert!(retained_hook_listener_owns_task(registration_id));

    release.store(true, Ordering::Release);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while retained_hook_listener_owns_task(registration_id)
        && tokio::time::Instant::now() < deadline
    {
        tokio::task::yield_now().await;
    }
    assert!(!retained_hook_listener_owns_task(registration_id));
}

/// shutdown future 被真实取消时，局部批次必须把句柄交还 control 并可再次回收。
#[tokio::test]
async fn cancelling_shutdown_future_restores_hook_task_to_control() {
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&AiTool::ALL)));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (shutdown, _receiver) = watch::channel(false);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _drop_signal = DropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("owned hook task starts");
    let control = Arc::new(HookListenerControl::new(
        policy,
        status,
        shutdown,
        vec![task],
    ));

    let shutdown_control = Arc::clone(&control);
    let shutdown_task = tokio::spawn(async move {
        shutdown_control
            .shutdown_with_timeout(Duration::from_millis(100))
            .await;
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !control.lock_tasks().tasks.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown batch takes the registered handle");

    shutdown_task.abort();
    let join_error = shutdown_task
        .await
        .expect_err("shutdown task is deterministically cancelled");
    assert!(join_error.is_cancelled());
    assert_eq!(control.lock_tasks().tasks.len(), 1);

    control.shutdown_with_timeout(Duration::ZERO).await;
    dropped_rx
        .await
        .expect("restored hook task is aborted and reaches terminal state");
    assert!(control.lock_tasks().tasks.is_empty());
}

/// 关闭批次建立后的 panic 必须经 Drop 恢复全部未终态句柄。
#[tokio::test]
async fn panic_after_taking_shutdown_batch_restores_hook_task_to_control() {
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&AiTool::ALL)));
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (shutdown, _receiver) = watch::channel(false);
    let task = tokio::spawn(std::future::pending::<()>());
    let control = HookListenerControl::new(policy, status, shutdown, vec![task]);

    let (batch, _) = control.begin_shutdown_batch(Duration::from_millis(100));
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _batch = batch;
        panic!("expected shutdown panic");
    }));

    assert!(unwind.is_err());
    assert_eq!(control.lock_tasks().tasks.len(), 1);
    control.shutdown_with_timeout(Duration::ZERO).await;
    assert!(control.lock_tasks().tasks.is_empty());
}

/// 未先调用 shutdown 就 Drop control 时，也不能丢弃尚未终态的 Tokio 句柄。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drop_transfers_non_yielding_task_to_idle_reaper() {
    let release = Arc::new(AtomicBool::new(false));
    let control = non_yielding_control(Arc::clone(&release)).await;
    let registration_id = control.lock_tasks().tasks[0].registration_id;

    drop(control);
    assert!(retained_hook_listener_owns_task(registration_id));
    release.store(true, Ordering::Release);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while retained_hook_listener_owns_task(registration_id)
        && tokio::time::Instant::now() < deadline
    {
        tokio::task::yield_now().await;
    }
    assert!(!retained_hook_listener_owns_task(registration_id));
}

/// 关闭后保存成功仍不能重新放开入队门禁。
#[tokio::test]
async fn closed_control_cannot_be_reenabled() {
    let policy=Arc::new(RwLock::new(HookListenerPolicy::new(&[AiTool::Codex])));
    let status=Arc::new(RwLock::new(HookRelayStatus::default()));
    let (shutdown,_)=watch::channel(false);let control=HookListenerControl::new(policy.clone(),status,shutdown,Vec::new());
    control.shutdown().await;control.replace_enabled_tools(&[AiTool::Codex]);
    assert_eq!(hook_listener_policy(&policy).admission(AiTool::Codex),None);
}

/// 第二个关闭请求必须等待同一真实批次，不能因为首批取走句柄就提前成功。
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn concurrent_shutdown_waits_for_owned_batch() {
    let policy=Arc::new(RwLock::new(HookListenerPolicy::new(&[])));let status=Arc::new(RwLock::new(HookRelayStatus::default()));let (shutdown,rx)=watch::channel(false);
    let (release_tx,release_rx)=tokio::sync::oneshot::channel();let task=tokio::spawn(async move{let _=release_rx.await;});
    let control=Arc::new(HookListenerControl::new(policy,status,shutdown,vec![task]));
    let first=control.clone();let first=tokio::spawn(async move{first.shutdown().await;});
    let mut rx=rx;rx.changed().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_millis(20),control.shutdown()).await.is_err());
    release_tx.send(()).unwrap();first.await.unwrap();control.shutdown().await;assert!(control.lock_tasks().tasks.is_empty());
}
