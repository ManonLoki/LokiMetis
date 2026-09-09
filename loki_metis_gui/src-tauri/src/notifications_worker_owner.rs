//! 通知 worker 的关闭时限与 JoinHandle 所有权；不承担通知业务或宿主投递。

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

impl RetainedNotificationTaskOwner {
    /// 创建空的进程级通知任务 owner。
    fn new() -> Self {
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

impl NotificationWorkerTaskOwner {
    /// 从 worker 初始句柄创建唯一任务 owner。
    pub(super) fn new(task: JoinHandle<()>) -> Self {
        reap_retained_notification_tasks();
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
