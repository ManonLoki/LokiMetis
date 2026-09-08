//! 概览页的固定本机时间窗口与整体响应形状。

use loki_metis_core::{LocalIndexState, LocalUsageAggregate, MetricFact};
use serde::{Deserialize, Serialize};

use super::UiMessageCodeDto;

/// 标识概览与统计页面共用的本机时间窗口，保持六个日历窗口口径一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageWindow {
    /// 当前用户时区的当天自然日窗口。
    Today,
    /// 观测日的前一个完整民用日窗口。
    Yesterday,
    /// 当前自然周：周一至当天。
    ThisWeek,
    /// 上一自然周：周一至周日。
    LastWeek,
    /// 当前自然月：当月 1 日至当天。
    ThisMonth,
    /// 上一自然月：1 日至月末。
    LastMonth,
}

/// 描述一个本机窗口的可追溯聚合事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUsageDto {
    /// 稳定窗口键。
    pub window: UsageWindow,
    /// 该窗口内的本机可观察事实。
    pub fact: MetricFact<LocalUsageAggregate>,
}

/// 描述本机记录区块中的相互一致窗口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRecordsSectionDto {
    /// 区分未扫描、需要重扫、已扫描空结果与当前可用索引。
    pub index_state: LocalIndexState,
    /// 今日、昨日、本周、上周、本月和上月六个日历窗口。
    pub windows: Vec<WindowUsageDto>,
}

/// 描述概览 command 的完整响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageOverviewDto {
    /// 真实业务尚未完成装配时继续保持实施门禁。
    pub product_definition_required: bool,
    /// 实施门禁启用时展示的真实阶段说明。
    pub implementation_message: Option<String>,
    /// 与实施或降级说明对应的稳定本地化代码。
    pub implementation_message_code: Option<UiMessageCodeDto>,
    /// 业务装配完成后提供本机记录区块。
    pub local_records: Option<LocalRecordsSectionDto>,
}
