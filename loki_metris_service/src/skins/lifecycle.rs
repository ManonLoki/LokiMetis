//! 监视任务的停止预算、页面监视生命周期与退出回收。

use super::*;

/// 强制中止 watcher 后留给 Tokio 确认 future 已到终态的预算；端点清理继续受同一总截止约束。
const WATCH_ABORT_REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// 描述 watcher 合作停止、总截止和 abort 终态确认的预算。
#[derive(Clone, Copy)]
pub(super) struct WatchStopBudget {
    /// 合作停止的等待时长。
    pub(super) cooperative: Duration,
    /// 总截止时长。
    pub(super) total: Duration,
    /// abort 后确认终态的时长。
    pub(super) abort_reap: Duration,
}

impl WatchStopBudget {
    /// 从产品常量构造生产环境预算。
    pub(super) fn production() -> Self {
        Self {
            cooperative: WATCH_STOP_TIMEOUT,
            // 保留原有合作停止与 CDP 清理预算，但二者现在共享一个不可重置的总截止。
            total: WATCH_STOP_TIMEOUT.saturating_add(CDP_CLEANUP_TIMEOUT),
            abort_reap: WATCH_ABORT_REAP_TIMEOUT,
        }
    }

    /// 由同一开始时间派生不可重置的合作截止与最终截止。
    pub(super) fn deadlines(
        self,
        started_at: tokio::time::Instant,
    ) -> (tokio::time::Instant, tokio::time::Instant) {
        let final_deadline = started_at + self.total;
        let cooperative_deadline = started_at + self.cooperative.min(self.total);
        (cooperative_deadline, final_deadline)
    }
}

/// 区分端点清理已返回（含显式错误）与外层共同截止耗尽。
enum EndpointCleanupOutcome {
    /// 清理已返回，附结果。
    Completed(Result<usize, AppError>),
    /// 共同截止耗尽。
    DeadlineElapsed,
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - lifecycle_watchers.rs：取得 watcher 回收器短临界区；即使测试污染锁，也不能遗失真实任务句柄。
// - lifecycle_reaping.rs：监视任务回收、服务释放、提示词路径校验，以及皮肤 ID、删除选择与导入 token 校验。
include!("lifecycle_watchers.rs");
include!("lifecycle_reaping.rs");
