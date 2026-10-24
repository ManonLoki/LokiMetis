//! CDP handler 任务的进程级容量与回收：spawn 前预约名额，取消后保留句柄直到确认终态。

use super::*;

/// 同时存在的活跃或待回收 CDP handler 上限；达到上限后在 spawn 前施加背压。
pub(super) const MAX_OWNED_HANDLER_TASKS: usize = 256;

#[derive(Default)]
/// 进程级持有全部 handler 容量令牌，以及已取消但尚未确认终态的任务。
pub(super) struct HandlerTaskReaper {
    /// 运行中的 handler 任务。
    pub(super) active: HashMap<tokio::task::Id, AbortHandle>,
    /// 已取消待确认终态的任务。
    pub(super) retained: Vec<JoinHandle<()>>,
    /// 正在回收的任务 ID。
    reaping: HashSet<tokio::task::Id>,
    /// 当前占用的容量。
    pub(super) owned_tasks: usize,
    /// 所有预约（含清理用途）已关闭。
    pub(super) reservations_closed: bool,
    /// 普通预约已关闭，只剩清理用途可以预约。
    pub(super) normal_closed: bool,
}

/// spawn 前取得的容量令牌；未绑定任务就析构时自动归还名额。
pub(super) struct HandlerTaskReservation {
    /// 预约仍有效；为 true 时析构会归还名额。
    pub(super) active: bool,
    /// 清理用途预约，在普通预约关闭后仍可取得。
    pub(super) cleanup: bool,
}

impl HandlerTaskReservation {
    /// 把已登记容量与新任务绑定，并将所有权移交给 guard。
    pub(super) fn bind(mut self, task: JoinHandle<()>) -> HandlerTaskGuard {
        register_handler_task_for_purpose(&task, self.cleanup);
        self.active = false;
        HandlerTaskGuard(Some(task))
    }
}

impl Drop for HandlerTaskReservation {
    /// spawn 前失败或 panic 时归还尚未绑定的容量。
    fn drop(&mut self) {
        if self.active {
            release_handler_task_capacity();
        }
    }
}

/// 异步等待期间临时取出的句柄；等待 future 取消或 panic 时自动归还稳定 owner。
struct HandlerTaskReapGuard(Option<JoinHandle<()>>);

impl HandlerTaskReapGuard {
    /// 临时接管一个已请求取消的 handler。
    pub(super) fn new(task: JoinHandle<()>) -> Self {
        Self(Some(task))
    }

    /// 返回当前等待中的原始句柄。
    pub(super) fn task_mut(&mut self) -> &mut JoinHandle<()> {
        self.0.as_mut().expect("handler reap guard must own a task")
    }

    /// 任务已确认终态，移除 in-flight 标记并归还容量。
    pub(super) fn complete(mut self) {
        let task = self.0.take().expect("completed handler task must exist");
        let task_id = task.id();
        {
            let mut owner = lock_handler_task_reaper();
            owner.reaping.remove(&task_id);
            owner.owned_tasks = owner.owned_tasks.saturating_sub(1);
        }
        drop(task);
    }
}

impl Drop for HandlerTaskReapGuard {
    /// 未确认终态时把原句柄放回进程级 owner，绝不因等待 future 析构而 detach。
    fn drop(&mut self) {
        let Some(task) = self.0.take() else {
            return;
        };
        let task_id = task.id();
        let mut owner = lock_handler_task_reaper();
        owner.reaping.remove(&task_id);
        owner.retained.push(task);
    }
}

/// 返回跨 `SkinService` 生命周期稳定存在的 handler 任务 owner。
fn handler_task_reaper() -> &'static StdMutex<HandlerTaskReaper> {
    static OWNER: std::sync::OnceLock<StdMutex<HandlerTaskReaper>> = std::sync::OnceLock::new();
    OWNER.get_or_init(|| StdMutex::new(HandlerTaskReaper::default()))
}

/// 串行化进程级回收轮次，避免一个 shutdown 越过另一个正在等待的 handler。
fn handler_task_reap_lock() -> &'static Mutex<()> {
    static REAP_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    REAP_LOCK.get_or_init(|| Mutex::new(()))
}

/// 取得 handler owner 的短临界区；锁中毒时仍保留既有任务所有权。
fn lock_handler_task_reaper() -> std::sync::MutexGuard<'static, HandlerTaskReaper> {
    handler_task_reaper()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 在短锁内清除已确认终态的 retained handle，并同步归还容量。
fn reap_finished_handler_tasks_locked(owner: &mut HandlerTaskReaper) -> usize {
    let before = owner.retained.len();
    owner.retained.retain(|task| !task.is_finished());
    let reaped = before.saturating_sub(owner.retained.len());
    owner.owned_tasks = owner.owned_tasks.saturating_sub(reaped);
    reaped
}

/// 移除已经确认到达终态的句柄，避免正常运行中累积已完成任务。
pub(super) fn reap_finished_handler_tasks() -> usize {
    reap_finished_handler_tasks_locked(&mut lock_handler_task_reaper())
}

/// 判断当前活跃与 retained handler 总数是否仍低于硬上限。
pub(super) fn has_handler_task_capacity(owner: &HandlerTaskReaper) -> bool {
    !owner.reservations_closed
        && !owner.normal_closed
        && owner.owned_tasks < MAX_OWNED_HANDLER_TASKS
}

/// 在调用方持有短锁时原子占用容量；失败不会修改计数。
pub(super) fn try_reserve_handler_task_capacity_locked(owner: &mut HandlerTaskReaper) -> bool {
    if !has_handler_task_capacity(owner) {
        return false;
    }
    owner.owned_tasks = owner.owned_tasks.saturating_add(1);
    true
}

/// Draining阶段只允许有限容量的专用清理连接。
pub(super) fn reserve_handler_task_for_purpose(cleanup: bool) -> Result<HandlerTaskReservation, AppError> {
    let mut owner = lock_handler_task_reaper();
    reap_finished_handler_tasks_locked(&mut owner);
    let accepted = if cleanup {
        if !owner.reservations_closed && owner.owned_tasks < MAX_OWNED_HANDLER_TASKS {
            owner.owned_tasks += 1;
            true
        } else {
            false
        }
    } else {
        try_reserve_handler_task_capacity_locked(&mut owner)
    };
    if !accepted {
        return Err(AppError::new(
            "skin.cdp_failed",
            "本机调试会话仍在回收，请稍后重试。",
        ));
    }
    Ok(HandlerTaskReservation {
        active: true,
        cleanup,
    })
}

/// 将新建 handler 登记为 active；若退出门禁已关闭则立即请求取消。
/// 绑定复核预约用途，关闭前尚未绑定的普通连接不能越过退出门禁。
fn register_handler_task_for_purpose(task: &JoinHandle<()>, cleanup: bool) {
    register_handler_task_locked(&mut lock_handler_task_reaper(), task, cleanup);
}
/// 短锁内绑定与门禁复核共用生产路径，测试可注入独立owner。
pub(super) fn register_handler_task_locked(
    owner: &mut HandlerTaskReaper,
    task: &JoinHandle<()>,
    cleanup: bool,
) {
    owner.active.insert(task.id(), task.abort_handle());
    if owner.reservations_closed || (owner.normal_closed && !cleanup) {
        task.abort();
    }
}

/// 在短锁内关闭新 handler 准入并取消所有已经登记的 active handler。
pub(super) fn close_handler_task_reservations_locked(owner: &mut HandlerTaskReaper) {
    owner.reservations_closed = true;
    for task in owner.active.values() {
        task.abort();
    }
}

/// 清理开始只关闭普通准入，已绑定watcher的通信通道保持到清理终态。
pub(super) fn begin_handler_task_shutdown() {
    lock_handler_task_reaper().normal_closed = true;
}

/// 清理完成后原子关闭全部准入，并取消剩余探针。
pub(super) fn close_handler_task_reservations() {
    close_handler_task_reservations_locked(&mut lock_handler_task_reaper());
}

/// 归还一个未绑定任务的容量名额。
fn release_handler_task_capacity() {
    let mut owner = lock_handler_task_reaper();
    owner.owned_tasks = owner.owned_tasks.saturating_sub(1);
}

/// 请求取消并把未到终态的原始 JoinHandle 移交稳定 owner；容量令牌继续保留。
pub(super) fn retain_aborted_handler_task(task: JoinHandle<()>) {
    task.abort();
    let mut owner = lock_handler_task_reaper();
    reap_finished_handler_tasks_locked(&mut owner);
    owner.active.remove(&task.id());
    if task.is_finished() {
        owner.owned_tasks = owner.owned_tasks.saturating_sub(1);
    } else {
        owner.retained.push(task);
    }
}

/// 从稳定 owner 取出一个任务并登记为正在回收，供取消安全 guard 临时持有。
fn take_retained_handler_task() -> Option<HandlerTaskReapGuard> {
    let mut owner = lock_handler_task_reaper();
    reap_finished_handler_tasks_locked(&mut owner);
    let task = owner.retained.pop()?;
    owner.reaping.insert(task.id());
    Some(HandlerTaskReapGuard::new(task))
}

/// 判断 shutdown 是否仍需等待 handler；生产 gate 关闭后 active 任务也必须清零。
fn has_pending_handler_tasks() -> bool {
    let owner = lock_handler_task_reaper();
    if owner.reservations_closed {
        owner.owned_tasks > 0
    } else {
        !owner.retained.is_empty() || !owner.reaping.is_empty()
    }
}

/// 在共同截止前确认全部已取消 handler 到达终态；超时任务仍由稳定 owner 持有。
pub(super) async fn reap_handler_tasks_until(deadline: tokio::time::Instant) -> Result<usize, AppError> {
    if tokio::time::Instant::now() >= deadline {
        reap_finished_handler_tasks();
        return if has_pending_handler_tasks() {
            Err(watch_stop_timeout_error())
        } else {
            Ok(0)
        };
    }
    let _reap = tokio::time::timeout_at(deadline, handler_task_reap_lock().lock())
        .await
        .map_err(|_| watch_stop_timeout_error())?;
    let mut reaped = 0_usize;
    loop {
        if tokio::time::Instant::now() >= deadline {
            reap_finished_handler_tasks();
            return if has_pending_handler_tasks() {
                Err(watch_stop_timeout_error())
            } else {
                Ok(reaped)
            };
        }
        let Some(mut task) = take_retained_handler_task() else {
            if !has_pending_handler_tasks() {
                return Ok(reaped);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::time::sleep(remaining.min(Duration::from_millis(10))).await;
            continue;
        };

        task.task_mut().abort();
        let completed = if task.task_mut().is_finished() {
            let _ = task.task_mut().await;
            true
        } else {
            tokio::time::timeout_at(deadline, task.task_mut())
                .await
                .is_ok()
        };
        if !completed {
            return Err(watch_stop_timeout_error());
        }
        task.complete();
        reaped = reaped.saturating_add(1);
    }
}

#[cfg(test)]
/// 判断指定任务是否仍由稳定 owner 或取消安全回收 guard 持有。
pub(super) fn handler_task_is_retained(task_id: tokio::task::Id) -> bool {
    let owner = lock_handler_task_reaper();
    owner.retained.iter().any(|task| task.id() == task_id) || owner.reaping.contains(&task_id)
}
