//! 统一持有 Unix launcher 子进程，确保超时、取消和退出路径不会丢失回收责任。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::process::{Child, Command};
use tokio::sync::watch;

/// future 被取消时允许同步确认 SIGKILL 终态的最大预算。
const CANCEL_REAP_TIMEOUT: Duration = Duration::from_millis(250);
/// 应用退出用于回收 launcher 的总预算；重复调用不会重置该截止。
pub(super) const PROCESS_REAPER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

/// 子进程 owner 在启动、等待或最终回收阶段的稳定错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OwnedProcessError {
    Spawn,
    Io,
    ReapTimedOut,
    ShuttingDown,
}

/// 尚未确认终态的子进程登记表；锁内只移动句柄，不执行等待。
#[derive(Default)]
struct ProcessReaperState {
    retained: Vec<Child>,
    active: HashMap<u64, watch::Sender<bool>>,
    next_registration_id: u64,
    shutting_down: bool,
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// 取得进程级登记表。
fn process_reaper() -> &'static Mutex<ProcessReaperState> {
    static REAPER: OnceLock<Mutex<ProcessReaperState>> = OnceLock::new();
    REAPER.get_or_init(|| Mutex::new(ProcessReaperState::default()))
}

/// 锁中毒不能使真实子进程句柄遗失。
fn lock_process_reaper() -> std::sync::MutexGuard<'static, ProcessReaperState> {
    process_reaper()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 在批量回收 future 被取消时把尚未轮到的句柄全部交还 owner。
struct RetainedChildBatch {
    children: Vec<Child>,
}

impl Drop for RetainedChildBatch {
    /// 批量回收被取消时，把尚未处理的 Child 全部放回进程级登记表。
    fn drop(&mut self) {
        if self.children.is_empty() {
            return;
        }
        lock_process_reaper().retained.append(&mut self.children);
    }
}

/// 单个外部命令的所有权守卫；显式失败和 future 取消都不会 detach 子进程。
pub(super) struct OwnedProcessChild {
    child: Option<Child>,
    registration_id: Option<u64>,
    shutdown: Option<watch::Receiver<bool>>,
}

impl OwnedProcessChild {
    /// 在进程级关闭门禁之外启动并接管子进程。
    pub(super) fn spawn(command: &mut Command) -> Result<Self, OwnedProcessError> {
        // 同一短锁覆盖关闭复核、同步 spawn 和 active 登记，消除 shutdown TOCTOU。
        let mut state = lock_process_reaper();
        if state.shutting_down {
            return Err(OwnedProcessError::ShuttingDown);
        }
        command.kill_on_drop(true);
        let child = command.spawn().map_err(|_| OwnedProcessError::Spawn)?;
        let registration_id = state.next_registration_id;
        state.next_registration_id = state.next_registration_id.wrapping_add(1);
        let (shutdown, receiver) = watch::channel(false);
        state.active.insert(registration_id, shutdown);
        Ok(Self {
            child: Some(child),
            registration_id: Some(registration_id),
            shutdown: Some(receiver),
        })
    }

    /// 接管已有子进程，供回收器与定向测试使用。
    fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            registration_id: None,
            shutdown: None,
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
        if let Some(registration_id) = self.registration_id.take() {
            lock_process_reaper().active.remove(&registration_id);
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
        self.shutdown.take();
        if child.is_some() || registration_id.is_some() {
            let mut state = lock_process_reaper();
            if let Some(child) = child {
                state.retained.push(child);
            }
            if let Some(registration_id) = registration_id {
                state.active.remove(&registration_id);
            }
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
    reap_retained_processes_until(deadline).await?;
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

/// 逐项回收已保留子进程；当前与未轮到的句柄均由取消守卫覆盖。
async fn reap_retained_processes_until(
    deadline: tokio::time::Instant,
) -> Result<usize, OwnedProcessError> {
    let mut batch = RetainedChildBatch {
        children: {
            let mut state = lock_process_reaper();
            std::mem::take(&mut state.retained)
        },
    };
    let mut reaped = 0_usize;
    let mut first_error = None;
    while let Some(child) = batch.children.pop() {
        let mut owner = OwnedProcessChild::new(child);
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
    let (deadline, shutdown_signals) = begin_process_shutdown(timeout);
    for shutdown in shutdown_signals {
        let _ = shutdown.send(true);
    }

    let mut reaped = 0_usize;
    loop {
        match reap_retained_processes_until(deadline).await {
            Ok(count) => reaped = reaped.saturating_add(count),
            Err(OwnedProcessError::ReapTimedOut) => return Err(OwnedProcessError::ReapTimedOut),
            Err(_) => {}
        }
        let (active, retained) = {
            let state = lock_process_reaper();
            (state.active.len(), state.retained.len())
        };
        if active == 0 && retained == 0 {
            return Ok(reaped);
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(OwnedProcessError::ReapTimedOut);
        }
        tokio::time::sleep(Duration::from_millis(10).min(remaining)).await;
    }
}

/// 原子关闭 spawn 门禁、固定首次截止并取得全部 active owner 的取消发送端。
fn begin_process_shutdown(timeout: Duration) -> (tokio::time::Instant, Vec<watch::Sender<bool>>) {
    let now = tokio::time::Instant::now();
    let mut state = lock_process_reaper();
    state.shutting_down = true;
    let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
    let signals = state.active.values().cloned().collect::<Vec<_>>();
    (deadline, signals)
}

#[cfg(test)]
/// 返回仍由进程级 owner 持有的句柄数。
pub(super) fn retained_process_count() -> usize {
    lock_process_reaper().retained.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// 回收截止已经耗尽时必须保存原 Child，下一轮可继续等待到真实终态。
    #[tokio::test]
    async fn reap_timeout_retains_child_until_a_later_attempt() {
        let cleanup_deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        let _ = reap_retained_processes_until(cleanup_deadline).await;

        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut owner = OwnedProcessChild::spawn(&mut command).expect("测试子进程应可启动");
        let expired = tokio::time::Instant::now()
            .checked_sub(Duration::from_millis(1))
            .expect("测试截止可构造");
        assert_eq!(
            owner.terminate_and_reap_until(expired).await,
            Err(OwnedProcessError::ReapTimedOut)
        );
        assert!(retained_process_count() >= 1);

        prepare_process_spawn(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("下一次命令必须先回收已 kill 的子进程");
        assert_eq!(retained_process_count(), 0);
    }

    /// active owner 必须在同步 spawn 返回前登记，并在确认终态后原子移除。
    #[tokio::test]
    async fn active_child_is_registered_for_shutdown_until_reaped() {
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut owner = OwnedProcessChild::spawn(&mut command).expect("测试子进程应可启动");
        let registration_id = owner
            .registration_id
            .expect("主动子进程必须保存自己的登记标识");
        assert!(
            lock_process_reaper().active.contains_key(&registration_id),
            "本测试子进程必须在同步 spawn 返回前完成登记"
        );
        owner
            .terminate_and_reap_until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("测试子进程必须可终止并回收");
        assert!(
            !lock_process_reaper().active.contains_key(&registration_id),
            "本测试子进程确认终态后必须移除自己的登记"
        );
    }

    /// shutdown 门禁、spawn 与 active 登记必须处于同一临界区，禁止检查后启动竞态。
    #[test]
    fn spawn_registration_has_no_shutdown_toctou() {
        let source = include_str!("process_reaper.rs");
        let spawn = source
            .split_once("pub(super) fn spawn")
            .expect("spawn helper must exist")
            .1
            .split_once("/// 接管已有子进程")
            .expect("spawn helper boundary must exist")
            .0;
        let lock = spawn
            .find("let mut state = lock_process_reaper()")
            .expect("spawn must acquire process owner lock");
        let child = spawn
            .find("command.spawn()")
            .expect("spawn must create child under the owner lock");
        let registration = spawn
            .find("state.active.insert")
            .expect("spawn must register the active owner before unlocking");
        assert!(lock < child && child < registration);
    }

    /// shutdown 实现必须同时通知 active owner，并在循环中等待 active 与 retained 都为空。
    #[test]
    fn shutdown_contract_covers_active_and_retained_owners() {
        let source = include_str!("process_reaper.rs");
        let shutdown = source
            .split_once("pub(super) async fn shutdown_process_reaper")
            .expect("shutdown helper must exist")
            .1
            .split_once("/// 原子关闭 spawn 门禁")
            .expect("shutdown helper boundary must exist")
            .0;
        assert!(shutdown.contains("for shutdown in shutdown_signals"));
        assert!(shutdown.contains("shutdown.send(true)"));
        assert!(shutdown.contains("state.active.len()"));
        assert!(shutdown.contains("state.retained.len()"));
        assert!(shutdown.contains("remaining.is_zero()"));
    }
}
