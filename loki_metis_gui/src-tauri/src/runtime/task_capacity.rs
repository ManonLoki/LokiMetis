//! 为进程内后台任务提供跨 owner 的硬容量与全生命周期槽位。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// active、关闭批次、异常转交与 spawn 预约共用的进程级任务硬上限。
pub(super) const BACKGROUND_TASK_CAPACITY: usize = 32;
/// 后台任务容量耗尽时返回的稳定背压错误。
pub(super) const BACKGROUND_TASK_CAPACITY_ERROR: &str = "background-task-owner-capacity-exceeded";

/// 所有 owner 与 retained 表共享的进程级容量计数器。
static BACKGROUND_TASK_CAPACITY_OWNER: OnceLock<Arc<BackgroundTaskCapacity>> = OnceLock::new();

/// 保存当前进程仍被稳定拥有的后台任务槽位数。
#[derive(Default)]
pub(super) struct BackgroundTaskCapacity {
    pub(super) owned: AtomicUsize,
}

/// 从 spawn 前预约到任务 future/闭包真实返回全程随执行体移动的容量槽位。
pub(super) struct BackgroundTaskSlot {
    capacity: Arc<BackgroundTaskCapacity>,
}

impl BackgroundTaskSlot {
    /// 使用 CAS 在硬上限内预约一个槽位，失败时不得调用任务构造器。
    pub(super) fn reserve(capacity: &Arc<BackgroundTaskCapacity>) -> Result<Self, &'static str> {
        capacity
            .owned
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |owned| {
                (owned < BACKGROUND_TASK_CAPACITY).then_some(owned + 1)
            })
            .map_err(|_| BACKGROUND_TASK_CAPACITY_ERROR)?;
        Ok(Self {
            capacity: Arc::clone(capacity),
        })
    }
}

impl Drop for BackgroundTaskSlot {
    /// 只在任务执行体真实终态或 spawn 预约失败展开时归还全生命周期槽位。
    fn drop(&mut self) {
        let previous = self.capacity.owned.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "background task slot accounting underflow");
    }
}

/// 返回所有应用 owner 共享的进程级任务容量。
pub(super) fn process_background_task_capacity() -> Arc<BackgroundTaskCapacity> {
    Arc::clone(
        BACKGROUND_TASK_CAPACITY_OWNER.get_or_init(|| Arc::new(BackgroundTaskCapacity::default())),
    )
}
