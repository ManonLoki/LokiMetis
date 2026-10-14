//! 拥有非扫描型应用后台任务，并在退出时统一取消、等待和回收。

mod task_capacity;

use std::{
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    sync::Arc,
    sync::Mutex,
    sync::OnceLock,
    task::{Context, Poll},
    time::Duration,
};

use loki_metris_core::RootDiscoveryCoordinator;
use tokio::task::JoinHandle;
use tokio::sync::watch;

#[cfg(test)]
use self::task_capacity::{BACKGROUND_TASK_CAPACITY, BACKGROUND_TASK_CAPACITY_ERROR};
use self::task_capacity::{
    BackgroundTaskCapacity, BackgroundTaskSlot, process_background_task_capacity,
};

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
    /// 应用明确注入的 runtime，后台任务不借用全局运行时。
    runtime: tokio::runtime::Handle,
    root_discovery: Arc<RootDiscoveryCoordinator>,
    capacity: Arc<BackgroundTaskCapacity>,
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

    /// 在 GPUI 创建阶段注入专属运行时句柄。
    pub(crate) fn with_runtime(root_discovery: Arc<RootDiscoveryCoordinator>,runtime:tokio::runtime::Handle) -> Self {
        Self {
            runtime,
            root_discovery,
            capacity: process_background_task_capacity(),
            state: Mutex::new(BackgroundTaskOwnerState::default()),
        }
    }

    /// 使用指定隔离容量构造测试 owner，避免并行用例争用生产计数器。
    #[cfg(test)]
    fn new_with_capacity(
        root_discovery: Arc<RootDiscoveryCoordinator>,
        capacity: Arc<BackgroundTaskCapacity>,
    ) -> Self {
        Self {
            root_discovery,
            capacity,
            runtime:tokio::runtime::Handle::current(),
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
        self.spawn_handle(
            name,
            BackgroundTaskKind::AbortableAsync,
            None,
            |shutdown, slot| {
                let task = build(shutdown);
                self.runtime.spawn(async move {
                    let _slot = slot;
                    task.await;
                })
            },
        )
    }

    /// 启动并登记一项阻塞岛任务；业务循环应同时观察关闭端。
    #[cfg_attr(target_os = "macos", allow(dead_code))]
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
            |shutdown, slot| {
                self.runtime.spawn_blocking(move || {
                    let _slot = slot;
                    build(shutdown);
                })
            },
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
            |shutdown, slot| {
                self.runtime.spawn_blocking(move || {
                    let _slot = slot;
                    build(shutdown);
                })
            },
        )
    }

    /// 先在锁内预约，锁外启动任务，再按关闭竞态决定登记或立即取消并转交。
    fn spawn_handle(
        &self,
        name: &'static str,
        kind: BackgroundTaskKind,
        cooperative_cancel: Option<Box<dyn Fn() + Send + Sync + 'static>>,
        spawn: impl FnOnce(BackgroundTaskShutdown, BackgroundTaskSlot) -> JoinHandle<()>,
    ) -> Result<(), &'static str> {
        reap_process_background_tasks();
        let slot = {
            let mut state = self.lock_state();
            reap_finished_owned_tasks(&mut state.tasks);
            if state.shutting_down {
                return Err("background-task-owner-shutting-down");
            }
            BackgroundTaskSlot::reserve(&self.capacity)?
        };
        let (shutdown, receiver) = watch::channel(false);
        let handle = spawn(BackgroundTaskShutdown { receiver }, slot);
        let mut pending = Some(OwnedBackgroundTask {
            name,
            shutdown,
            cooperative_cancel,
            handle,
            kind,
        });
        {
            let mut state = self.lock_state();
            if !state.shutting_down {
                state
                    .tasks
                    .push(pending.take().expect("pending task exists before adoption"));
            }
        }
        let Some(task) = pending else {
            return Ok(());
        };

        request_owned_task_cancellation(&task);
        if task.kind == BackgroundTaskKind::AbortableAsync {
            task.handle.abort();
        }
        let mut tasks = vec![task];
        reap_finished_owned_tasks(&mut tasks);
        retain_process_background_tasks(tasks);
        Err("background-task-owner-shutting-down")
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
            reap_finished_owned_tasks(&mut state.tasks);
            let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
            (std::mem::take(&mut state.tasks), deadline)
        };
        // 必须先建立异常恢复守卫，再调用任何可能 panic 的业务取消钩子。
        let batch = ShutdownTaskBatchGuard::new(self, tasks);
        let _ = self.root_discovery.request_cancel();
        for task in &batch.tasks {
            request_owned_task_cancellation(task);
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

/// 逐任务隔离合作取消钩子的 panic，并始终继续发布通用关闭信号。
fn request_owned_task_cancellation(task: &OwnedBackgroundTask) {
    if let Some(cancel) = &task.cooperative_cancel
        && catch_unwind(AssertUnwindSafe(cancel)).is_err()
    {
        tracing::error!(
            task = task.name,
            "owned background task cancel hook panicked"
        );
    }
    let _ = task.shutdown.send(true);
}

impl Drop for BackgroundTaskOwner {
    /// 异常销毁无法异步等待；未确认终态的句柄转交进程级 owner，绝不直接 detach。
    fn drop(&mut self) {
        let mut tasks = self.begin_shutdown(Duration::ZERO).0.into_tasks();
        reap_finished_owned_tasks(&mut tasks);
        for task in &tasks {
            if task.kind == BackgroundTaskKind::AbortableAsync {
                task.handle.abort();
            } else if !task.handle.is_finished() {
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
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    tasks.retain_mut(|task| {
        if !task.handle.is_finished() {
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
fn log_owned_task_join_result(name: &'static str, result: Result<(),tokio::task::JoinError>) -> bool {
    match result {
        Ok(()) => false,
        Err(error) if error.is_cancelled() => false,
        Err(error) if error.is_panic() => {
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
) -> Option<Result<(),tokio::task::JoinError>> {

    if task.handle.is_finished() {
        Some((&mut task.handle).await)
    } else {
        tokio::time::timeout_at(deadline, &mut task.handle)
            .await
            .ok()
    }
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
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "task_owner_capacity_tests.rs"]
mod capacity_tests;
