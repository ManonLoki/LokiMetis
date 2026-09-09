//! 拥有非扫描型应用后台任务，并在退出时统一取消、等待和回收。

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    sync::Mutex,
    sync::OnceLock,
    task::{Context, Poll},
    time::Duration,
};

use loki_metis_core::RootDiscoveryCoordinator;
use tauri::async_runtime::JoinHandle;
use tokio::sync::watch;

/// 正常退出等待后台任务合作收敛的共同时限。
const BACKGROUND_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// 总时限尾部保留给 async abort 后的终态确认，以及 blocking 任务最后一次合作回收。
const BACKGROUND_TASK_FINAL_REAP_BUDGET: Duration = Duration::from_secs(1);

/// 交给应用后台任务的关闭观察端。
pub(crate) struct BackgroundTaskShutdown {
    receiver: watch::Receiver<bool>,
}

impl BackgroundTaskShutdown {
    /// 返回 owner 是否已进入不可逆的关闭阶段。
    pub(crate) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// 等待 owner 发起关闭；发送端消失同样按关闭处理。
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

/// 一项已登记的应用后台任务。
struct OwnedBackgroundTask {
    name: &'static str,
    shutdown: watch::Sender<bool>,
    cooperative_cancel: Option<Box<dyn Fn() + Send + Sync + 'static>>,
    handle: JoinHandle<()>,
    kind: BackgroundTaskKind,
}

/// 区分可由 Tokio abort 销毁的异步 future 与已经开始后不可强停的 blocking 闭包。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackgroundTaskKind {
    AbortableAsync,
    CooperativeBlocking,
}

/// 进程级保留异常销毁时仍未终态的任务，防止丢弃句柄形成 detached 执行。
static RETAINED_BACKGROUND_TASKS: OnceLock<Mutex<Vec<OwnedBackgroundTask>>> = OnceLock::new();

/// 受同步锁保护的短临界区登记表；锁内不执行 await。
#[derive(Default)]
struct BackgroundTaskOwnerState {
    shutting_down: bool,
    tasks: Vec<OwnedBackgroundTask>,
    /// 首次关闭确定的共同截止时间；重复退出事件不得重置预算。
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// GUI 生命周期内 root discovery 与一次性维护任务的唯一 owner。
pub(crate) struct BackgroundTaskOwner {
    root_discovery: Arc<RootDiscoveryCoordinator>,
    state: Mutex<BackgroundTaskOwnerState>,
}

/// 在关闭 future 被取消或 panic 时，把仍未确认终态的句柄交还原 owner。
struct ShutdownTaskBatchGuard<'owner> {
    owner: &'owner BackgroundTaskOwner,
    tasks: Vec<OwnedBackgroundTask>,
}

impl<'owner> ShutdownTaskBatchGuard<'owner> {
    /// 接管本轮关闭批次；正常或异常离开作用域都由 Drop 恢复剩余所有权。
    fn new(owner: &'owner BackgroundTaskOwner, tasks: Vec<OwnedBackgroundTask>) -> Self {
        Self { owner, tasks }
    }

    /// 由同步 Drop 路径取回批次，清空守卫以避免重新登记到即将销毁的 owner。
    fn into_tasks(mut self) -> Vec<OwnedBackgroundTask> {
        std::mem::take(&mut self.tasks)
    }
}

impl Drop for ShutdownTaskBatchGuard<'_> {
    /// await 取消与 panic 都会经过这里，不能让局部 JoinHandle 因 Vec 销毁而 detach。
    fn drop(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        self.owner.lock_state().tasks.append(&mut self.tasks);
    }
}

impl BackgroundTaskOwner {
    /// 绑定发现协调器，使退出可先发出业务取消再等待任务。
    pub(crate) fn new(root_discovery: Arc<RootDiscoveryCoordinator>) -> Self {
        Self {
            root_discovery,
            state: Mutex::new(BackgroundTaskOwnerState::default()),
        }
    }

    /// 启动并登记一项异步任务；关闭开始后拒绝新任务。
    pub(crate) fn spawn<Build, Task>(
        &self,
        name: &'static str,
        build: Build,
    ) -> Result<(), &'static str>
    where
        Build: FnOnce(BackgroundTaskShutdown) -> Task,
        Task: Future<Output = ()> + Send + 'static,
    {
        self.spawn_handle(name, BackgroundTaskKind::AbortableAsync, None, |shutdown| {
            tauri::async_runtime::spawn(build(shutdown))
        })
    }

    /// 启动并登记一项阻塞岛任务；业务循环应同时观察关闭端。
    pub(crate) fn spawn_blocking<Build>(
        &self,
        name: &'static str,
        build: Build,
    ) -> Result<(), &'static str>
    where
        Build: FnOnce(BackgroundTaskShutdown) + Send + 'static,
    {
        self.spawn_handle(
            name,
            BackgroundTaskKind::CooperativeBlocking,
            None,
            |shutdown| tauri::async_runtime::spawn_blocking(move || build(shutdown)),
        )
    }

    /// 启动带显式合作取消钩子的阻塞任务；关闭开始时先触发钩子再发布通用信号。
    pub(crate) fn spawn_blocking_cancelable<Cancel, Build>(
        &self,
        name: &'static str,
        cancel: Cancel,
        build: Build,
    ) -> Result<(), &'static str>
    where
        Cancel: Fn() + Send + Sync + 'static,
        Build: FnOnce(BackgroundTaskShutdown) + Send + 'static,
    {
        self.spawn_handle(
            name,
            BackgroundTaskKind::CooperativeBlocking,
            Some(Box::new(cancel)),
            |shutdown| tauri::async_runtime::spawn_blocking(move || build(shutdown)),
        )
    }

    /// 在单一同步临界区内完成关闭检查、启动与登记。
    fn spawn_handle(
        &self,
        name: &'static str,
        kind: BackgroundTaskKind,
        cooperative_cancel: Option<Box<dyn Fn() + Send + Sync + 'static>>,
        spawn: impl FnOnce(BackgroundTaskShutdown) -> JoinHandle<()>,
    ) -> Result<(), &'static str> {
        reap_process_background_tasks();
        let mut state = self.lock_state();
        reap_finished_owned_tasks(&mut state.tasks);
        if state.shutting_down {
            return Err("background-task-owner-shutting-down");
        }
        let (shutdown, receiver) = watch::channel(false);
        let handle = spawn(BackgroundTaskShutdown { receiver });
        state.tasks.push(OwnedBackgroundTask {
            name,
            shutdown,
            cooperative_cancel,
            handle,
            kind,
        });
        Ok(())
    }

    /// 请求 root discovery 及所有任务取消，并在共同时限内并发回收。
    pub(crate) async fn shutdown(&self) {
        self.shutdown_with_timeout(BACKGROUND_TASK_SHUTDOWN_TIMEOUT)
            .await;
    }

    /// 使用可注入时限执行关闭，供生产门限与快速回归复用。
    async fn shutdown_with_timeout(&self, timeout: Duration) {
        let (mut batch, final_deadline) = self.begin_shutdown(timeout);
        let final_reap_budget = BACKGROUND_TASK_FINAL_REAP_BUDGET.min(timeout / 2);
        let cooperative_deadline = final_deadline
            .checked_sub(final_reap_budget)
            .unwrap_or(final_deadline);

        // 所有任务本身已经并发运行；逐句柄使用同一绝对截止时间等待，既不让慢任务
        // 延长总预算，也让 batch 在每个 await 期间持续拥有尚未终态的句柄。
        reap_owned_tasks_until(&mut batch.tasks, cooperative_deadline).await;

        // abort 只对尚未进入同步阻塞区的 async future 有终止语义；对 spawn_blocking
        // 调用 abort 会制造已经回收的假象，因此 blocking 任务只继续观察合作取消。
        for task in &batch.tasks {
            if task.kind == BackgroundTaskKind::AbortableAsync {
                task.handle.abort();
            }
        }

        reap_owned_tasks_until(&mut batch.tasks, final_deadline).await;
        if !batch.tasks.is_empty() {
            for task in &batch.tasks {
                tracing::error!(
                    task = task.name,
                    kind = ?task.kind,
                    "owned background task did not reach a terminal state before shutdown deadline"
                );
            }
        }
        // batch 的 Drop 在正常返回时也把超时任务交还 owner；取消或 panic 走同一路径。
    }

    /// 原子进入关闭，先用异常安全守卫接管句柄，再发布业务取消与通用关闭信号。
    fn begin_shutdown(
        &self,
        timeout: Duration,
    ) -> (ShutdownTaskBatchGuard<'_>, tokio::time::Instant) {
        let now = tokio::time::Instant::now();
        let (tasks, deadline) = {
            let mut state = self.lock_state();
            state.shutting_down = true;
            let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
            (std::mem::take(&mut state.tasks), deadline)
        };
        // 必须先建立异常恢复守卫，再调用任何可能 panic 的业务取消钩子。
        let batch = ShutdownTaskBatchGuard::new(self, tasks);
        let _ = self.root_discovery.request_cancel();
        for task in &batch.tasks {
            if let Some(cancel) = &task.cooperative_cancel {
                cancel();
            }
            let _ = task.shutdown.send(true);
        }
        (batch, deadline)
    }

    /// 取得登记表锁；测试 panic 污染后仍保留回收能力。
    fn lock_state(&self) -> std::sync::MutexGuard<'_, BackgroundTaskOwnerState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 返回当前登记任务数。
    #[cfg(test)]
    pub(crate) fn owned_task_count(&self) -> usize {
        self.lock_state().tasks.len()
    }

    /// 返回进程级 owner 是否仍持有指定测试任务。
    #[cfg(test)]
    fn process_owns_task_named(name: &str) -> bool {
        reap_process_background_tasks();
        RETAINED_BACKGROUND_TASKS.get().is_some_and(|owner| {
            owner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .any(|task| task.name == name)
        })
    }
}

impl Drop for BackgroundTaskOwner {
    /// 异常销毁无法异步等待；未确认终态的句柄转交进程级 owner，绝不直接 detach。
    fn drop(&mut self) {
        let mut tasks = self.begin_shutdown(Duration::ZERO).0.into_tasks();
        reap_finished_owned_tasks(&mut tasks);
        for task in &tasks {
            if task.kind == BackgroundTaskKind::AbortableAsync {
                task.handle.abort();
            } else if !task.handle.inner().is_finished() {
                tracing::error!(
                    task = task.name,
                    "cooperative blocking task transferred to process owner during teardown"
                );
            }
        }
        reap_finished_owned_tasks(&mut tasks);
        retain_process_background_tasks(tasks);
    }
}

/// 回收进程级 owner 中已经到达终态的任务；未结束句柄继续保留。
fn reap_process_background_tasks() {
    let Some(owner) = RETAINED_BACKGROUND_TASKS.get() else {
        return;
    };
    let mut tasks = owner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reap_finished_owned_tasks(&mut tasks);
}

/// 把无法同步确认终态的任务转交静态 owner，进程存活期间不销毁其 JoinHandle。
fn retain_process_background_tasks(tasks: Vec<OwnedBackgroundTask>) {
    if tasks.is_empty() {
        return;
    }
    let owner = RETAINED_BACKGROUND_TASKS.get_or_init(|| Mutex::new(Vec::new()));
    let mut retained = owner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reap_finished_owned_tasks(&mut retained);
    retained.extend(tasks);
}

/// 无阻塞轮询已经报告终态的句柄，记录 panic/异常后才从 owner 表移除。
fn reap_finished_owned_tasks(tasks: &mut Vec<OwnedBackgroundTask>) {
    let waker = futures::task::noop_waker_ref();
    let mut context = Context::from_waker(waker);
    tasks.retain_mut(|task| {
        if !task.handle.inner().is_finished() {
            return true;
        }
        match Pin::new(&mut task.handle).poll(&mut context) {
            Poll::Ready(result) => {
                log_owned_task_join_result(task.name, result);
                false
            }
            Poll::Pending => true,
        }
    });
}

/// 记录后台任务终态；返回是否属于需要观测的 panic 或运行时异常。
fn log_owned_task_join_result(name: &'static str, result: tauri::Result<()>) -> bool {
    match result {
        Ok(()) => false,
        Err(tauri::Error::JoinError(error)) if error.is_cancelled() => false,
        Err(tauri::Error::JoinError(error)) if error.is_panic() => {
            tracing::error!(task = name, %error, "owned background task panicked");
            true
        }
        Err(error) => {
            tracing::warn!(task = name, %error, "owned background task stopped unexpectedly");
            true
        }
    }
}

/// 等待一项任务到共同截止时间；返回终态结果，超时则保持原句柄不动。
async fn wait_for_owned_task(
    task: &mut OwnedBackgroundTask,
    deadline: tokio::time::Instant,
) -> Option<tauri::Result<()>> {
    let result = if task.handle.inner().is_finished() {
        Some((&mut task.handle).await)
    } else {
        tokio::time::timeout_at(deadline, &mut task.handle)
            .await
            .ok()
    };
    result
}

/// 在共享截止时间前逐一观察终态，并立即移除已完成句柄，保证 guard 可安全恢复余项。
async fn reap_owned_tasks_until(
    tasks: &mut Vec<OwnedBackgroundTask>,
    deadline: tokio::time::Instant,
) {
    let mut index = 0;
    while index < tasks.len() {
        if let Some(result) = wait_for_owned_task(&mut tasks[index], deadline).await {
            let task = tasks.swap_remove(index);
            log_owned_task_join_result(task.name, result);
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

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
        use loki_metis_core::ScanCancellation;

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
        let result = tauri::async_runtime::spawn(async {
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
            while !owner.lock_state().tasks[0].handle.inner().is_finished() {
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
}
