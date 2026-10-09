//! 今日 All 快照的日界、来源和质量资格；宿主只提供同一次捕获的时间与聚合。

use crate::{
    Completeness, Confidence, LocalIndexState, LocalRecordsSummary, LocalUsageWindow, TimeStandard,
    civil_date_for_timestamp, day_start_epoch_ms, next_day_start_epoch_ms, stable_id,
};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

/// 民用日身份使用规则名与实际边界，DST 偏移改变不另建同日身份。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    /// 本地民用日期。
    pub date: String,
    /// IANA 规则名或系统固定时区规则。
    pub zone: String,
    /// 当地午夜包含端点。
    pub start_ms: i64,
    /// 下一午夜不包含端点，可能相距 23 或 25 小时。
    pub end_ms: i64,
}
impl UsageDay {
    /// 从一次捕获的时间与时区解析真实日历边界，无效时间失败关闭。
    pub fn capture(now_ms: i64, zone: &TimeZone) -> Option<Self> {
        let date = civil_date_for_timestamp(now_ms, &TimeStandard::Local, zone)?;
        Some(Self {
            date: date.to_string(),
            zone: zone
                .iana_name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{zone:?}")),
            start_ms: day_start_epoch_ms(date, &TimeStandard::Local, zone)?,
            end_ms: next_day_start_epoch_ms(now_ms, &TimeStandard::Local, zone)?,
        })
    }
    /// 确定性摘要不依赖进程随机哈希，也不把本机路径写入通知 ID。
    pub fn key(&self) -> String {
        stable_id(
            "day",
            &format!(
                "{}|{}|{}|{}",
                self.date, self.zone, self.start_ms, self.end_ms
            ),
        )
    }
    /// 拒绝午夜或时区切换前的迟到结果。
    pub fn contains(&self, now_ms: i64) -> bool {
        self.start_ms <= now_ms && now_ms < self.end_ms
    }
}

/// 不把未扫描、无记录、错误和可信零值混为同一状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodayAvailability {
    /// 有可解释索引及聚合。
    Available,
    /// 未扫描或没有可解释记录。
    NoRecords,
    /// 已启用源读取失败或没有启用源。
    Unavailable,
}

/// 应用所有消费者共享的不可变今日总量；前台筛选不改变此 All 口径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodayUsageSnapshot {
    /// 当前民用日与时区规则。
    pub day: UsageDay,
    /// 已规范化的来源集合与索引代次摘要。
    pub source: String,
    /// canonical/WorkBuddy 合并后的整数总量。
    pub total_tokens: u64,
    /// 同一次查询捕获的观测时间。
    pub observed_ms: i64,
    /// 可用与缺失分开表达。
    pub availability: TodayAvailability,
    /// 参与来源的完整度。
    pub completeness: Completeness,
    /// 聚合的最低置信度。
    pub confidence: Confidence,
}
impl TodayUsageSnapshot {
    /// 复用真实窗口事实，WorkBuddy 失败或任一参与源不完整均不能触发通知。
    pub fn from_records(
        day: UsageDay,
        source: String,
        records: &LocalRecordsSummary,
        observed_ms: i64,
        all_sources_ready: bool,
        source_failed: bool,
    ) -> Option<Self> {
        let fact = &records
            .windows
            .iter()
            .find(|w| w.window == LocalUsageWindow::Today)?
            .fact;
        Some(Self {
            day,
            source,
            total_tokens: fact.value.tokens.total_tokens,
            observed_ms,
            availability: if source_failed {
                TodayAvailability::Unavailable
            } else if !all_sources_ready
                || !matches!(
                    records.index_state,
                    LocalIndexState::Ready | LocalIndexState::ReadyNoCalls
                )
            {
                TodayAvailability::NoRecords
            } else {
                TodayAvailability::Available
            },
            completeness: fact.completeness,
            confidence: fact.confidence,
        })
    }
    /// 当前日、来源、新鲜度与完整精确质量同时成立才允许推进业务高水位。
    pub fn eligible(&self, now_ms: i64, current_day: &UsageDay, source: &str) -> bool {
        &self.day == current_day
            && self.day.contains(now_ms)
            && self.source == source
            && (0..=90_000).contains(&now_ms.saturating_sub(self.observed_ms))
            && self.availability == TodayAvailability::Available
            && self.completeness == Completeness::Complete
            && self.confidence == Confidence::Exact
    }
}

#[cfg(test)]
mod tests;
