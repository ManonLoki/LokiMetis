//! 图表页的固定时间桶、多维分布与一致事实 DTO。

use loki_metis_core::{
    LocalIndexState, LocalUsageAggregate, MetricFact, UsageMeasure,
};
use serde::{Deserialize, Serialize};

use super::{UsageGroupDto, UsageWindow};

/// 图表页允许请求的固定分组维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartDimensionDto {
    /// 「全部」视图按具体 Agent 分组。
    Agent,
    /// 按模型分组。
    Model,
    /// 按推理强度分组。
    ReasoningEffort,
    /// 按项目分组。
    Project,
    /// 按线程分组。
    Thread,
    /// 按数据根分组。
    Root,
}

/// 概览趋势允许同时选择的固定 Token 指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartTokenMetricDto {
    /// 上游单次总量之和。
    TotalTokens,
    /// 包含缓存的全部输入。
    InputTokens,
    /// 输入中的缓存读取子集。
    CachedInputTokens,
    /// 输入中的缓存写入子集。
    CacheWriteInputTokens,
    /// 全部输出。
    OutputTokens,
    /// 输出中的推理子集。
    ReasoningOutputTokens,
}

/// 用量分布允许选择的固定指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartDistributionMetricDto {
    /// 上游单次总量之和。
    TotalTokens,
    /// 包含缓存的全部输入。
    InputTokens,
    /// 输入中的缓存读取子集。
    CachedInputTokens,
    /// 输入中的缓存写入子集。
    CacheWriteInputTokens,
    /// 全部输出。
    OutputTokens,
    /// 输出中的推理子集。
    ReasoningOutputTokens,
    /// canonical 调用数。
    CallCount,
}

impl From<UsageChartDimensionDto> for loki_metis_core::UsageChartDimension {
    /// 将 IPC 图表维度映射为 core 领域维度。
    fn from(value: UsageChartDimensionDto) -> Self {
        match value {
            UsageChartDimensionDto::Agent => Self::Agent,
            UsageChartDimensionDto::Model => Self::Model,
            UsageChartDimensionDto::ReasoningEffort => Self::ReasoningEffort,
            UsageChartDimensionDto::Project => Self::Project,
            UsageChartDimensionDto::Thread => Self::Thread,
            UsageChartDimensionDto::Root => Self::Root,
        }
    }
}

impl From<loki_metis_core::UsageChartDimension> for UsageChartDimensionDto {
    /// 将 core 图表维度映射回稳定 IPC 枚举。
    fn from(value: loki_metis_core::UsageChartDimension) -> Self {
        match value {
            loki_metis_core::UsageChartDimension::Agent => Self::Agent,
            loki_metis_core::UsageChartDimension::Model => Self::Model,
            loki_metis_core::UsageChartDimension::ReasoningEffort => {
                Self::ReasoningEffort
            }
            loki_metis_core::UsageChartDimension::Project => Self::Project,
            loki_metis_core::UsageChartDimension::Thread => Self::Thread,
            loki_metis_core::UsageChartDimension::Root => Self::Root,
        }
    }
}

/// 图表横轴使用的固定粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartGranularityDto {
    /// 单个民用日固定返回 24 个小时桶。
    Hour,
    /// 多日范围按民用日返回桶。
    Day,
}

/// 图表横轴上的一个完整时间桶。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageChartBucketDto {
    /// 小时或日期稳定键。
    pub key: String,
    /// 横轴短标签。
    pub label: String,
    /// 当前尚未结束的小时或日期。
    pub in_progress: bool,
    /// 桶内可加总计量。
    pub measure: UsageMeasure,
}

/// 图表页一次请求返回的时间趋势与维度分布一致快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageChartDto {
    /// 请求的自然日窗口。
    pub window: UsageWindow,
    /// 请求的固定分组维度。
    pub dimension: UsageChartDimensionDto,
    /// 横轴实际粒度。
    pub granularity: UsageChartGranularityDto,
    /// 与同一快照绑定的索引状态。
    pub index_state: LocalIndexState,
    /// 窗口下界 Unix 毫秒。
    pub lower_bound_epoch_ms: i64,
    /// 统一观测时刻 Unix 毫秒。
    pub observed_at_epoch_ms: i64,
    /// 窗口总量及质量元数据。
    pub fact: MetricFact<LocalUsageAggregate>,
    /// 从最旧到最新且不省略零值的时间桶。
    pub buckets: Vec<UsageChartBucketDto>,
    /// 确定性排序后的前十个维度分组。
    pub groups: Vec<UsageGroupDto>,
    /// 超过十个分组时合并的其余项。
    pub remainder: Option<UsageGroupDto>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IPC 维度只接受固定枚举，拒绝 Cursor 和任意表达式。
    #[test]
    fn chart_dimension_wire_values_are_fixed() {
        assert_eq!(
            serde_json::from_str::<UsageChartDimensionDto>("\"reasoningEffort\"").unwrap(),
            UsageChartDimensionDto::ReasoningEffort
        );
        assert_eq!(
            loki_metis_core::UsageChartDimension::from(
                UsageChartDimensionDto::Agent
            ),
            loki_metis_core::UsageChartDimension::Agent
        );
        assert!(serde_json::from_str::<UsageChartDimensionDto>("\"cursor\"").is_err());
        assert!(serde_json::from_str::<UsageChartDimensionDto>("\"model || project\"").is_err());
    }
}
