//! 通知 worker 与 macOS 设置 opener 的关闭所有权；不承担通知业务或宿主投递。

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex, OnceLock},
    task::{Context, Poll},
    thread,
    time::Duration,
};

use tauri::async_runtime::JoinHandle;

/// 进程级 owner 检查被移交 Tokio 任务终态的轮询间隔。
const RETAINED_NOTIFICATION_TASK_POLL: Duration = Duration::from_millis(10);
#[cfg(any(target_os = "macos", test))]
/// 进程级 owner 检查被移交 opener 子进程终态的轮询间隔。
const RETAINED_NOTIFICATION_OPENER_POLL: Duration = Duration::from_millis(10);
#[cfg(any(target_os = "macos", test))]
/// 限制在途与遗留 opener 总数，避免不可回收进程无界累积。
pub(super) const RETAINED_NOTIFICATION_OPENER_CAPACITY: usize = 4;
#[cfg(any(target_os = "macos", test))]
/// opener owner 容量耗尽时返回的稳定错误。
pub(super) const NOTIFICATION_OPENER_CAPACITY_ERROR: &str =
    "notification-settings-opener-capacity-exceeded";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// 保存首次关闭确定的合作等待与最终回收截止时间，重复调用不得重置预算。
pub(super) struct NotificationShutdownSchedule {
    pub(super) cooperative_deadline: tokio::time::Instant,
    pub(super) final_deadline: tokio::time::Instant,
}

/// 稳定持有通知 worker 句柄与首次关闭时限；同步锁内不得执行 await。
struct NotificationWorkerTaskState {
    tasks: Vec<JoinHandle<()>>,
    shutdown_schedule: Option<NotificationShutdownSchedule>,
}

/// shutdown 各阶段共享的 worker 任务 owner。
pub(super) struct NotificationWorkerTaskOwner {
    state: Mutex<NotificationWorkerTaskState>,
}

/// shutdown future 取消或 panic 时把所有未终态句柄归还稳定 owner。
pub(super) struct NotificationShutdownTaskBatchGuard<'owner> {
    owner: &'owner NotificationWorkerTaskOwner,
    pub(super) tasks: Vec<JoinHandle<()>>,
}

#[derive(Default)]
/// 保存异常销毁后仍未终态的通知任务及唯一 reaper 运行标志。
struct RetainedNotificationTaskState {
    tasks: Vec<JoinHandle<()>>,
    reaper_running: bool,
}

/// 进程级持续持有异常销毁任务与 reaper 线程自身的稳定 owner。
struct RetainedNotificationTaskOwner {
    shared: Arc<(Mutex<RetainedNotificationTaskState>, Condvar)>,
    reapers: Mutex<Vec<thread::JoinHandle<()>>>,
}

/// 所有 NotificationWorker 实例共享的最终任务 owner。
static RETAINED_NOTIFICATION_TASKS: OnceLock<RetainedNotificationTaskOwner> = OnceLock::new();

#[cfg(any(target_os = "macos", test))]
/// 为真实子进程与确定性测试夹具提供最小同步回收边界。
pub(super) trait RetainableNotificationOpenerChild: Send {
    /// 请求子进程终止；调用不等待进程终态。
    fn start_kill_owned(&mut self) -> std::io::Result<()>;

    /// 非阻塞判断子进程是否已经被系统回收。
    fn try_wait_terminal(&mut self) -> std::io::Result<bool>;

    /// 返回该 owner 生命周期内稳定的子进程标识。
    fn owner_id(&self) -> u64;
}

#[cfg(target_os = "macos")]
impl RetainableNotificationOpenerChild for tokio::process::Child {
    /// 使用 Tokio 同步 kill 入口请求真实 opener 终止。
    fn start_kill_owned(&mut self) -> std::io::Result<()> {
        self.start_kill()
    }

    /// 使用 try_wait 确认真实 opener 是否已经到达可丢弃终态。
    fn try_wait_terminal(&mut self) -> std::io::Result<bool> {
        self.try_wait().map(|status| status.is_some())
    }

    /// 使用操作系统 PID 作为当前 owner 生命周期内的稳定标识。
    fn owner_id(&self) -> u64 {
        u64::from(self.id().unwrap_or_default())
    }
}

#[cfg(any(target_os = "macos", test))]
/// 静态 owner 内的一项 opener 子进程及一次性诊断状态。
struct RetainedNotificationOpenerEntry {
    owner_id: u64,
    child: Box<dyn RetainableNotificationOpenerChild>,
    diagnostic_emitted: bool,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
/// 保存被移交的 opener、在途槽位及唯一 reaper 运行标志。
struct RetainedNotificationOpenerState {
    children: Vec<RetainedNotificationOpenerEntry>,
    reaping_child_ids: Vec<u64>,
    owned_slots: usize,
    reaper_running: bool,
}

#[cfg(any(target_os = "macos", test))]
/// 进程级持续持有异常退出 opener 与 reaper 线程自身的稳定 owner。
pub(super) struct RetainedNotificationOpenerOwner {
    shared: Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>,
    reapers: Mutex<Vec<thread::JoinHandle<()>>>,
}

#[cfg(any(target_os = "macos", test))]
/// spawn 前占用的有界 opener 槽位，失败与正常终态会自动归还。
pub(super) struct NotificationOpenerReservation {
    owner: &'static RetainedNotificationOpenerOwner,
    active: bool,
}

#[cfg(any(target_os = "macos", test))]
/// 从 spawn 成功到 wait 终态全程持有 opener，异常 Drop 时转交静态 owner。
pub(super) struct NotificationOpenerChildGuard<C>
where
    C: RetainableNotificationOpenerChild + 'static,
{
    reservation: Option<NotificationOpenerReservation>,
    child: Option<C>,
}

#[cfg(any(target_os = "macos", test))]
/// 临时接管 retained opener 批次；panic 展开时把所有未终态项归还静态 owner。
struct NotificationOpenerReapBatchGuard {
    shared: Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>,
    children: Vec<RetainedNotificationOpenerEntry>,
}

#[cfg(any(target_os = "macos", test))]
/// reaper panic 时恢复运行标志，使后续机会式回收可以重新启动线程。
struct NotificationOpenerReaperRunGuard {
    shared: Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>,
    active: bool,
}

#[cfg(any(target_os = "macos", test))]
/// 所有通知设置 opener 共享的进程级最终 owner。
static RETAINED_NOTIFICATION_OPENERS: OnceLock<RetainedNotificationOpenerOwner> = OnceLock::new();

impl RetainedNotificationTaskOwner {
    /// 创建空的进程级通知任务 owner。
    pub(super) fn new() -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(RetainedNotificationTaskState::default()),
                Condvar::new(),
            )),
            reapers: Mutex::new(Vec::new()),
        }
    }

    /// 接管被 abort 但尚未确认终态的句柄，并确保后台 reaper 持续拥有它们。
    fn retain(&'static self, tasks: Vec<JoinHandle<()>>) {
        if tasks.is_empty() {
            return;
        }
        self.reap_finished_reapers();
        let should_start = {
            let (state, wake) = &*self.shared;
            let mut state = lock_unpoisoned(state);
            state.tasks.extend(tasks);
            let should_start = !state.reaper_running;
            if should_start {
                state.reaper_running = true;
            }
            wake.notify_all();
            should_start
        };
        if should_start {
            self.start_reaper();
        }
    }

    /// 机会式清理已结束 reaper，并在启动失败后为遗留任务重建 reaper。
    fn reap_finished(&'static self) {
        self.reap_finished_reapers();
        let should_start = {
            let mut state = lock_unpoisoned(&self.shared.0);
            let should_start = !state.tasks.is_empty() && !state.reaper_running;
            if should_start {
                state.reaper_running = true;
            }
            should_start
        };
        if should_start {
            self.start_reaper();
        }
    }

    /// 启动只轮询终态的具名线程，并由当前静态 owner 保存线程句柄。
    fn start_reaper(&'static self) {
        let shared = Arc::clone(&self.shared);
        match thread::Builder::new()
            .name("loki-metis-notification-task-reaper".to_owned())
            .spawn(move || retained_notification_task_reaper_loop(&shared))
        {
            Ok(reaper) => lock_unpoisoned(&self.reapers).push(reaper),
            Err(error) => {
                lock_unpoisoned(&self.shared.0).reaper_running = false;
                tracing::error!(%error, "failed to start notification task reaper");
            }
        }
    }

    /// join 已终态 reaper；仍运行的 reaper 线程继续由静态 owner 持有。
    fn reap_finished_reapers(&self) {
        let mut reapers = lock_unpoisoned(&self.reapers);
        let mut index = 0;
        while index < reapers.len() {
            if reapers[index].is_finished() {
                let reaper = reapers.swap_remove(index);
                if reaper.join().is_err() {
                    tracing::warn!("notification task reaper stopped unexpectedly");
                }
            } else {
                index += 1;
            }
        }
    }

    /// 返回进程级 owner 是否仍持有指定 Tokio 任务，避免并行测试依赖全局计数。
    #[cfg(test)]
    fn owns_task_id(&self, task_id: tokio::task::Id) -> bool {
        lock_unpoisoned(&self.shared.0)
            .tasks
            .iter()
            .any(|task| task.inner().id() == task_id)
    }
}

#[cfg(any(target_os = "macos", test))]
impl RetainedNotificationOpenerOwner {
    /// 创建空的进程级 opener owner。
    pub(super) fn new() -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(RetainedNotificationOpenerState::default()),
                Condvar::new(),
            )),
            reapers: Mutex::new(Vec::new()),
        }
    }

    /// 在固定容量内预约一个 spawn 槽位，并机会式回收已有终态子进程。
    pub(super) fn reserve(&'static self) -> Result<NotificationOpenerReservation, &'static str> {
        self.reap_finished_reapers();
        self.reap_once();
        self.ensure_reaper();
        let mut state = lock_unpoisoned(&self.shared.0);
        if state.owned_slots >= RETAINED_NOTIFICATION_OPENER_CAPACITY {
            return Err(NOTIFICATION_OPENER_CAPACITY_ERROR);
        }
        state.owned_slots += 1;
        Ok(NotificationOpenerReservation {
            owner: self,
            active: true,
        })
    }

    /// 后续 worker 生命周期机会式确认终态，并恢复此前启动失败的 reaper。
    fn reap_finished(&'static self) {
        self.reap_finished_reapers();
        self.reap_once();
        self.ensure_reaper();
    }

    /// 把已预约的未终态子进程转入 retained 表，槽位所有权保持不变。
    fn retain_reserved(&'static self, child: Box<dyn RetainableNotificationOpenerChild>) {
        let owner_id = child.owner_id();
        let (state, wake) = &*self.shared;
        let mut state = lock_unpoisoned(state);
        debug_assert!(state.owned_slots > 0);
        state.children.push(RetainedNotificationOpenerEntry {
            owner_id,
            child,
            diagnostic_emitted: false,
        });
        wake.notify_all();
    }

    /// 归还未采用子进程的预约槽位。
    fn release_reservation(&self) {
        let mut state = lock_unpoisoned(&self.shared.0);
        debug_assert!(state.owned_slots > 0);
        state.owned_slots = state.owned_slots.saturating_sub(1);
        self.shared.1.notify_all();
    }

    /// 临时取出 retained 批次并在锁外执行非阻塞终态检查与 kill 重试。
    fn reap_once(&'static self) {
        let mut batch = NotificationOpenerReapBatchGuard::take(&self.shared);
        reap_terminal_notification_openers(&mut batch);
    }

    /// retained 表非空且当前没有 reaper 时启动唯一后台回收线程。
    fn ensure_reaper(&'static self) {
        let should_start = {
            let mut state = lock_unpoisoned(&self.shared.0);
            let should_start = !state.children.is_empty() && !state.reaper_running;
            if should_start {
                state.reaper_running = true;
            }
            should_start
        };
        if should_start {
            self.start_reaper();
        }
    }

    /// 启动只做 kill 重试与 try_wait 的具名 reaper，并稳定保存线程句柄。
    fn start_reaper(&'static self) {
        let shared = Arc::clone(&self.shared);
        match thread::Builder::new()
            .name("loki-metis-notification-opener-reaper".to_owned())
            .spawn(move || retained_notification_opener_reaper_loop(&shared))
        {
            Ok(reaper) => lock_unpoisoned(&self.reapers).push(reaper),
            Err(error) => {
                lock_unpoisoned(&self.shared.0).reaper_running = false;
                tracing::error!(%error, "failed to start notification opener reaper");
            }
        }
    }

    /// join 已终态 reaper；仍运行的线程继续由静态 owner 持有。
    fn reap_finished_reapers(&self) {
        let mut reapers = lock_unpoisoned(&self.reapers);
        let mut index = 0;
        while index < reapers.len() {
            if reapers[index].is_finished() {
                let reaper = reapers.swap_remove(index);
                if reaper.join().is_err() {
                    tracing::warn!("notification opener reaper stopped unexpectedly");
                }
            } else {
                index += 1;
            }
        }
    }

    /// 返回静态 owner 是否持有指定测试子进程。
    #[cfg(test)]
    pub(super) fn owns_test_child(&self, child_id: u64) -> bool {
        let state = lock_unpoisoned(&self.shared.0);
        state
            .children
            .iter()
            .any(|entry| entry.owner_id == child_id)
            || state.reaping_child_ids.contains(&child_id)
    }

    /// 返回当前 owner 的后台 reaper 是否占有运行标志。
    #[cfg(test)]
    pub(super) fn reaper_running(&self) -> bool {
        lock_unpoisoned(&self.shared.0).reaper_running
    }
}

#[cfg(any(target_os = "macos", test))]
impl NotificationOpenerReservation {
    /// spawn 成功后立即把具体子进程放入取消安全 guard。
    pub(super) fn adopt<C>(self, child: C) -> NotificationOpenerChildGuard<C>
    where
        C: RetainableNotificationOpenerChild + 'static,
    {
        NotificationOpenerChildGuard {
            reservation: Some(self),
            child: Some(child),
        }
    }

    /// 把预约转换为 retained 项；转换与在途回收全程保持同一个槽位。
    fn retain<C>(&mut self, child: C)
    where
        C: RetainableNotificationOpenerChild + 'static,
    {
        self.owner.retain_reserved(Box::new(child));
        self.active = false;
        self.owner.reap_once();
        self.owner.ensure_reaper();
    }
}

#[cfg(any(target_os = "macos", test))]
impl NotificationOpenerReapBatchGuard {
    /// 在状态锁内仅移出句柄，随后由批次守卫在锁外持有。
    fn take(shared: &Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>) -> Self {
        let children = {
            let mut state = lock_unpoisoned(&shared.0);
            let children = std::mem::take(&mut state.children);
            state
                .reaping_child_ids
                .extend(children.iter().map(|entry| entry.owner_id));
            children
        };
        Self {
            shared: Arc::clone(shared),
            children,
        }
    }

    /// 真实终态项离开批次时释放其全生命周期槽位。
    fn release_terminal_slot(&self, child_id: u64) {
        let (state, wake) = &*self.shared;
        let mut state = lock_unpoisoned(state);
        debug_assert!(state.owned_slots > 0);
        if let Some(index) = state
            .reaping_child_ids
            .iter()
            .position(|candidate| *candidate == child_id)
        {
            state.reaping_child_ids.swap_remove(index);
        } else {
            debug_assert!(false, "terminal opener must belong to an in-flight batch");
        }
        state.owned_slots = state.owned_slots.saturating_sub(1);
        wake.notify_all();
    }
}

#[cfg(any(target_os = "macos", test))]
impl Drop for NotificationOpenerReapBatchGuard {
    /// 正常轮询或 panic 展开都把全部未终态 opener 原样归还静态 owner。
    fn drop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        let (state, wake) = &*self.shared;
        let mut state = lock_unpoisoned(state);
        for entry in &self.children {
            if let Some(index) = state
                .reaping_child_ids
                .iter()
                .position(|candidate| *candidate == entry.owner_id)
            {
                state.reaping_child_ids.swap_remove(index);
            } else {
                debug_assert!(false, "retained opener must belong to an in-flight batch");
            }
        }
        state.children.append(&mut self.children);
        wake.notify_all();
    }
}

#[cfg(any(target_os = "macos", test))]
impl NotificationOpenerReaperRunGuard {
    /// 标记当前线程负责 reaper_running，panic 时自动恢复。
    fn new(shared: &Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>) -> Self {
        Self {
            shared: Arc::clone(shared),
            active: true,
        }
    }

    /// 在线程正常退出的同一状态锁内释放运行标志，避免交接竞态。
    fn finish(&mut self, state: &mut RetainedNotificationOpenerState) {
        state.reaper_running = false;
        self.active = false;
    }
}

#[cfg(any(target_os = "macos", test))]
impl Drop for NotificationOpenerReaperRunGuard {
    /// panic 展开时允许后续 reserve 或 worker 生命周期重新启动 reaper。
    fn drop(&mut self) {
        if self.active {
            lock_unpoisoned(&self.shared.0).reaper_running = false;
            self.shared.1.notify_all();
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl Drop for NotificationOpenerReservation {
    /// spawn 失败或正常终态时归还尚未转换为 retained 项的槽位。
    fn drop(&mut self) {
        if self.active {
            self.owner.release_reservation();
            self.active = false;
        }
    }
}

#[cfg(any(target_os = "macos", test))]
impl<C> NotificationOpenerChildGuard<C>
where
    C: RetainableNotificationOpenerChild + 'static,
{
    /// 返回真实子进程的唯一可变引用，供受限 wait/kill 流程使用。
    pub(super) fn child_mut(&mut self) -> &mut C {
        self.child
            .as_mut()
            .expect("notification opener guard must own its child")
    }

    /// 请求子进程终止但继续保留 guard 所有权。
    pub(super) fn start_kill(&mut self) -> std::io::Result<()> {
        self.child_mut().start_kill_owned()
    }

    /// 仅在调用方已观察真实 wait 终态后释放子进程与容量槽位。
    pub(super) fn mark_terminal(&mut self) {
        self.child.take();
        self.reservation.take();
    }
}

#[cfg(any(target_os = "macos", test))]
impl<C> Drop for NotificationOpenerChildGuard<C>
where
    C: RetainableNotificationOpenerChild + 'static,
{
    /// future 取消、panic 或回收超时均把原 Child 转交静态 owner，禁止局部 Drop。
    fn drop(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        if let Some(mut reservation) = self.reservation.take() {
            reservation.retain(child);
        }
    }
}

impl NotificationWorkerTaskOwner {
    /// 从 worker 初始句柄创建唯一任务 owner。
    pub(super) fn new(task: JoinHandle<()>) -> Self {
        reap_retained_notification_tasks();
        #[cfg(any(target_os = "macos", test))]
        reap_retained_notification_openers();
        Self {
            state: Mutex::new(NotificationWorkerTaskState {
                tasks: vec![task],
                shutdown_schedule: None,
            }),
        }
    }

    /// 原子接管当前句柄并冻结首次关闭时限，返回的守卫覆盖后续所有 await。
    pub(super) fn begin_shutdown(
        &self,
        timeout: Duration,
        abort_reap_budget: Duration,
    ) -> (
        NotificationShutdownTaskBatchGuard<'_>,
        NotificationShutdownSchedule,
    ) {
        reap_retained_notification_tasks();
        #[cfg(any(target_os = "macos", test))]
        reap_retained_notification_openers();
        let now = tokio::time::Instant::now();
        let final_deadline = now + timeout;
        let cooperative_deadline = final_deadline
            .checked_sub(abort_reap_budget.min(timeout / 2))
            .unwrap_or(final_deadline);
        let proposed_schedule = NotificationShutdownSchedule {
            cooperative_deadline,
            final_deadline,
        };
        let (tasks, schedule) = {
            let mut state = self.lock_state();
            let schedule = *state.shutdown_schedule.get_or_insert(proposed_schedule);
            (std::mem::take(&mut state.tasks), schedule)
        };
        (
            NotificationShutdownTaskBatchGuard { owner: self, tasks },
            schedule,
        )
    }

    /// 同步销毁路径中止所有任务，并把原句柄移交进程级稳定 owner。
    pub(super) fn abort_and_retain_all(&mut self) {
        let tasks = self
            .state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tasks
            .drain(..)
            .collect::<Vec<_>>();
        for task in &tasks {
            task.abort();
        }
        retained_notification_task_owner().retain(tasks);
    }

    /// 返回 owner 当前持有的 worker 句柄数。
    #[cfg(test)]
    pub(super) fn task_count(&self) -> usize {
        self.lock_state().tasks.len()
    }

    /// 返回首次关闭冻结的时限，供重复调用预算回归验证。
    #[cfg(test)]
    pub(super) fn shutdown_schedule(&self) -> Option<NotificationShutdownSchedule> {
        self.lock_state().shutdown_schedule
    }

    /// 取得任务 owner 锁；测试 panic 污染后仍保留关闭与回收能力。
    fn lock_state(&self) -> std::sync::MutexGuard<'_, NotificationWorkerTaskState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for NotificationShutdownTaskBatchGuard<'_> {
    /// 正常超时、future 取消和 panic 展开都经此路径恢复真实句柄所有权。
    fn drop(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        self.owner.lock_state().tasks.append(&mut self.tasks);
    }
}

/// 返回所有通知 worker 实例共享的最终任务 owner。
fn retained_notification_task_owner() -> &'static RetainedNotificationTaskOwner {
    RETAINED_NOTIFICATION_TASKS.get_or_init(RetainedNotificationTaskOwner::new)
}

/// 后续 worker 创建或 shutdown 时机会式回收静态表，并恢复可能启动失败的 reaper。
fn reap_retained_notification_tasks() {
    if let Some(owner) = RETAINED_NOTIFICATION_TASKS.get() {
        owner.reap_finished();
    }
}

/// 只在 Tokio 句柄报告完成后 poll 终态，未终态句柄始终留在静态 owner。
fn retained_notification_task_reaper_loop(
    shared: &Arc<(Mutex<RetainedNotificationTaskState>, Condvar)>,
) {
    loop {
        let (state, wake) = &**shared;
        let mut state = lock_unpoisoned(state);
        let waker = futures::task::noop_waker_ref();
        let mut context = Context::from_waker(waker);
        state.tasks.retain_mut(|task| {
            if !task.inner().is_finished() {
                return true;
            }
            match Pin::new(task).poll(&mut context) {
                Poll::Ready(result) => {
                    super::log_notification_worker_join_result(result);
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
        drop(wake.wait_timeout(state, RETAINED_NOTIFICATION_TASK_POLL));
    }
}

#[cfg(any(target_os = "macos", test))]
/// 返回所有通知设置 opener 共享的进程级最终 owner。
fn retained_notification_opener_owner() -> &'static RetainedNotificationOpenerOwner {
    RETAINED_NOTIFICATION_OPENERS.get_or_init(RetainedNotificationOpenerOwner::new)
}

#[cfg(any(target_os = "macos", test))]
/// 在后续 worker 创建或 shutdown 时机会式回收 opener，并恢复 reaper。
fn reap_retained_notification_openers() {
    if let Some(owner) = RETAINED_NOTIFICATION_OPENERS.get() {
        owner.reap_finished();
    }
}

#[cfg(any(target_os = "macos", test))]
/// spawn 前预约有界槽位，防止不可回收 opener 无界累积。
pub(super) fn reserve_notification_opener_slot()
-> Result<NotificationOpenerReservation, &'static str> {
    retained_notification_opener_owner().reserve()
}

#[cfg(any(target_os = "macos", test))]
/// 轮询并移除真实终态 opener；错误项保留所有权并只记录一次诊断。
fn reap_terminal_notification_openers(batch: &mut NotificationOpenerReapBatchGuard) {
    let mut index = 0;
    while index < batch.children.len() {
        let terminal_result = batch.children[index].child.try_wait_terminal();
        match terminal_result {
            Ok(true) => {
                let terminal_entry = batch.children.swap_remove(index);
                batch.release_terminal_slot(terminal_entry.owner_id);
                drop(terminal_entry);
            }
            Ok(false) => {
                let entry = &mut batch.children[index];
                if let Err(error) = entry.child.start_kill_owned()
                    && !entry.diagnostic_emitted
                {
                    tracing::warn!(%error, "retained notification opener kill retry failed");
                    entry.diagnostic_emitted = true;
                }
                index += 1;
            }
            Err(error) => {
                let entry = &mut batch.children[index];
                let kill_error = entry.child.start_kill_owned().err();
                if !entry.diagnostic_emitted {
                    tracing::warn!(%error, ?kill_error, "retained notification opener terminal check failed");
                    entry.diagnostic_emitted = true;
                }
                index += 1;
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
/// 在独立线程中持续确认子进程终态；未终态项始终留在静态 owner。
fn retained_notification_opener_reaper_loop(
    shared: &Arc<(Mutex<RetainedNotificationOpenerState>, Condvar)>,
) {
    let mut run_guard = NotificationOpenerReaperRunGuard::new(shared);
    loop {
        let mut batch = NotificationOpenerReapBatchGuard::take(shared);
        reap_terminal_notification_openers(&mut batch);
        drop(batch);

        let (state, wake) = &**shared;
        let mut state = lock_unpoisoned(state);
        if state.children.is_empty() {
            run_guard.finish(&mut state);
            wake.notify_all();
            return;
        }
        drop(wake.wait_timeout(state, RETAINED_NOTIFICATION_OPENER_POLL));
    }
}

/// 锁污染不得遗失仍运行的句柄，因此所有进程级 owner 锁统一恢复内部值。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 返回进程级 owner 是否仍持有指定 Tokio 任务。
#[cfg(test)]
pub(super) fn retained_notification_owner_owns_task(task_id: tokio::task::Id) -> bool {
    retained_notification_task_owner().owns_task_id(task_id)
}

/// 返回静态 owner 是否仍持有指定测试 opener。
#[cfg(test)]
pub(super) fn retained_notification_opener_is_owned(child_id: u64) -> bool {
    retained_notification_opener_owner().owns_test_child(child_id)
}
