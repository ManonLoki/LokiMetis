//! Hook listener 的运行时启用策略与共享状态同步。

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard},
    task::{Context, Poll},
    thread,
    time::Duration,
};

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use loki_metis_core::{AiTool, HookTransition, normalize_enabled_ai_tools};
use tauri::async_runtime::JoinHandle;
use tokio::sync::watch;

use super::super::listener_state;
use super::super::thread_owner::RetainedThreadOwner;
use super::HookRelayStatus;

/// Hook listener 与事件 worker 共享的退出收敛上限。
const HOOK_LISTENER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// 总时限尾部保留给 abort 后的终态确认。
const HOOK_LISTENER_FINAL_REAP_BUDGET: Duration = Duration::from_secs(1);
/// 进程级 owner 轮询被 abort 但尚未到终态任务的间隔。
const HOOK_LISTENER_REAPER_POLL: Duration = Duration::from_millis(10);

/// 交给 listener/worker 的共享关闭观察端。
#[derive(Clone)]
pub(super) struct HookListenerShutdown {
    receiver: watch::Receiver<bool>,
}

impl HookListenerShutdown {
    /// 把共享 watch 接收端包装为 listener 关闭观察端。
    pub(super) fn new(receiver: watch::Receiver<bool>) -> Self {
        Self { receiver }
    }

    /// 返回是否已经收到关闭信号。
    pub(super) fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// 等待关闭信号；发送端消失同样按关闭处理。
    pub(super) async fn cancelled(&mut self) {
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

/// 一项由 control 或进程级 reaper 持有的 Hook listener 任务。
struct OwnedHookListenerTask {
    handle: JoinHandle<()>,
    #[cfg(test)]
    registration_id: u64,
}

impl OwnedHookListenerTask {
    /// 包装新登记句柄；测试构建同时分配进程内唯一身份。
    fn new(handle: JoinHandle<()>) -> Self {
        Self {
            handle,
            #[cfg(test)]
            registration_id: NEXT_HOOK_LISTENER_TASK_TEST_ID.fetch_add(1, Ordering::Relaxed),
        }
    }
}

/// Hook 两项长期任务的已登记句柄与首次关闭截止时间。
struct HookListenerTaskState {
    tasks: Vec<OwnedHookListenerTask>,
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// 从 control 移交后的异步任务及唯一 reaper 运行标志。
#[derive(Default)]
struct RetainedHookListenerState {
    tasks: Vec<OwnedHookListenerTask>,
    reaper_running: bool,
}

/// 在 control Drop 或关闭期限后继续持有并确认异步任务终态。
struct RetainedHookListenerOwner {
    shared: Arc<(Mutex<RetainedHookListenerState>, Condvar)>,
    reapers: RetainedThreadOwner,
}

impl RetainedHookListenerOwner {
    /// 创建空的 listener task owner。
    fn new() -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(RetainedHookListenerState::default()),
                Condvar::new(),
            )),
            reapers: RetainedThreadOwner::new("hook-listener"),
        }
    }
}

/// 所有 control 实例共享的最终异步任务 owner。
static RETAINED_HOOK_LISTENER_TASKS: OnceLock<RetainedHookListenerOwner> = OnceLock::new();

/// 为并行 reaper 回归分配唯一任务身份，避免用共享表全局数量断言。
#[cfg(test)]
static NEXT_HOOK_LISTENER_TASK_TEST_ID: AtomicU64 = AtomicU64::new(1);

/// 当前允许进入状态机的工具集合，以及每次启停变化后的单调代数。
#[derive(Debug)]
pub(super) struct HookListenerPolicy {
    pub(super) enabled_tools: HashSet<AiTool>,
    pub(super) generations: HashMap<AiTool, u64>,
}

impl HookListenerPolicy {
    /// 使用规范化后的启用工具集合和零代数创建监听策略。
    pub(super) fn new(enabled_tools: &[AiTool]) -> Self {
        Self {
            enabled_tools: normalize_enabled_ai_tools(enabled_tools)
                .into_iter()
                .collect(),
            generations: AiTool::ALL.into_iter().map(|tool| (tool, 0)).collect(),
        }
    }

    /// 当前工具启用时返回其代数，供事件入队时捕获。
    pub(super) fn admission(&self, tool: AiTool) -> Option<u64> {
        self.enabled_tools
            .contains(&tool)
            .then(|| self.generations.get(&tool).copied().unwrap_or_default())
    }

    /// 只有工具仍启用且代数未变化时，排队事件才可推进状态机。
    pub(super) fn admits_generation(&self, tool: AiTool, generation: u64) -> bool {
        self.admission(tool) == Some(generation)
    }
}

/// Tauri 保存设置时同步更新的 listener 启用门禁。
pub struct HookListenerControl {
    pub(super) policy: Arc<RwLock<HookListenerPolicy>>,
    pub(super) status: Arc<RwLock<HookRelayStatus>>,
    shutdown: watch::Sender<bool>,
    tasks: Mutex<HookListenerTaskState>,
}

/// 在 shutdown future 被取消或 panic 时把尚未终态的句柄交还原 control。
struct HookListenerShutdownBatch<'owner> {
    owner: &'owner HookListenerControl,
    tasks: Vec<OwnedHookListenerTask>,
}

impl<'owner> HookListenerShutdownBatch<'owner> {
    /// 接管本轮关闭批次，直到终态确认或显式转交进程级 owner。
    fn new(owner: &'owner HookListenerControl, tasks: Vec<OwnedHookListenerTask>) -> Self {
        Self { owner, tasks }
    }

    /// 正常超时时取出剩余句柄，避免 Drop 再登记到原 control。
    fn into_tasks(mut self) -> Vec<OwnedHookListenerTask> {
        std::mem::take(&mut self.tasks)
    }
}

impl Drop for HookListenerShutdownBatch<'_> {
    /// 任一 await 被取消或栈展开时恢复整批所有权，绝不因局部 Vec 销毁而 detach。
    fn drop(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        self.owner.lock_tasks().tasks.append(&mut self.tasks);
    }
}

impl HookListenerControl {
    /// 创建同时拥有 listener 与事件 worker 句柄的运行时控制器。
    pub(super) fn new(
        policy: Arc<RwLock<HookListenerPolicy>>,
        status: Arc<RwLock<HookRelayStatus>>,
        shutdown: watch::Sender<bool>,
        tasks: Vec<JoinHandle<()>>,
    ) -> Self {
        Self {
            policy,
            status,
            shutdown,
            tasks: Mutex::new(HookListenerTaskState {
                tasks: tasks.into_iter().map(OwnedHookListenerTask::new).collect(),
                shutdown_deadline: None,
            }),
        }
    }

    /// 替换启用集合；任何启停变化都会使该工具的旧排队事件和状态机失效。
    pub fn replace_enabled_tools(&self, enabled_tools: &[AiTool]) -> bool {
        let next = normalize_enabled_ai_tools(enabled_tools)
            .into_iter()
            .collect::<HashSet<_>>();
        let removed = {
            let mut policy = write_hook_listener_policy(&self.policy);
            let removed = policy
                .enabled_tools
                .difference(&next)
                .copied()
                .collect::<Vec<_>>();
            for tool in AiTool::ALL {
                if policy.enabled_tools.contains(&tool) != next.contains(&tool) {
                    let generation = policy.generations.entry(tool).or_default();
                    *generation = generation.saturating_add(1);
                }
            }
            policy.enabled_tools = next;
            removed
        };
        release_disabled_pet_states(&self.status, &removed)
    }

    /// 关闭入队门禁、通知两项长期任务，并在共同时限内回收。
    pub async fn shutdown(&self) {
        self.shutdown_with_timeout(HOOK_LISTENER_SHUTDOWN_TIMEOUT)
            .await;
    }

    /// 使用可注入时限执行关闭，供快速生命周期回归使用。
    async fn shutdown_with_timeout(&self, timeout: Duration) {
        self.replace_enabled_tools(&[]);
        let _ = self.shutdown.send(true);
        let (mut batch, final_deadline) = self.begin_shutdown_batch(timeout);
        let final_reap_budget = HOOK_LISTENER_FINAL_REAP_BUDGET.min(timeout / 2);
        let cooperative_deadline = final_deadline
            .checked_sub(final_reap_budget)
            .unwrap_or(final_deadline);

        // 两项任务本身已经并发运行；逐句柄共用绝对截止时间可让 batch 在每个
        // await 期间持续拥有其余句柄，同时不会把串行等待累加到总预算之外。
        reap_hook_listener_tasks_until(&mut batch.tasks, cooperative_deadline).await;
        for task in &batch.tasks {
            task.handle.abort();
        }
        reap_hook_listener_tasks_until(&mut batch.tasks, final_deadline).await;
        if !batch.tasks.is_empty() {
            tracing::error!(
                task_count = batch.tasks.len(),
                "owned hook listener tasks did not reach a terminal state before shutdown deadline"
            );
            retain_hook_listener_tasks(batch.into_tasks());
        }
    }

    /// 原子摘除当前批次并建立恢复守卫；首次关闭保存唯一共同截止时间。
    fn begin_shutdown_batch(
        &self,
        timeout: Duration,
    ) -> (HookListenerShutdownBatch<'_>, tokio::time::Instant) {
        let now = tokio::time::Instant::now();
        let (tasks, deadline) = {
            let mut state = self.lock_tasks();
            let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
            (std::mem::take(&mut state.tasks), deadline)
        };
        (HookListenerShutdownBatch::new(self, tasks), deadline)
    }

    /// 取得句柄表锁；测试 panic 污染后仍保留回收能力。
    fn lock_tasks(&self) -> std::sync::MutexGuard<'_, HookListenerTaskState> {
        self.tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 等待一项 listener 任务到共同截止时间；超时保持原句柄仍在批次内。
async fn wait_for_hook_listener_task(
    task: &mut OwnedHookListenerTask,
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

/// 记录 listener 任务终态；取消属于预期回收，panic 与其他错误保持可观察。
fn log_hook_listener_task_result(result: tauri::Result<()>) {
    match result {
        Ok(()) => {}
        Err(tauri::Error::JoinError(error)) if error.is_cancelled() => {}
        Err(error) => {
            tracing::warn!(%error, "owned hook listener task stopped unexpectedly");
        }
    }
}

/// 在共享截止时间前观察各句柄终态，立即移除已完成项并保留其余所有权。
async fn reap_hook_listener_tasks_until(
    tasks: &mut Vec<OwnedHookListenerTask>,
    deadline: tokio::time::Instant,
) {
    let mut index = 0;
    while index < tasks.len() {
        if let Some(result) = wait_for_hook_listener_task(&mut tasks[index], deadline).await {
            let _task = tasks.swap_remove(index);
            log_hook_listener_task_result(result);
        } else {
            index += 1;
        }
    }
}

impl Drop for HookListenerControl {
    /// 异常销毁时关闭入队、中止任务，并把未确认终态的句柄移交进程级 owner。
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        let tasks = self.lock_tasks().tasks.drain(..).collect::<Vec<_>>();
        for task in &tasks {
            task.handle.abort();
        }
        retain_hook_listener_tasks(tasks);
    }
}

/// 返回所有 listener control 实例共享的最终任务 owner。
fn retained_hook_listener_owner() -> &'static RetainedHookListenerOwner {
    RETAINED_HOOK_LISTENER_TASKS.get_or_init(RetainedHookListenerOwner::new)
}

/// 把超时任务移交静态表，并启动一个不依赖 Tokio 调度让出的终态 reaper。
fn retain_hook_listener_tasks(tasks: Vec<OwnedHookListenerTask>) {
    if tasks.is_empty() {
        return;
    }
    let owner = retained_hook_listener_owner();
    let should_start = {
        let (state, wake) = &*owner.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.tasks.extend(tasks);
        let should_start = !state.reaper_running;
        if should_start {
            state.reaper_running = true;
        }
        wake.notify_all();
        should_start
    };
    if should_start {
        start_hook_listener_reaper(owner);
    }
}

/// 启动 std 线程轮询 Tokio 句柄，并由通用线程 owner 持有 reaper 自身。
fn start_hook_listener_reaper(owner: &'static RetainedHookListenerOwner) {
    let shared = Arc::clone(&owner.shared);
    match thread::Builder::new()
        .name("loki-metis-hook-listener-reaper".to_owned())
        .spawn(move || hook_listener_reaper_loop(&shared))
    {
        Ok(reaper) => owner.reapers.retain(reaper),
        Err(error) => {
            owner
                .shared
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .reaper_running = false;
            tracing::error!(%error, "failed to start hook listener task reaper");
        }
    }
}

/// 仅在 Tokio 句柄报告完成后 poll 终态，直到移交表为空。
fn hook_listener_reaper_loop(shared: &Arc<(Mutex<RetainedHookListenerState>, Condvar)>) {
    loop {
        let (state, wake) = &**shared;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let waker = futures::task::noop_waker_ref();
        let mut context = Context::from_waker(waker);
        state.tasks.retain_mut(|task| {
            if !task.handle.inner().is_finished() {
                return true;
            }
            match Pin::new(&mut task.handle).poll(&mut context) {
                Poll::Ready(Ok(())) => false,
                Poll::Ready(Err(error)) => {
                    if !matches!(&error, tauri::Error::JoinError(join) if join.is_cancelled()) {
                        tracing::warn!(%error, "retained hook listener task stopped unexpectedly");
                    }
                    false
                }
                Poll::Pending => true,
            }
        });
        if state.tasks.is_empty() {
            state.reaper_running = false;
            wake.notify_all();
            return;
        }
        drop(wake.wait_timeout(state, HOOK_LISTENER_REAPER_POLL));
    }
}

/// 返回进程级 owner 是否仍持有指定测试任务。
#[cfg(test)]
fn retained_hook_listener_owns_task(registration_id: u64) -> bool {
    retained_hook_listener_owner()
        .shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tasks
        .iter()
        .any(|task| task.registration_id == registration_id)
}

/// 清空被禁用工具当前占用的槽位；没有活跃槽位时不制造虚假 revision。
fn release_disabled_pet_states(
    status: &Arc<RwLock<HookRelayStatus>>,
    disabled_tools: &[AiTool],
) -> bool {
    if disabled_tools.is_empty() {
        return false;
    }
    let mut current = write_hook_relay_status(status);
    let mut changed = false;
    for &tool in disabled_tools {
        if current
            .pet_states
            .iter()
            .any(|state| state.tool == tool && state.behavior.is_some())
        {
            let mut revision = current.revision;
            listener_state::apply_pet_transition(
                &mut current.pet_states,
                &mut revision,
                tool,
                0,
                HookTransition::Release,
            );
            current.revision = revision;
            changed = true;
        }
    }
    if current
        .last_event
        .as_ref()
        .is_some_and(|event| disabled_tools.contains(&event.tool))
    {
        current.last_event = None;
    }
    if changed {
        current.last_error = None;
    }
    changed
}

/// 读取启用策略；锁污染时保留现状并记录诊断，避免静默放宽门禁。
pub(super) fn hook_listener_policy(
    policy: &RwLock<HookListenerPolicy>,
) -> RwLockReadGuard<'_, HookListenerPolicy> {
    policy.read().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook listener policy lock"
        );
        poisoned.into_inner()
    })
}

/// 写入启用策略；锁污染时仍以调用方的新设置恢复。
fn write_hook_listener_policy(
    policy: &RwLock<HookListenerPolicy>,
) -> RwLockWriteGuard<'_, HookListenerPolicy> {
    policy.write().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook listener policy lock for update"
        );
        poisoned.into_inner()
    })
}

/// 锁曾被 panic 污染时记录错误并恢复内部状态，避免后续所有 Hook 被静默丢弃。
pub(super) fn write_hook_relay_status(
    status: &RwLock<HookRelayStatus>,
) -> RwLockWriteGuard<'_, HookRelayStatus> {
    status.write().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook relay status lock"
        );
        poisoned.into_inner()
    })
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
