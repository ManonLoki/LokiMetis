//! 超过单次 WSL 命令截止时间的 Windows child 进程级 owner。

use std::{
    process::Child,
    sync::{Arc, Condvar, Mutex, OnceLock},
    thread,
    time::Instant,
};

use super::super::thread_owner::RetainedThreadOwner;
use super::WSL_COMMAND_POLL;

/// WSL child reaper 的共享句柄表和唯一运行标志。
#[derive(Default)]
struct RetainedWslChildState {
    children: Vec<Child>,
    reaper_running: bool,
}

/// 在原命令结束后继续 kill/reap WSL 子进程，并持有 reaper 自身线程。
struct RetainedWslChildOwner {
    shared: Arc<(Mutex<RetainedWslChildState>, Condvar)>,
    reapers: RetainedThreadOwner,
}

impl RetainedWslChildOwner {
    /// 创建空的 WSL child owner。
    fn new() -> Self {
        Self {
            shared: Arc::new((Mutex::new(RetainedWslChildState::default()), Condvar::new())),
            reapers: RetainedThreadOwner::new("wsl-child"),
        }
    }
}

/// kill/reap 超过调用方截止时间的 child 由此静态 owner 保持真实所有权。
static WSL_RETAINED_CHILDREN: OnceLock<RetainedWslChildOwner> = OnceLock::new();

/// 返回跨命令共享的 WSL child owner。
fn retained_wsl_child_owner() -> &'static RetainedWslChildOwner {
    WSL_RETAINED_CHILDREN.get_or_init(RetainedWslChildOwner::new)
}

/// 保留未达终态子进程并立即启动后台 reaper，不依赖下一条 WSL 命令。
pub(super) fn retain_wsl_child(child: Child) {
    let owner = retained_wsl_child_owner();
    let should_start = {
        let (state, wake) = &*owner.shared;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.children.push(child);
        let should_start = !state.reaper_running;
        if should_start {
            state.reaper_running = true;
        }
        wake.notify_all();
        should_start
    };
    if should_start {
        start_wsl_child_reaper(owner);
    }
}

/// 非阻塞维护 child owner；后台 reaper 已负责空闲期终态确认。
pub(super) fn reap_retained_wsl_children() {
    let owner = retained_wsl_child_owner();
    owner.reapers.reap_finished();
    let should_start = {
        let mut state = owner
            .shared
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let should_start = !state.children.is_empty() && !state.reaper_running;
        if should_start {
            state.reaper_running = true;
        }
        should_start
    };
    if should_start {
        start_wsl_child_reaper(owner);
    }
}

/// 启动一个 child reaper，并把 reaper 线程本身交给通用线程 owner。
fn start_wsl_child_reaper(owner: &'static RetainedWslChildOwner) {
    let shared = Arc::clone(&owner.shared);
    match thread::Builder::new()
        .name("loki-metis-wsl-child-reaper".to_owned())
        .spawn(move || wsl_child_reaper_loop(&shared))
    {
        Ok(reaper) => owner.reapers.retain(reaper),
        Err(error) => {
            owner
                .shared
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .reaper_running = false;
            tracing::error!(%error, "failed to start retained WSL child reaper");
        }
    }
}

/// 持续 kill/try_wait 已保留 child，直到表为空才结束本轮 reaper。
fn wsl_child_reaper_loop(shared: &Arc<(Mutex<RetainedWslChildState>, Condvar)>) {
    loop {
        let (state, wake) = &**shared;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut index = 0;
        while index < state.children.len() {
            let child = &mut state.children[index];
            let _ = child.kill();
            match child.try_wait() {
                Ok(Some(_)) => {
                    state.children.swap_remove(index);
                }
                Ok(None) => index += 1,
                Err(error) => {
                    tracing::error!(
                        process_id = child.id(),
                        %error,
                        "failed to query a retained WSL child; preserving its process handle"
                    );
                    index += 1;
                }
            }
        }
        if state.children.is_empty() {
            state.reaper_running = false;
            wake.notify_all();
            return;
        }
        drop(wake.wait_timeout(state, WSL_COMMAND_POLL));
    }
}

/// 应用 shutdown 在共同截止前等待后台 child reaper；超时仍由静态 owner 保存句柄。
pub(super) fn shutdown_retained_wsl_children_until(deadline: Instant) {
    reap_retained_wsl_children();
    let owner = retained_wsl_child_owner();
    loop {
        let (state, wake) = &*owner.shared;
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.children.is_empty() && !state.reaper_running {
            drop(state);
            let _ = owner.reapers.wait_until(deadline);
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline.saturating_duration_since(now);
        drop(wake.wait_timeout(state, WSL_COMMAND_POLL.min(remaining)));
    }
}

/// 返回仍由 child owner 持有的进程 ID，供 Windows 生命周期回归使用。
#[cfg(test)]
pub(super) fn retained_wsl_child_ids() -> Vec<u32> {
    retained_wsl_child_owner()
        .shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .children
        .iter()
        .map(Child::id)
        .collect()
}
