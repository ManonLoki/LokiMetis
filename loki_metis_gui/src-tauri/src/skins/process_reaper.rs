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
    Spawn,
    Io,
    ReapTimedOut,
    ShuttingDown,
    CapacityExceeded,
}

/// 尚未确认终态的子进程登记表；锁内只移动句柄，不执行等待。
#[derive(Default)]
struct ProcessReaperState {
    retained: Vec<Child>,
    active: HashMap<u64, watch::Sender<bool>>,
    spawn_reservations: usize,
    owned_processes: usize,
    next_registration_id: u64,
    shutting_down: bool,
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// 持有进程登记表与非阻塞变化通知，不在锁内运行进程操作。
struct ProcessReaper {
    state: Mutex<ProcessReaperState>,
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
    reaper: &'static ProcessReaper,
    child: Option<Child>,
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
    children: Vec<Child>,
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
    child: Option<Child>,
    registration_id: Option<u64>,
    shutdown: Option<watch::Receiver<bool>>,
    reaper: &'static ProcessReaper,
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

/// 等待进程级 shutdown；发送端意外消失也按关闭处理。
pub(super) async fn wait_for_process_shutdown(receiver: &mut watch::Receiver<bool>) {
    if *receiver.borrow_and_update() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow_and_update() {
            return;
        }
    }
}

/// 在指定 owner 中逐项回收 retained 子进程，批次移出期间仍占用原总槽位。
async fn reap_retained_processes_until_in(
    reaper: &'static ProcessReaper,
    deadline: tokio::time::Instant,
) -> Result<usize, OwnedProcessError> {
    let mut batch = RetainedChildBatch {
        children: {
            let mut state = lock_reaper(reaper);
            std::mem::take(&mut state.retained)
        },
        reaper,
    };
    let mut reaped = 0_usize;
    let mut first_error = None;
    while let Some(child) = batch.children.pop() {
        let mut owner = OwnedProcessChild::new_retained(child, reaper);
        match owner.terminate_and_reap_until(deadline).await {
            Ok(()) => reaped = reaped.saturating_add(1),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(reaped), Err)
}

/// 应用退出只建立一次总截止，并继续持有最终期限内尚未到终态的句柄。
pub(super) async fn shutdown_process_reaper(timeout: Duration) -> Result<usize, OwnedProcessError> {
    shutdown_process_reaper_in(process_reaper(), timeout).await
}

/// 在指定 owner 中关闭 spawn 门禁，并在首次绝对截止内等待全部槽位释放。
async fn shutdown_process_reaper_in(
    reaper: &'static ProcessReaper,
    timeout: Duration,
) -> Result<usize, OwnedProcessError> {
    let (deadline, shutdown_signals) = begin_process_shutdown_in(reaper, timeout);
    for shutdown in shutdown_signals {
        let _ = shutdown.send(true);
    }

    let mut reaped = 0_usize;
    loop {
        match reap_retained_processes_until_in(reaper, deadline).await {
            Ok(count) => reaped = reaped.saturating_add(count),
            Err(OwnedProcessError::ReapTimedOut) => return Err(OwnedProcessError::ReapTimedOut),
            Err(_) => {}
        }
        if lock_reaper(reaper).owned_processes == 0 {
            return Ok(reaped);
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(OwnedProcessError::ReapTimedOut);
        }
        tokio::select! {
            _ = reaper.changed.notified() => {}
            _ = tokio::time::sleep(PROCESS_REAPER_STATE_POLL.min(remaining)) => {}
        }
    }
}

/// 对指定 owner 原子关闭门禁，供生产 shutdown 与隔离竞态回归复用。
fn begin_process_shutdown_in(
    reaper: &'static ProcessReaper,
    timeout: Duration,
) -> (tokio::time::Instant, Vec<watch::Sender<bool>>) {
    let now = tokio::time::Instant::now();
    let mut state = lock_reaper(reaper);
    state.shutting_down = true;
    let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
    let signals = state.active.values().cloned().collect::<Vec<_>>();
    drop(state);
    reaper.changed.notify_waiters();
    (deadline, signals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// 为并行回归分配不污染生产单例的进程 owner。
    fn isolated_reaper() -> &'static ProcessReaper {
        Box::leak(Box::new(ProcessReaper::default()))
    }

    /// 构造不继承标准流的长时间测试子进程。
    fn sleeping_command() -> Command {
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// 回收截止已经耗尽时必须保存原 Child，下一轮可继续等待到真实终态。
    #[tokio::test]
    async fn reap_timeout_retains_child_until_a_later_attempt() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("测试子进程应可启动");
        let expired = tokio::time::Instant::now()
            .checked_sub(Duration::from_millis(1))
            .expect("测试截止可构造");
        assert_eq!(
            owner.terminate_and_reap_until(expired).await,
            Err(OwnedProcessError::ReapTimedOut)
        );
        {
            let state = lock_reaper(reaper);
            assert_eq!(state.retained.len(), 1);
            assert_eq!(state.owned_processes, 1);
        }

        reap_retained_processes_until_in(
            reaper,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .expect("下一次命令必须先回收已 kill 的子进程");
        let state = lock_reaper(reaper);
        assert!(state.retained.is_empty());
        assert_eq!(state.owned_processes, 0);
    }

    /// active owner 必须在同步 spawn 返回前登记，并在确认终态后原子移除。
    #[tokio::test]
    async fn active_child_is_registered_for_shutdown_until_reaped() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("测试子进程应可启动");
        let registration_id = owner
            .registration_id
            .expect("主动子进程必须保存自己的登记标识");
        {
            let state = lock_reaper(reaper);
            assert!(state.active.contains_key(&registration_id));
            assert_eq!(state.spawn_reservations, 0);
            assert_eq!(state.owned_processes, 1);
        }
        owner
            .terminate_and_reap_until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("测试子进程必须可终止并回收");
        let state = lock_reaper(reaper);
        assert!(!state.active.contains_key(&registration_id));
        assert_eq!(state.owned_processes, 0);
    }

    /// spawn 必须按 reservation、锁外启动、原子移交的顺序消除 shutdown TOCTOU。
    #[test]
    fn spawn_registration_has_no_shutdown_toctou() {
        let source = include_str!("process_reaper.rs");
        let spawn = source
            .split_once("fn spawn_command")
            .expect("spawn_command helper must exist")
            .1
            .split_once("/// 在 reservation 已持有 Child")
            .expect("spawn_command helper boundary must exist")
            .0;
        let child = spawn
            .find("command.spawn()")
            .expect("spawn must create the child");
        let adoption = spawn
            .find("self.finish_adoption()")
            .expect("spawn must transfer the child into the owner");
        assert!(child < adoption);
        assert!(spawn.contains("self.child = Some(command.spawn()"));
        assert!(!spawn.contains("lock_reaper("));
        assert!(!spawn.contains("lock_process_reaper("));
    }

    /// 登记 ID 回绕时必须跳过仍 active 的键，不得覆盖其 shutdown sender。
    #[test]
    fn registration_id_wrap_skips_active_entries() {
        let mut state = ProcessReaperState {
            next_registration_id: u64::MAX,
            ..ProcessReaperState::default()
        };
        let (max_sender, _) = watch::channel(false);
        let (zero_sender, _) = watch::channel(false);
        state.active.insert(u64::MAX, max_sender);
        state.active.insert(0, zero_sender);

        assert_eq!(allocate_registration_id(&mut state), 1);
        assert_eq!(state.next_registration_id, 2);
        assert!(state.active.contains_key(&u64::MAX) && state.active.contains_key(&0));
    }

    /// reservation 已持有 Child 后的 Drop 必须在转 retained 前锁外发布终止。
    #[tokio::test]
    async fn dropped_spawn_reservation_kills_before_retaining_child() {
        let reaper = isolated_reaper();
        let mut reservation = ProcessSpawnReservation::reserve(reaper).expect("reserve slot");
        let mut command = sleeping_command();
        command.kill_on_drop(true);
        reservation.child = Some(command.spawn().expect("spawn outside owner mutex"));

        drop(reservation);
        let child = {
            let mut state = lock_reaper(reaper);
            assert_eq!(state.spawn_reservations, 0);
            assert_eq!(state.owned_processes, 1);
            state.retained.pop().expect("drop must retain child")
        };
        let mut owner = OwnedProcessChild::new_retained(child, reaper);
        let status = tokio::time::timeout(Duration::from_secs(1), owner.child_mut().wait())
            .await
            .expect("drop must request termination before retention")
            .expect("retained child wait");
        assert!(!status.success());
        owner.finish_reaped();
        assert_eq!(lock_reaper(reaper).owned_processes, 0);
    }

    /// shutdown 必须等待在途 reservation，其锁外 spawn 结果只能转入 retained 再回收。
    #[tokio::test]
    async fn shutdown_waits_for_in_flight_spawn_reservation() {
        let reaper = isolated_reaper();
        let reservation = ProcessSpawnReservation::reserve(reaper).expect("reserve slot");
        let shutdown = tokio::spawn(shutdown_process_reaper_in(reaper, Duration::from_secs(2)));
        for _ in 0..100 {
            if lock_reaper(reaper).shutting_down {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(lock_reaper(reaper).shutting_down);

        let mut command = sleeping_command();
        command.kill_on_drop(true);
        let child = command.spawn().expect("spawn outside owner mutex");
        assert!(matches!(
            reservation.adopt(child),
            Err(OwnedProcessError::ShuttingDown)
        ));

        assert_eq!(shutdown.await.expect("shutdown task"), Ok(1));
        let state = lock_reaper(reaper);
        assert_eq!(state.spawn_reservations, 0);
        assert_eq!(state.owned_processes, 0);
        assert!(state.active.is_empty() && state.retained.is_empty());
    }

    /// active 与 reservation 共用严格容量，满额时稳定拒绝且 Drop 归还槽位。
    #[tokio::test]
    async fn process_owner_capacity_includes_spawn_reservations() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("active process slot");
        let reservations = (1..OWNED_PROCESS_CAPACITY)
            .map(|_| ProcessSpawnReservation::reserve(reaper).expect("capacity slot"))
            .collect::<Vec<_>>();
        assert!(matches!(
            ProcessSpawnReservation::reserve(reaper),
            Err(OwnedProcessError::CapacityExceeded)
        ));
        {
            let state = lock_reaper(reaper);
            assert_eq!(state.active.len(), 1);
            assert_eq!(state.spawn_reservations, OWNED_PROCESS_CAPACITY - 1);
            assert_eq!(state.owned_processes, OWNED_PROCESS_CAPACITY);
        }

        drop(reservations);
        owner
            .terminate_and_reap_until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("active process must be reaped");
        let state = lock_reaper(reaper);
        assert_eq!(state.spawn_reservations, 0);
        assert_eq!(state.owned_processes, 0);
    }

    /// 重复 shutdown 不得重置首次建立的绝对截止时间。
    #[test]
    fn repeated_shutdown_keeps_the_first_deadline() {
        let reaper = isolated_reaper();
        let (first, _) = begin_process_shutdown_in(reaper, Duration::from_millis(50));
        let (second, _) = begin_process_shutdown_in(reaper, Duration::from_secs(30));
        assert_eq!(first, second);
    }
}
