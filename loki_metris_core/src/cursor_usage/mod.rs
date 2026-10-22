//! Cursor 账户级事件：保留上游分项和缺失事实，不混入本机文件用量。

mod pages;
mod summary;
#[cfg(test)]
mod tests;

use jiff::tz::TimeZone;
use crate::{LocalUsageWindow, TimeStandard, dates_for_window, day_start_epoch_ms};

/// 每页最多读取一百条，整次查询最多一万条；超限不发布部分结果。
pub const PAGE_SIZE: usize = 100;
/// 单次查询的事件上限。
pub const MAX_EVENTS: usize = 10_000;
/// 成功与失败查询至多缓存十秒，与看板自动读取周期一致。
pub const REFRESH_INTERVAL_MS: i64 = 10_000;

/// 不携带正文、身份或凭据的稳定读取错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CursorUsageError {
    /// 本机没有可读取的登录材料。
    #[error("cursor_usage_credentials")]
    Credentials,
    /// 上游拒绝当前登录。
    #[error("cursor_usage_unauthorized")]
    Unauthorized,
    /// 外部依赖、网络或超时失败。
    #[error("cursor_usage_transport")]
    Transport,
    /// 响应不符合已知协议。
    #[error("cursor_usage_malformed")]
    Malformed,
    /// 分页缺失、重复或读取期间总量变化。
    #[error("cursor_usage_incomplete")]
    Incomplete,
    /// 数值求和溢出。
    #[error("cursor_usage_overflow")]
    Overflow,
    /// 查询已取消。
    #[error("cursor_usage_cancelled")]
    Cancelled,
}

/// 一次冻结的半开时间范围，所有分页共用同一截止时刻。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorUsageRange {
    /// 包含的起始 Unix 毫秒。
    pub start_ms: i64,
    /// 不包含的结束 Unix 毫秒。
    pub end_ms: i64,
}

impl CursorUsageRange {
    /// 复用本机民用日窗口和 DST 规则，当前窗口只读取至观测时刻。
    pub fn for_window(window: LocalUsageWindow, now: i64, zone: &TimeZone) -> Result<Self, CursorUsageError> {
        let dates = dates_for_window(window, now, &TimeStandard::Local, zone).map_err(|_| CursorUsageError::Malformed)?;
        let first = *dates.first().ok_or(CursorUsageError::Malformed)?;
        let next = dates.last().ok_or(CursorUsageError::Malformed)?.checked_add(jiff::Span::new().days(1))
            .map_err(|_| CursorUsageError::Malformed)?;
        let start_ms = day_start_epoch_ms(first, &TimeStandard::Local, zone).ok_or(CursorUsageError::Malformed)?;
        let end_ms = day_start_epoch_ms(next, &TimeStandard::Local, zone).ok_or(CursorUsageError::Malformed)?
            .min(now.checked_add(1).ok_or(CursorUsageError::Malformed)?);
        if start_ms >= end_ms { return Err(CursorUsageError::Malformed) }
        Ok(Self { start_ms, end_ms })
    }
}

/// 单个字段的已上报合计及其覆盖数；缺失绝不变成可信零。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReportedTokens {
    /// 已明确返回的整数 Token 合计。
    pub tokens: u64,
    /// 明确返回此字段的事件数，包括显式零。
    pub reported: u64,
    /// 没有返回此字段的事件数。
    pub missing: u64,
}

/// 上游四个互相独立的字段，不将缓存再次加进输入或捏造总量。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CursorTokenCounts {
    /// API 的 inputTokens 字段。
    pub input: ReportedTokens,
    /// API 的 outputTokens 字段。
    pub output: ReportedTokens,
    /// API 的 cacheReadTokens 字段。
    pub cache_read: ReportedTokens,
    /// API 的 cacheWriteTokens 字段。
    pub cache_write: ReportedTokens,
    /// 上游事件总数。
    pub events: u64,
    /// 完全没有 Token 明细的事件数。
    pub without_usage: u64,
}

/// 同一快照的日期或模型分组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorUsageGroup {
    /// ISO 日期或安全模型名；缺失模型为 None。
    pub label: Option<String>,
    /// 分组分项与覆盖事实。
    pub counts: CursorTokenCounts,
}

/// 只包含非敏感用量的账户级查询结果，不携带会话正文或用户身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorUsageSummary {
    /// 本次查询的完整时间范围。
    pub range: CursorUsageRange,
    /// 冻结本次查询范围的观测 Unix 毫秒。
    pub observed_at_ms: i64,
    /// 整个范围的四分项。
    pub counts: CursorTokenCounts,
    /// 范围内全部民用日期；无事件的完整日期为可信零。
    pub daily: Vec<CursorUsageGroup>,
    /// 模型分组，与日期表基于同一集合。
    pub models: Vec<CursorUsageGroup>,
}

/// 连续分页的权威状态；只有完整结束才允许生成汇总。
pub struct CursorUsagePages {
    range: CursorUsageRange,
    next_page: usize,
    expected: Option<usize>,
    events: std::collections::BTreeMap<String, pages::Event>,
}

impl CursorUsagePages {
    /// 建立空分页状态，不接受反向或空时间范围。
    pub fn new(range: CursorUsageRange) -> Result<Self, CursorUsageError> {
        if range.start_ms >= range.end_ms || range.end_ms.checked_sub(range.start_ms)
            .is_none_or(|span| span > 32 * 24 * 60 * 60 * 1000) { return Err(CursorUsageError::Malformed) }
        Ok(Self { range, next_page: 1, expected: None, events: Default::default() })
    }

    /// 返回下一页编号；明确读满上游总数后结束。
    pub fn next_page(&self) -> Option<usize> {
        (!self.expected.is_some_and(|count| self.events.len() == count)).then_some(self.next_page)
    }
}
