//! 拥有 GUI 生命周期内主动生成的扫描任务，并在真正退出时有界取消、等待和回收。
//! 本模块不决定扫描业务，只管理 JoinHandle、关闭信号和共享 writer 的宿主生命周期。

use std::{future::Future, sync::Mutex, time::Duration};

use loki_metis_core::ScanCancellation;
use tauri::async_runtime::JoinHandle;
use tokio::sync::watch;

use crate::backend::local_index::ScanCoordinator as LocalScanCoordinator;

/// 正常退出等待扫描到达安全点的共同宽限；超时后中止并等待 async 壳销毁。
const SCAN_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// 总时限尾部留给 async abort 后的终态确认。
const SCAN_TASK_FINAL_REAP_BUDGET: Duration = Duration::from_secs(1);

/// 交给长期任务的关闭观察端，使周期 sleep 可被退出事件立即唤醒。
pub(crate) struct ScanTaskShutdown {
    receiver: watch::Receiver<bool>,
}

impl ScanTaskShutdown {
    /// 返回 owner 是否已经开始关闭。
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// 等待 owner 开始关闭；发送端异常消失同样按关闭处理。
    pub(crate) async fn cancelled(&mut self) {
        loop {
            if *self.receiver.borrow_and_update() {
                return;
            }
            if self.receiver.changed().await.is_err() {
                return;
            }
        }
    }
}

/// 保存一项已登记任务的真实取消令牌、关闭唤醒端和可等待句柄。
struct OwnedScanTask {
    cancellation: ScanCancellation,
    shutdown: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

/// 在共同截止时间内等待任务终态；超时则把真实句柄交还 owner 继续处理。
async fn wait_for_owned_scan_task(
    mut task: OwnedScanTask,
    deadline: tokio::time::Instant,
) -> Option<OwnedScanTask> {
    match tokio::time::timeout_at(deadline, &mut task.handle).await {
        Ok(Ok(())) => None,
        Ok(Err(tauri::Error::JoinError(error))) if error.is_cancelled() => None,
        Ok(Err(error)) => {
            tracing::warn!(%error, "owned scan task stopped unexpectedly during shutdown");
            None
        }
        Err(_) => Some(task),
    }
}

/// 受同步锁保护的短生命周期登记表；锁内从不执行 await。
#[derive(Default)]
struct ScanTaskOwnerState {
    shutting_down: bool,
    tasks: Vec<OwnedScanTask>,
    /// 由 Tauri command 自身直接 await、没有独立 JoinHandle 的在途扫描数。
    direct_scans: usize,
    /// 第一次正常关闭确定的共同截止时间；后续退出事件不得重新获得完整时限。
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// 应用级扫描任务 owner；所有显式后台扫描和周期循环都必须在这里登记。
pub(crate) struct ScanTaskOwner {
    local_scan: LocalScanCoordinator,
    state: Mutex<ScanTaskOwnerState>,
    /// 发布 direct_scans 变化，让异步关闭不持有同步锁即可等待命令返回。
    direct_scan_changes: watch::Sender<usize>,
}

/// 把直接 await 的 Tauri 扫描命令登记到 owner；drop 即证明该命令已退出扫描路径。
pub(crate) struct DirectScanGuard<'owner> {
    owner: &'owner ScanTaskOwner,
}

impl ScanTaskOwner {
    /// 绑定共享 writer 协调器，确保关闭时也能取消由 Tauri command 直接 await 的扫描。
    pub(crate) fn new(local_scan: LocalScanCoordinator) -> Self {
        let (direct_scan_changes, _) = watch::channel(0);
        Self {
            local_scan,
            state: Mutex::new(ScanTaskOwnerState::default()),
            direct_scan_changes,
        }
    }

    /// 返回是否已进入不可逆的应用退出回收阶段。
    #[cfg(test)]
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.lock_state().shutting_down
    }

    /// 登记一项由当前 Tauri command 直接 await 的扫描；关闭开始后拒绝新登记。
    pub(crate) fn register_direct_scan(&self) -> Result<DirectScanGuard<'_>, &'static str> {
        let mut state = self.lock_state();
        if state.shutting_down {
            return Err("scan-task-owner-shutting-down");
        }
        state.direct_scans = state
            .direct_scans
            .checked_add(1)
            .ok_or("scan-task-owner-capacity-exceeded")?;
        self.direct_scan_changes.send_replace(state.direct_scans);
        Ok(DirectScanGuard { owner: self })
    }

    /// 启动并登记一项任务；关闭开始后拒绝新任务并取消调用方提供的令牌。
    pub(crate) fn spawn<Build, Task>(
        &self,
        cancellation: ScanCancellation,
        build: Build,
    ) -> Result<(), &'static str>
    where
        Build: FnOnce(ScanTaskShutdown) -> Task,
        Task: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.lock_state();
        state
            .tasks
            .retain(|task| !task.handle.inner().is_finished());
        if state.shutting_down {
            cancellation.cancel();
            return Err("scan-task-owner-shutting-down");
        }

        let (shutdown, receiver) = watch::channel(false);
        let handle = tauri::async_runtime::spawn(build(ScanTaskShutdown { receiver }));
        state.tasks.push(OwnedScanTask {
            cancellation,
            shutdown,
            handle,
        });
        Ok(())
    }

    /// 取消全部扫描并在共同截止时间内等待任务收敛，超时任务随后被中止并等待回收。
    pub(crate) async fn shutdown(&self) {
        self.shutdown_with_timeout(SCAN_TASK_SHUTDOWN_TIMEOUT).await;
    }

    /// 使用给定总时限完成关闭，供生产固定门限和快速回归测试复用。
    async fn shutdown_with_timeout(&self, timeout: Duration) {
        let (tasks, final_deadline) = self.begin_shutdown_with_deadline(timeout);
        let final_reap_budget = SCAN_TASK_FINAL_REAP_BUDGET.min(timeout / 2);
        let cooperative_deadline = final_deadline
            .checked_sub(final_reap_budget)
            .unwrap_or(final_deadline);

        // 并发收敛：顺序等待会让一个慢任务独占共同预算。
        let overdue = futures::future::join_all(
            tasks
                .into_iter()
                .map(|task| wait_for_owned_scan_task(task, cooperative_deadline)),
        )
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        for task in &overdue {
            task.handle.abort();
        }
        let unreaped = futures::future::join_all(
            overdue
                .into_iter()
                .map(|task| wait_for_owned_scan_task(task, final_deadline)),
        )
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if !unreaped.is_empty() {
            tracing::error!(
                count = unreaped.len(),
                "owned scan tasks did not terminate before shutdown deadline"
            );
            // 保留尚未终止的真实句柄，避免把同步 poll 误报为已经回收。
            self.lock_state().tasks.extend(unreaped);
        }
        self.wait_for_direct_scans(final_deadline).await;
    }

    /// 原子进入关闭并只在首次调用时创建 deadline，供连续退出事件共享总预算。
    fn begin_shutdown_with_deadline(
        &self,
        timeout: Duration,
    ) -> (Vec<OwnedScanTask>, tokio::time::Instant) {
        let now = tokio::time::Instant::now();
        let (tasks, deadline) = {
            let mut state = self.lock_state();
            state.shutting_down = true;
            let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
            (std::mem::take(&mut state.tasks), deadline)
        };
        self.cancel_for_shutdown(&tasks);
        (tasks, deadline)
    }

    /// 等待没有独立 JoinHandle 的命令返回；共享 deadline 保持退出总时限不变。
    async fn wait_for_direct_scans(&self, deadline: tokio::time::Instant) {
        let mut changes = self.direct_scan_changes.subscribe();
        loop {
            let active = self.lock_state().direct_scans;
            if active == 0 {
                return;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero()
                || tokio::time::timeout(remaining, changes.changed())
                    .await
                    .is_err()
            {
                tracing::warn!(active, "direct scan commands exceeded shutdown deadline");
                return;
            }
        }
    }

    /// 取得登记表锁；若测试 panic 污染锁则保留可关闭能力并继续回收。
    fn lock_state(&self) -> std::sync::MutexGuard<'_, ScanTaskOwnerState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 返回仍登记在 owner 下的任务数量。
    #[cfg(test)]
    fn owned_task_count(&self) -> usize {
        self.lock_state().tasks.len()
    }

    /// 返回由 command 直接 await、仍未离开扫描路径的任务数。
    #[cfg(test)]
    fn direct_scan_count(&self) -> usize {
        self.lock_state().direct_scans
    }

    /// 返回首次关闭保存的共同截止时间。
    #[cfg(test)]
    fn shutdown_deadline(&self) -> Option<tokio::time::Instant> {
        self.lock_state().shutdown_deadline
    }

    /// 关闭共享 writer，并向已摘除的后台任务广播两层取消信号。
    fn cancel_for_shutdown(&self, tasks: &[OwnedScanTask]) {
        self.local_scan.shutdown();
        for task in tasks {
            task.cancellation.cancel();
            let _ = task.shutdown.send(true);
        }
    }
}

impl Drop for DirectScanGuard<'_> {
    /// 在所有正常返回、错误和 panic 展开路径中解除命令登记并唤醒关闭等待者。
    fn drop(&mut self) {
        let mut state = self.owner.lock_state();
        debug_assert!(state.direct_scans > 0, "direct scan guard underflow");
        state.direct_scans = state.direct_scans.saturating_sub(1);
        self.owner
            .direct_scan_changes
            .send_replace(state.direct_scans);
    }
}

impl Drop for ScanTaskOwner {
    /// 非正常销毁无法异步等待，仍会关闭 writer、广播取消并中止所有登记任务。
    fn drop(&mut self) {
        for task in self.begin_shutdown_with_deadline(Duration::ZERO).0 {
            task.handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在异步壳真正销毁时通知测试，证明 owner 已等待或中止并回收 future。
    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        /// 发送一次性销毁信号；接收端提前退出时忽略错误。
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    /// 验证关闭会唤醒周期型任务、取消真实 worker token、等待 Drop 并保持幂等。
    #[tokio::test]
    async fn shutdown_cancels_waits_and_reaps_owned_task() {
        let coordinator = LocalScanCoordinator::default();
        let owner = ScanTaskOwner::new(coordinator.clone());
        let permit = coordinator.try_start().expect("writer starts");
        let cancellation = permit.cancellation_token();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();

        owner
            .spawn(cancellation.clone(), move |mut shutdown| async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                let _permit = permit;
                shutdown.cancelled().await;
            })
            .expect("task is registered");
        assert_eq!(owner.owned_task_count(), 1);

        owner.shutdown_with_timeout(Duration::from_secs(1)).await;

        assert!(cancellation.is_cancelled());
        dropped_rx.await.expect("owned future is dropped");
        assert_eq!(owner.owned_task_count(), 0);
        assert!(owner.is_shutting_down());
        assert!(coordinator.try_start().is_err());
        owner.shutdown_with_timeout(Duration::from_millis(1)).await;
    }

    /// 验证截止时间后 owner 会中止不响应协作取消的 async 壳并回收其资源。
    #[tokio::test]
    async fn shutdown_aborts_and_reaps_task_that_ignores_cancellation_after_deadline() {
        let coordinator = LocalScanCoordinator::default();
        let owner = ScanTaskOwner::new(coordinator);
        let cancellation = ScanCancellation::new();
        let (dropped_tx, mut dropped_rx) = tokio::sync::oneshot::channel();

        owner
            .spawn(cancellation.clone(), move |_shutdown| async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                std::future::pending::<()>().await;
            })
            .expect("task is registered");

        owner.shutdown_with_timeout(Duration::from_millis(10)).await;

        assert!(cancellation.is_cancelled());
        assert_eq!(
            dropped_rx.try_recv(),
            Ok(()),
            "shutdown must not return before the aborted future is reaped"
        );
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 验证关闭会等待由 Tauri command 直接 await 的扫描退出，并拒绝关闭后的新命令。
    #[tokio::test]
    async fn shutdown_waits_for_registered_direct_scan_command() {
        let coordinator = LocalScanCoordinator::default();
        let owner = std::sync::Arc::new(ScanTaskOwner::new(coordinator.clone()));
        let direct_scan = owner
            .register_direct_scan()
            .expect("direct scan command is registered");
        assert_eq!(owner.direct_scan_count(), 1);

        let shutdown_owner = std::sync::Arc::clone(&owner);
        let shutdown = tokio::spawn(async move {
            shutdown_owner
                .shutdown_with_timeout(Duration::from_secs(1))
                .await;
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !owner.is_shutting_down() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown reaches its registration gate");

        assert!(coordinator.try_start().is_err());
        assert!(!shutdown.is_finished());
        drop(direct_scan);
        tokio::time::timeout(Duration::from_secs(1), shutdown)
            .await
            .expect("shutdown observes direct scan completion")
            .expect("shutdown task joins");
        assert_eq!(owner.direct_scan_count(), 0);
        assert!(owner.register_direct_scan().is_err());
    }

    /// 验证命令预约与后台 spawn 交接之间发生 shutdown 时，关闭会等待预约且拒绝迟到任务。
    #[tokio::test]
    async fn direct_reservation_closes_shutdown_race_before_spawn_handoff() {
        let coordinator = LocalScanCoordinator::default();
        let owner = std::sync::Arc::new(ScanTaskOwner::new(coordinator));
        let reservation = owner
            .register_direct_scan()
            .expect("command reserves owner before preflight");
        let shutdown_owner = std::sync::Arc::clone(&owner);
        let shutdown = tokio::spawn(async move {
            shutdown_owner
                .shutdown_with_timeout(Duration::from_secs(1))
                .await;
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !owner.is_shutting_down() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown closes registration gate");

        let cancellation = ScanCancellation::new();
        assert!(
            owner
                .spawn(cancellation.clone(), |_shutdown| async {})
                .is_err()
        );
        assert!(cancellation.is_cancelled());
        assert!(!shutdown.is_finished());

        drop(reservation);
        shutdown.await.expect("shutdown joins after handoff ends");
    }

    /// 验证 ExitRequested 与 Exit 连续关闭复用首次 deadline，不会各自重获完整等待预算。
    #[tokio::test]
    async fn repeated_shutdown_reuses_first_deadline() {
        let coordinator = LocalScanCoordinator::default();
        let owner = ScanTaskOwner::new(coordinator);
        let reservation = owner
            .register_direct_scan()
            .expect("direct command is registered");

        owner.shutdown_with_timeout(Duration::from_millis(10)).await;
        let first_deadline = owner.shutdown_deadline().expect("deadline is persisted");
        owner.shutdown_with_timeout(Duration::from_secs(1)).await;

        assert_eq!(owner.shutdown_deadline(), Some(first_deadline));
        drop(reservation);
    }

    /// 不可让出的同步 poll 超过总时限时仍有界返回，并保留句柄直到任务真正终止。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_retains_uncooperative_task_without_exceeding_deadline() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let coordinator = LocalScanCoordinator::default();
        let owner = std::sync::Arc::new(ScanTaskOwner::new(coordinator));
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let task_release = std::sync::Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn(ScanCancellation::new(), move |_shutdown| async move {
                let _ = started_tx.send(());
                while !task_release.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
            })
            .expect("uncooperative task is registered");
        started_rx.await.expect("task entered synchronous poll");

        tokio::time::timeout(
            Duration::from_millis(250),
            owner.shutdown_with_timeout(Duration::from_millis(20)),
        )
        .await
        .expect("shutdown keeps its total deadline");
        assert_eq!(owner.owned_task_count(), 1);

        release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let finished = owner
                    .lock_state()
                    .tasks
                    .iter()
                    .all(|task| task.handle.inner().is_finished());
                if finished {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("retained task eventually reaches its terminal state");
        owner.shutdown_with_timeout(Duration::from_millis(1)).await;
        assert_eq!(owner.owned_task_count(), 0);
    }
}
