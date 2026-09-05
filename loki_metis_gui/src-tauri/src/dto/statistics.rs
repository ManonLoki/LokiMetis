//! 用量页固定窗口、固定维度且可对账的逐日趋势与 Top-N 分组统计。

use loki_metis_core::{
    LocalIndexState, LocalUsageAggregate, MetricFact, UsageMeasure,
};
use serde::Serialize;

use super::{DisplayLabelCodeDto, UsageDimension, UsageWindow};

/// 描述一个本地自然日桶及其可加总统计量。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDailyBucketDto {
    /// 系统本地时区下的 `YYYY-MM-DD` 日期。
    pub local_date: String,
    /// 当前日期仍在进行中，不能解释为完整自然日。
    pub in_progress: bool,
    /// 该日期内 canonical 调用的可加总统计量。
    pub measure: UsageMeasure,
}

/// 描述一个已脱敏统计分组或合并后的其余项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroupDto {
    /// 由固定维度与内部键散列得到的稳定、不透明行 ID。
    pub id: String,
    /// 兼容旧前端的安全回退标签；新前端优先使用展示语义。
    pub label: String,
    /// 标签的稳定展示语义。
    pub label_code: DisplayLabelCodeDto,
    /// 同名安全标签的稳定序号；前端按当前 locale 添加标点。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disambiguation_index: Option<usize>,
    /// 该组 canonical 调用的可加总统计量。
    pub measure: UsageMeasure,
    /// 该组总 Token 占窗口总 Token 的基点数；总量为零时不适用。
    pub total_token_share_basis_points: Option<u16>,
    /// 标识本行是超过 Top-N 上限后合并的全部其余项。
    pub remainder: bool,
}

/// 描述一次固定窗口、固定维度且可对账的本机统计响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatisticsDto {
    /// 今日、昨日、本周、上周、本月或上月的稳定窗口键。
    pub window: UsageWindow,
    /// 模型、推理强度、项目、线程或数据根固定维度。
    pub dimension: UsageDimension,
    /// 与调用和根记录来自同一 SQLite 快照的本机索引状态。
    pub index_state: LocalIndexState,
    /// 窗口闭区间下界的 Unix 毫秒时间戳。
    pub lower_bound_epoch_ms: i64,
    /// 本次统一快照的闭区间上界与观测时间。
    pub observed_at_epoch_ms: i64,
    /// 窗口总计及覆盖、scope、置信度元数据。
    pub fact: MetricFact<LocalUsageAggregate>,
    /// 从窗口首日起逐日排列且不会省略零值日期的桶。
    pub daily_buckets: Vec<UsageDailyBucketDto>,
    /// 确定性排序后的前十个分组。
    pub groups: Vec<UsageGroupDto>,
    /// 超过十个分组时合并的其余项。
    pub remainder: Option<UsageGroupDto>,
}
