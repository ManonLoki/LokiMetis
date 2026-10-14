//! 统计页与图表页的结果缓存：同一民用小时内所选 Agent 的索引文件与覆盖都未变时，
//! 直接复用上次页面，只刷新观测时间，不再重读窗口内全部调用并重新分组。
//!
//! 页面可见时每 10 秒刷新一次，而索引只会被本进程扫描事务写入；判据沿用今日摘要缓存的
//! 索引文件戳与覆盖报告（外部改动同样改变文件戳），读取前后键一致才保存。键额外包含
//! 民用小时，因为小时图会把当前小时标记为进行中。缓存只保留一份，即当前正在查看的页面。

use std::sync::Mutex;

use loki_metris_core::{
    LocalUsageWindow, UsageChartDimension, UsageChartPage, UsageDimension, UsageStatisticsPage,
};

use super::today_cache::IndexKey;

/// 可缓存的页面查询参数。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PageQuery {
    /// 单 Agent 统计页。
    Statistics(LocalUsageWindow, UsageDimension),
    /// 单 Agent 图表页。
    Chart(LocalUsageWindow, UsageChartDimension),
}

/// 缓存键：索引侧状态（含民用日）、民用小时与页面参数全部相同才命中。
#[derive(Clone, PartialEq, Eq)]
pub(super) struct PageKey {
    /// 所选 Agent 的索引文件戳、覆盖报告与民用日。
    pub(super) index: IndexKey,
    /// 观测时刻所在的本地民用小时。
    pub(super) hour: i8,
    /// 页面种类与窗口、维度。
    pub(super) query: PageQuery,
}

/// 缓存的页面结果。
#[derive(Clone)]
pub(super) enum CachedPage {
    /// 统计页。
    Statistics(UsageStatisticsPage),
    /// 图表页。
    Chart(UsageChartPage),
}

impl CachedPage {
    /// 命中时页面数值沿用旧结果，但观测时间必须是本轮时刻。
    pub(super) fn restamp(&mut self, now: i64) {
        match self {
            Self::Statistics(page) => {
                page.observed_at_epoch_ms = now;
                page.fact.observed_at_epoch_ms = now;
            }
            Self::Chart(page) => {
                page.observed_at_epoch_ms = now;
                page.fact.observed_at_epoch_ms = now;
            }
        }
    }
}

/// 只保留最近一份页面结果，新页面直接替换旧页面。
#[derive(Default)]
pub(super) struct PageCache(
    Mutex<Option<(PageKey, CachedPage)>>,
    /// 测试用：命中次数，证明命中时没有重新读取并计算。
    #[cfg(test)] std::sync::atomic::AtomicUsize,
);

impl PageCache {
    /// 键完全一致才返回缓存副本。
    pub(super) fn lookup(&self, key: &PageKey) -> Option<CachedPage> {
        let entry = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let hit = entry.as_ref().filter(|(cached, _)| cached == key).map(|(_, page)| page.clone());
        #[cfg(test)]
        if hit.is_some() {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        hit
    }

    /// 测试用：当前缓存条目的键。
    #[cfg(test)]
    pub(super) fn cached_key(&self) -> Option<PageKey> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(|(key, _)| key.clone())
    }

    /// 测试用：累计命中次数。
    #[cfg(test)]
    pub(super) fn hits(&self) -> usize {
        self.1.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 以读取前取得的键保存页面，调用方负责确认读取期间索引未变化。
    pub(super) fn store(&self, key: PageKey, page: CachedPage) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((key, page));
    }
}
