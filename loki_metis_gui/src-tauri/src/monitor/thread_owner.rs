//! 超过调用方截止时间的阻塞线程句柄由进程级 owner 在后台继续确认终态。

#[cfg(any(target_os = "windows", test))]
use std::time::Instant;
use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

/// owner 检查阻塞线程终态的轮询间隔。
const RETAINED_THREAD_POLL: Duration = Duration::from_millis(10);

/// 仍待回收的工作线程，以及当前是否已有唯一 reaper 在轮询。
#[derive(Default)]
struct RetainedThreadState {
    tasks: Vec<thread::JoinHandle<()>>,
    reaper_running: bool,
}

/// 在原调用期限耗尽后继续持有并回收 `std::thread` 句柄。
pub(super) struct RetainedThreadOwner {
    name: &'static str,
    shared: Arc<(Mutex<RetainedThreadState>, Condvar)>,
    reapers: Mutex<Vec<thread::JoinHandle<()>>>,
}

impl RetainedThreadOwner {
    /// 为一类具名阻塞任务创建进程级 owner。
    pub(super) fn new(name: &'static str) -> Self {
        Self {
            name,
            shared: Arc::new((Mutex::new(RetainedThreadState::default()), Condvar::new())),
            reapers: Mutex::new(Vec::new()),
        }
    }

    /// 接管一个尚未结束的线程，并确保空闲期仍有 reaper 最终 join 它。
    pub(super) fn retain(&self, task: thread::JoinHandle<()>) {
        self.reap_finished_reapers();
        let should_start = {
            let (state, wake) = &*self.shared;
            let mut state = lock_unpoisoned(state);
            state.tasks.push(task);
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

    /// 在给定绝对截止时间前等待全部保留线程达到终态。
    #[cfg(any(target_os = "windows", test))]
    pub(super) fn wait_until(&self, deadline: Instant) -> bool {
        self.ensure_reaper();
        loop {
            self.reap_finished_reapers();
            let (state, wake) = &*self.shared;
            let state = lock_unpoisoned(state);
            if state.tasks.is_empty() && !state.reaper_running {
                drop(state);
                self.reap_finished_reapers();
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline.saturating_duration_since(now);
            drop(wake.wait_timeout(state, RETAINED_THREAD_POLL.min(remaining)));
        }
    }

    /// 非阻塞回收已经结束的 reaper，并在保留任务失去 reaper 时重新启动。
    pub(super) fn reap_finished(&self) {
        self.reap_finished_reapers();
        self.ensure_reaper();
    }

    /// 返回仍由 owner 持有、等待真实终态的工作线程数。
    #[cfg(test)]
    pub(super) fn retained_count(&self) -> usize {
        lock_unpoisoned(&self.shared.0).tasks.len()
    }

    /// 当表中仍有任务但 reaper 不在运行时启动新的具名 reaper。
    fn ensure_reaper(&self) {
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

    /// 启动只做终态轮询和 join 的 reaper；启动失败仍由静态登记表保留原句柄。
    fn start_reaper(&self) {
        let shared = Arc::clone(&self.shared);
        let thread_name = format!("loki-metis-{}-reaper", self.name);
        match thread::Builder::new()
            .name(thread_name)
            .spawn(move || retained_thread_reaper_loop(&shared))
        {
            Ok(reaper) => lock_unpoisoned(&self.reapers).push(reaper),
            Err(error) => {
                lock_unpoisoned(&self.shared.0).reaper_running = false;
                tracing::error!(owner = self.name, %error, "failed to start retained thread reaper");
            }
        }
    }

    /// join 已结束的 reaper 线程；仍在运行的 reaper 句柄继续由 owner 保存。
    fn reap_finished_reapers(&self) {
        let mut reapers = lock_unpoisoned(&self.reapers);
        let mut index = 0;
        while index < reapers.len() {
            if reapers[index].is_finished() {
                let reaper = reapers.swap_remove(index);
                let _ = reaper.join();
            } else {
                index += 1;
            }
        }
    }
}

/// 轮询所有保留线程；只在 `is_finished` 后 join，避免 reaper 自身永久阻塞。
fn retained_thread_reaper_loop(shared: &Arc<(Mutex<RetainedThreadState>, Condvar)>) {
    loop {
        let (state, wake) = &**shared;
        let mut state = lock_unpoisoned(state);
        let mut index = 0;
        while index < state.tasks.len() {
            if state.tasks[index].is_finished() {
                let task = state.tasks.swap_remove(index);
                let _ = task.join();
            } else {
                index += 1;
            }
        }
        if state.tasks.is_empty() {
            state.reaper_running = false;
            wake.notify_all();
            return;
        }
        drop(wake.wait_timeout(state, RETAINED_THREAD_POLL));
    }
}

/// 锁污染不应遗失仍在运行的线程句柄，因此统一恢复内部值。
fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// 测试共用一个 owner，避免并行用例把静态线程计数互相污染。
    fn test_owner() -> &'static RetainedThreadOwner {
        static OWNER: OnceLock<RetainedThreadOwner> = OnceLock::new();
        OWNER.get_or_init(|| RetainedThreadOwner::new("test-thread"))
    }

    /// 没有后续业务调用时，后台 reaper 也必须在空闲期 join 已完成线程。
    #[test]
    fn retained_thread_is_reaped_during_idle() {
        let owner = test_owner();
        assert!(owner.wait_until(Instant::now() + Duration::from_secs(1)));
        owner.retain(thread::spawn(|| thread::sleep(Duration::from_millis(20))));

        let deadline = Instant::now() + Duration::from_secs(1);
        while owner.retained_count() != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(owner.retained_count(), 0);
        assert!(owner.wait_until(deadline));
    }
}
