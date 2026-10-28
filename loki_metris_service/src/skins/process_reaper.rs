//! 统一持有 Unix launcher 子进程，确保超时、取消和退出路径不会丢失回收责任。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::{Notify, watch};

/// future 被取消时允许同步确认 SIGKILL 终态的最大预算。
const CANCEL_REAP_TIMEOUT: Duration = Duration::from_millis(250);
/// 应用退出用于回收 launcher 的总预算；重复调用不会重置该截止。
pub(super) const PROCESS_REAPER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// active、retained、回收中句柄与 spawn reservation 共用的进程硬上限。
const OWNED_PROCESS_CAPACITY: usize = 32;
/// shutdown 等待 owner 变化的最长轮询间隔，通知丢失也不会越过总截止。
const PROCESS_REAPER_STATE_POLL: Duration = Duration::from_millis(10);

/// 子进程 owner 在启动、等待或最终回收阶段的稳定错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OwnedProcessError {
    /// 启动失败。
    Spawn,
    /// 等待或读写失败。
    Io,
    /// 回收超时，子进程已移交给进程级回收器。
    ReapTimedOut,
    /// 应用正在关闭，拒绝启动。
    ShuttingDown,
    /// 进程数达到上限。
    CapacityExceeded,
}

/// 尚未确认终态的子进程登记表；锁内只移动句柄，不执行等待。
#[derive(Default)]
struct ProcessReaperState {
    /// 已移交、待最终回收的子进程。
    retained: Vec<Child>,
    /// 运行中进程的关闭信号发送端，按登记 ID 索引。
    active: HashMap<u64, watch::Sender<bool>>,
    /// 已预约尚未启动的槽位数。
    spawn_reservations: usize,
    /// 当前拥有的进程数，含预约与待回收。
    owned_processes: usize,
    /// 下一个登记 ID。
    next_registration_id: u64,
    /// 已开始关闭，不再接受新进程。
    shutting_down: bool,
    /// 首次关闭确定的共同截止时间。
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// 持有进程登记表与非阻塞变化通知，不在锁内运行进程操作。
struct ProcessReaper {
    /// 登记表。
    state: Mutex<ProcessReaperState>,
    /// 登记变化时唤醒等待者。
    changed: Notify,
}

impl Default for ProcessReaper {
    /// 创建空的进程级 owner。
    fn default() -> Self {
        Self {
            state: Mutex::new(ProcessReaperState::default()),
            changed: Notify::new(),
        }
    }
}

/// 取得进程级登记表。
fn process_reaper() -> &'static ProcessReaper {
    static REAPER: OnceLock<ProcessReaper> = OnceLock::new();
    REAPER.get_or_init(ProcessReaper::default)
}

/// 锁中毒不能使指定 owner 的真实句柄遗失。
fn lock_reaper(
    reaper: &'static ProcessReaper,
) -> std::sync::MutexGuard<'static, ProcessReaperState> {
    reaper
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 锁中毒不能使真实子进程句柄遗失。
fn lock_process_reaper() -> std::sync::MutexGuard<'static, ProcessReaperState> {
    lock_reaper(process_reaper())
}

/// spawn 前占用有界槽位；失败归还，已启动 Child 在 panic 或关闭竞态中转 retained。
struct ProcessSpawnReservation {
    /// 进程级回收器。
    reaper: &'static ProcessReaper,
    /// 已启动的子进程；启动前为 None。
    child: Option<Child>,
    /// 预约仍有效；为 true 时析构会归还槽位。
    active: bool,
}

impl ProcessSpawnReservation {
    /// 在关闭门禁与总容量下预留一个 spawn 槽位。
    fn reserve(reaper: &'static ProcessReaper) -> Result<Self, OwnedProcessError> {
        let mut state = lock_reaper(reaper);
        if state.shutting_down {
            return Err(OwnedProcessError::ShuttingDown);
        }
        if state.owned_processes >= OWNED_PROCESS_CAPACITY {
            return Err(OwnedProcessError::CapacityExceeded);
        }
        state.spawn_reservations += 1;
        state.owned_processes += 1;
        Ok(Self {
            reaper,
            child: None,
            active: true,
        })
    }

    /// 把锁外 spawn 产生的 Child 原子转为 active，或在 shutdown 竞态中转为 retained。
    #[cfg(test)]
    fn adopt(mut self, child: Child) -> Result<OwnedProcessChild, OwnedProcessError> {
        self.child = Some(child);
        self.finish_adoption()
    }

    /// 在 mutex 之外 spawn，并在返回后立即由 reservation 接住 Child。
    fn spawn_command(
        mut self,
        command: &mut Command,
    ) -> Result<OwnedProcessChild, OwnedProcessError> {
        command.kill_on_drop(true);
        self.child = Some(command.spawn().map_err(|_| OwnedProcessError::Spawn)?);
        self.finish_adoption()
    }

    /// 在 owner mutex 之外尽力终止已启动 Child，失败仍保留真实句柄。
    fn request_kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }

    /// 在 reservation 已持有 Child 后完成计数不变的状态移交。
    fn finish_adoption(mut self) -> Result<OwnedProcessChild, OwnedProcessError> {
        let (shutdown, receiver) = watch::channel(false);
        let mut state = lock_reaper(self.reaper);
        if state.shutting_down {
            drop(state);
            self.request_kill();
            let mut state = lock_reaper(self.reaper);
            debug_assert!(state.shutting_down);
            state
                .retained
                .push(self.child.take().expect("reservation 必须持有子进程"));
            debug_assert!(state.spawn_reservations > 0);
            state.spawn_reservations -= 1;
            self.active = false;
            drop(state);
            self.reaper.changed.notify_waiters();
            return Err(OwnedProcessError::ShuttingDown);
        }
        let registration_id = allocate_registration_id(&mut state);
        state.active.insert(registration_id, shutdown);
        let child = self.child.take().expect("reservation 必须持有子进程");
        debug_assert!(state.spawn_reservations > 0);
        state.spawn_reservations -= 1;
        self.active = false;
        drop(state);
        self.reaper.changed.notify_waiters();
        Ok(OwnedProcessChild {
            child: Some(child),
            registration_id: Some(registration_id),
            shutdown: Some(receiver),
            reaper: self.reaper,
            counted_as_retained: false,
        })
    }
}

impl Drop for ProcessSpawnReservation {
    /// 未 spawn 时归还槽位；已启动却未移交时把 Child 放入 retained，总计数不变。
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.request_kill();
        let mut state = lock_reaper(self.reaper);
        debug_assert!(state.spawn_reservations > 0);
        state.spawn_reservations -= 1;
        if let Some(child) = self.child.take() {
            state.retained.push(child);
        } else {
            debug_assert!(state.owned_processes > 0);
            state.owned_processes -= 1;
        }
        drop(state);
        self.reaper.changed.notify_waiters();
    }
}

/// 在 u64 回绕后跳过仍 active 的 ID；总容量保证搜索必然终止。
fn allocate_registration_id(state: &mut ProcessReaperState) -> u64 {
    loop {
        let registration_id = state.next_registration_id;
        state.next_registration_id = state.next_registration_id.wrapping_add(1);
        if !state.active.contains_key(&registration_id) {
            return registration_id;
        }
    }
}

/// 在批量回收 future 被取消时把尚未轮到的句柄全部交还 owner。
struct RetainedChildBatch {
    /// 本批待回收的子进程。
    children: Vec<Child>,
    /// 进程级回收器。
    reaper: &'static ProcessReaper,
}

impl Drop for RetainedChildBatch {
    /// 批量回收被取消时，把尚未处理的 Child 全部放回进程级登记表。
    fn drop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        lock_reaper(self.reaper).retained.append(&mut self.children);
        self.reaper.changed.notify_waiters();
    }
}

/// 单个外部命令的所有权守卫；显式失败和 future 取消都不会 detach 子进程。
pub(super) struct OwnedProcessChild {
    /// 拥有的子进程；移交后为 None。
    child: Option<Child>,
    /// 活动登记 ID。
    registration_id: Option<u64>,
    /// 关闭信号接收端。
    shutdown: Option<watch::Receiver<bool>>,
    /// 进程级回收器。
    reaper: &'static ProcessReaper,
    /// 已计入待回收数量，避免重复计数。
    counted_as_retained: bool,
}

impl OwnedProcessChild {
    /// 先预留有界槽位，再在进程级 mutex 之外启动并接管子进程。
    pub(super) fn spawn(command: &mut Command) -> Result<Self, OwnedProcessError> {
        Self::spawn_in(command, process_reaper())
    }

    /// 在指定 owner 中执行预留、锁外 spawn 与原子移交，供生产和隔离回归共用。
    fn spawn_in(
        command: &mut Command,
        reaper: &'static ProcessReaper,
    ) -> Result<Self, OwnedProcessError> {
        let reservation = ProcessSpawnReservation::reserve(reaper)?;
        reservation.spawn_command(command)
    }

    /// 接管从 retained 批次取出但仍占用原总槽位的子进程。
    fn new_retained(child: Child, reaper: &'static ProcessReaper) -> Self {
        Self {
            child: Some(child),
            registration_id: None,
            shutdown: None,
            reaper,
            counted_as_retained: true,
        }
    }

    /// 返回仍由守卫持有的子进程。
    pub(super) fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("子进程必须仍由 owner 持有")
    }

    /// 返回进程级关闭信号，调用方必须与正常 wait/timeout 一并轮询。
    pub(super) fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown
            .as_ref()
            .expect("主动命令必须登记关闭信号")
            .clone()
    }

    /// 从 active 集合移除已经确认终态或已转交 retained 的 owner。
    fn deregister(&mut self) {
        let registration_id = self.registration_id.take();
        let retained = std::mem::take(&mut self.counted_as_retained);
        if registration_id.is_some() || retained {
            let mut state = lock_reaper(self.reaper);
            if let Some(registration_id) = registration_id {
                state.active.remove(&registration_id);
            }
            debug_assert!(state.owned_processes > 0);
            state.owned_processes -= 1;
            drop(state);
            self.reaper.changed.notify_waiters();
        }
        self.shutdown.take();
    }

    /// 清除已经由 wait 确认终态的 Child 与 active 登记。
    fn finish_reaped(&mut self) {
        self.child.take();
        self.deregister();
    }

    /// 把未确认终态的句柄转交进程级 owner。
    fn retain(&mut self) {
        let child = self.child.take();
        let registration_id = self.registration_id.take();
        let retained = std::mem::take(&mut self.counted_as_retained);
        self.shutdown.take();
        if let Some(child) = child {
            debug_assert!(registration_id.is_some() || retained);
            let mut state = lock_reaper(self.reaper);
            state.retained.push(child);
            if let Some(registration_id) = registration_id {
                state.active.remove(&registration_id);
            }
            drop(state);
            self.reaper.changed.notify_waiters();
        } else if registration_id.is_some() || retained {
            self.registration_id = registration_id;
            self.counted_as_retained = retained;
            self.deregister();
        }
    }

    /// 发送终止请求并只等待到调用方给出的共同截止；失败时立即保留句柄。
    pub(super) async fn terminate_and_reap_until(
        &mut self,
        deadline: tokio::time::Instant,
    ) -> Result<(), OwnedProcessError> {
        let already_exited = match self.child_mut().try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(_) => {
                self.retain();
                return Err(OwnedProcessError::Io);
            }
        };
        if already_exited {
            self.finish_reaped();
            return Ok(());
        }

        if self.child_mut().start_kill().is_err() {
            match self.child_mut().try_wait() {
                Ok(Some(_)) => {
                    self.finish_reaped();
                    return Ok(());
                }
                Ok(None) | Err(_) => {
                    self.retain();
                    return Err(OwnedProcessError::Io);
                }
            }
        }

        let waited = tokio::time::timeout_at(deadline, self.child_mut().wait()).await;
        self.finish_reap_wait(waited)
    }

    /// 按实际等待回执转移句柄归属；即时终态、I/O失败和确定超时沿用同一生产分支。
    fn finish_reap_wait(
        &mut self,
        waited: Result<std::io::Result<std::process::ExitStatus>, tokio::time::error::Elapsed>,
    ) -> Result<(), OwnedProcessError> {
        match waited {
            Ok(Ok(_)) => {
                self.finish_reaped();
                Ok(())
            }
            Ok(Err(_)) => {
                self.retain();
                Err(OwnedProcessError::Io)
            }
            Err(_) => {
                self.retain();
                Err(OwnedProcessError::ReapTimedOut)
            }
        }
    }
}

impl Drop for OwnedProcessChild {
    /// 执行子进程 owner 的同步取消兜底。
    /// 取消无法异步等待；先 kill，再在固定短预算内轮询，仍未终态则移交全局 owner。
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            self.deregister();
            return;
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.deregister();
                return;
            }
            Ok(None) => {}
            Err(_) => {
                self.child = Some(child);
                self.retain();
                return;
            }
        }
        if child.start_kill().is_err() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.deregister();
                    return;
                }
                Ok(None) | Err(_) => {
                    self.child = Some(child);
                    self.retain();
                    return;
                }
            }
        }
        let deadline = Instant::now() + CANCEL_REAP_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.deregister();
                    return;
                }
                Err(_) => {
                    self.child = Some(child);
                    self.retain();
                    return;
                }
                Ok(None) if Instant::now() >= deadline => {
                    self.child = Some(child);
                    self.retain();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }
}

/// 后续命令在自己的共同截止内优先回收上一轮遗留进程，并再次检查关闭门禁。
pub(super) async fn prepare_process_spawn(
    deadline: tokio::time::Instant,
) -> Result<(), OwnedProcessError> {
    if lock_process_reaper().shutting_down {
        return Err(OwnedProcessError::ShuttingDown);
    }
    reap_retained_processes_until_in(process_reaper(), deadline).await?;
    if lock_process_reaper().shutting_down {
        Err(OwnedProcessError::ShuttingDown)
    } else {
        Ok(())
    }
}

/// 返回首次应用退出建立的共同截止，主动命令收到关闭信号后不得自行重置预算。
pub(super) fn process_shutdown_deadline() -> Option<tokio::time::Instant> {
    lock_process_reaper().shutdown_deadline
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - process_reaper_shutdown.rs：退出等待、保留进程回收与 reaper 关闭流程，以及本模块单元测试
include!("process_reaper_shutdown.rs");
