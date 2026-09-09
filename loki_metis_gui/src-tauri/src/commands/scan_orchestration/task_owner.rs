//! 拥有 GUI 生命周期内主动生成的扫描任务，并在真正退出时有界取消、等待和回收。
//! 本模块不决定扫描业务，只管理 JoinHandle、关闭信号和共享 writer 的宿主生命周期。

use std::{
    future::Future,
    pin::Pin,
    sync::{Mutex, OnceLock},
    task::{Context, Poll},
    time::Duration,
};

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

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
    #[cfg(test)]
    registration_id: u64,
}

/// 进程级保留 owner 异常销毁时尚未确认终态的扫描句柄。
static RETAINED_SCAN_TASKS: OnceLock<Mutex<Vec<OwnedScanTask>>> = OnceLock::new();

/// 为并行测试分配不冲突的扫描任务身份，避免用全局数量形成竞态断言。
#[cfg(test)]
static NEXT_SCAN_TASK_TEST_ID: AtomicU64 = AtomicU64::new(1);

/// 在共同截止时间内等待任务终态；超时则把真实句柄交还 owner 继续处理。
async fn wait_for_owned_scan_task(
    task: &mut OwnedScanTask,
    deadline: tokio::time::Instant,
) -> Option<tauri::Result<()>> {
    if task.handle.inner().is_finished() {
        Some((&mut task.handle).await)
    } else {
        tokio::time::timeout_at(deadline, &mut task.handle)
            .await
            .ok()
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

/// 在 shutdown future 被取消或 panic 时，把仍未终态的扫描句柄交还原 owner。
struct ScanTaskShutdownBatch<'owner> {
    owner: &'owner ScanTaskOwner,
    tasks: Vec<OwnedScanTask>,
}

impl<'owner> ScanTaskShutdownBatch<'owner> {
    /// 接管本轮关闭批次；所有 await 都只能借用该批次内的句柄。
    fn new(owner: &'owner ScanTaskOwner, tasks: Vec<OwnedScanTask>) -> Self {
        Self { owner, tasks }
    }

    /// 供 owner 的同步 Drop 路径取回批次并转交进程级 owner。
    fn into_tasks(mut self) -> Vec<OwnedScanTask> {
        std::mem::take(&mut self.tasks)
    }
}

impl Drop for ScanTaskShutdownBatch<'_> {
    /// 取消和栈展开均恢复所有权，不让局部 JoinHandle 随 future 一起 detach。
    fn drop(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        self.owner.lock_state().tasks.append(&mut self.tasks);
    }
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
        reap_process_scan_tasks();
        let mut state = self.lock_state();
        reap_finished_owned_scan_tasks(&mut state.tasks);
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
            #[cfg(test)]
            registration_id: NEXT_SCAN_TASK_TEST_ID.fetch_add(1, Ordering::Relaxed),
        });
        Ok(())
    }

    /// 取消全部扫描并在共同截止时间内等待任务收敛，超时任务随后被中止并等待回收。
    pub(crate) async fn shutdown(&self) {
        self.shutdown_with_timeout(SCAN_TASK_SHUTDOWN_TIMEOUT).await;
    }

    /// 使用给定总时限完成关闭，供生产固定门限和快速回归测试复用。
    async fn shutdown_with_timeout(&self, timeout: Duration) {
        reap_process_scan_tasks();
        let (mut batch, final_deadline) = self.begin_shutdown_with_deadline(timeout);
        let final_reap_budget = SCAN_TASK_FINAL_REAP_BUDGET.min(timeout / 2);
        let cooperative_deadline = final_deadline
            .checked_sub(final_reap_budget)
            .unwrap_or(final_deadline);

        // 扫描任务已经并发运行；逐句柄复用同一绝对截止时间，既不会累加总预算，
        // 也可让 batch 在每个 await 期间继续持有尚未轮到的全部句柄。
        reap_owned_scan_tasks_until(&mut batch.tasks, cooperative_deadline).await;
        for task in &batch.tasks {
            task.handle.abort();
        }
        reap_owned_scan_tasks_until(&mut batch.tasks, final_deadline).await;
        if !batch.tasks.is_empty() {
            tracing::error!(
                count = batch.tasks.len(),
                "owned scan tasks did not terminate before shutdown deadline"
            );
        }
        // 正常超时与取消、panic 使用同一恢复路径；此处显式恢复后再等待直接命令。
        drop(batch);
        self.wait_for_direct_scans(final_deadline).await;
    }

    /// 原子进入关闭并只在首次调用时创建 deadline，供连续退出事件共享总预算。
    fn begin_shutdown_with_deadline(
        &self,
        timeout: Duration,
    ) -> (ScanTaskShutdownBatch<'_>, tokio::time::Instant) {
        let now = tokio::time::Instant::now();
        let (tasks, deadline) = {
            let mut state = self.lock_state();
            state.shutting_down = true;
            let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
            (std::mem::take(&mut state.tasks), deadline)
        };
        // 先建立异常恢复守卫，再调用 writer 或任务取消路径。
        let batch = ScanTaskShutdownBatch::new(self, tasks);
        self.cancel_for_shutdown(&batch.tasks);
        (batch, deadline)
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

    /// 返回进程级 owner 是否仍持有指定测试任务。
    #[cfg(test)]
    fn process_owns_task(registration_id: u64) -> bool {
        reap_process_scan_tasks();
        RETAINED_SCAN_TASKS.get().is_some_and(|owner| {
            owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .any(|task| task.registration_id == registration_id)
        })
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
    /// 非正常销毁无法异步等待；abort 后的句柄转交进程级 owner，不能直接 detach。
    fn drop(&mut self) {
        let mut tasks = self
            .begin_shutdown_with_deadline(Duration::ZERO)
            .0
            .into_tasks();
        reap_finished_owned_scan_tasks(&mut tasks);
        for task in &tasks {
            task.handle.abort();
        }
        reap_finished_owned_scan_tasks(&mut tasks);
        retain_process_scan_tasks(tasks);
    }
}

/// 记录扫描任务终态；正常取消不报错，panic 与运行时异常保持可观察。
fn log_owned_scan_task_result(result: tauri::Result<()>) {
    match result {
        Ok(()) => {}
        Err(tauri::Error::JoinError(error)) if error.is_cancelled() => {}
        Err(error) => {
            tracing::warn!(%error, "owned scan task stopped unexpectedly during shutdown");
        }
    }
}

/// 在共享绝对截止时间前观察各项终态，并立即从批次移除已完成句柄。
async fn reap_owned_scan_tasks_until(
    tasks: &mut Vec<OwnedScanTask>,
    deadline: tokio::time::Instant,
) {
    let mut index = 0;
    while index < tasks.len() {
        if let Some(result) = wait_for_owned_scan_task(&mut tasks[index], deadline).await {
            let _task = tasks.swap_remove(index);
            log_owned_scan_task_result(result);
        } else {
            index += 1;
        }
    }
}

/// 无阻塞轮询已经报告完成的句柄，仅在观察其终态后从 owner 移除。
fn reap_finished_owned_scan_tasks(tasks: &mut Vec<OwnedScanTask>) {
    let waker = futures::task::noop_waker_ref();
    let mut context = Context::from_waker(waker);
    tasks.retain_mut(|task| {
        if !task.handle.inner().is_finished() {
            return true;
        }
        match Pin::new(&mut task.handle).poll(&mut context) {
            Poll::Ready(result) => {
                log_owned_scan_task_result(result);
                false
            }
            Poll::Pending => true,
        }
    });
}

/// 回收进程级 owner 中已经终态的扫描任务。
fn reap_process_scan_tasks() {
    let Some(owner) = RETAINED_SCAN_TASKS.get() else {
        return;
    };
    let mut tasks = owner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reap_finished_owned_scan_tasks(&mut tasks);
}

/// 保留无法在同步 Drop 中确认终态的真实句柄，直至后续观察或进程退出。
fn retain_process_scan_tasks(tasks: Vec<OwnedScanTask>) {
    if tasks.is_empty() {
        return;
    }
    let owner = RETAINED_SCAN_TASKS.get_or_init(|| Mutex::new(Vec::new()));
    let mut retained = owner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reap_finished_owned_scan_tasks(&mut retained);
    retained.extend(tasks);
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

    /// shutdown future 被真实取消时，局部批次必须把句柄交还 owner 并可再次回收。
    #[tokio::test]
    async fn cancelling_shutdown_future_restores_scan_task_to_owner() {
        let owner = std::sync::Arc::new(ScanTaskOwner::new(LocalScanCoordinator::default()));
        let cancellation = ScanCancellation::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn(cancellation.clone(), move |_shutdown| async move {
                let _drop_signal = DropSignal(Some(dropped_tx));
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            })
            .expect("scan task is registered");
        started_rx.await.expect("scan task starts");

        let shutdown_owner = std::sync::Arc::clone(&owner);
        let shutdown_task = tokio::spawn(async move {
            shutdown_owner
                .shutdown_with_timeout(Duration::from_millis(100))
                .await;
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while owner.owned_task_count() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown batch takes the scan handle");

        shutdown_task.abort();
        let join_error = shutdown_task
            .await
            .expect_err("shutdown task is deterministically cancelled");
        assert!(join_error.is_cancelled());
        assert_eq!(owner.owned_task_count(), 1);
        assert!(cancellation.is_cancelled());

        owner.shutdown_with_timeout(Duration::ZERO).await;
        dropped_rx
            .await
            .expect("restored scan task is aborted and reaches terminal state");
        owner.shutdown_with_timeout(Duration::ZERO).await;
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 关闭批次建立后的 panic 必须经 Drop 恢复全部未终态句柄。
    #[tokio::test]
    async fn panic_after_taking_shutdown_batch_restores_scan_task_to_owner() {
        let owner = ScanTaskOwner::new(LocalScanCoordinator::default());
        owner
            .spawn(ScanCancellation::new(), |_shutdown| async {
                std::future::pending::<()>().await;
            })
            .expect("scan task is registered");

        let (batch, _) = owner.begin_shutdown_with_deadline(Duration::from_millis(100));
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _batch = batch;
            panic!("expected shutdown panic");
        }));

        assert!(unwind.is_err());
        assert_eq!(owner.owned_task_count(), 1);
        tokio::time::timeout(Duration::from_secs(1), async {
            while owner.owned_task_count() != 0 {
                owner.shutdown_with_timeout(Duration::ZERO).await;
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("panic-restored scan task is eventually reaped");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// owner Drop 时 abort 未结束任务后，句柄必须转交进程级稳定 owner 再观察终态。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drop_transfers_unfinished_scan_task_to_process_owner() {
        use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

        let owner = ScanTaskOwner::new(LocalScanCoordinator::default());
        let release = std::sync::Arc::new(AtomicBool::new(false));
        let task_release = std::sync::Arc::clone(&release);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        owner
            .spawn(ScanCancellation::new(), move |_shutdown| async move {
                let _ = started_tx.send(());
                while !task_release.load(AtomicOrdering::Acquire) {
                    std::hint::spin_loop();
                }
            })
            .expect("non-yielding scan task is registered");
        let registration_id = owner.lock_state().tasks[0].registration_id;
        started_rx.await.expect("non-yielding scan task starts");

        drop(owner);

        assert!(ScanTaskOwner::process_owns_task(registration_id));
        release.store(true, AtomicOrdering::Release);
        tokio::time::timeout(Duration::from_secs(1), async {
            while ScanTaskOwner::process_owns_task(registration_id) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("process owner observes and reaps the terminal scan task");
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
