//! WorkBuddy 本地用量统计快照与数据源状态的跨进程边界形状。

use serde::Serialize;

use loki_metis_core::{
    CoverageReport, LocalUsageWindow, WorkbuddyDailyBucket, WorkbuddyHourlyBucket,
    WorkbuddyHourlyTrend, WorkbuddyModelUsageGroup, WorkbuddyModelUsageWindow, WorkbuddyScanSource,
    WorkbuddyStatisticsSnapshot, WorkbuddyWindowAggregate,
};

use super::{
    RootActivationStateDto, SourceDiscoveryCodeDto, SourceRootDto, UsageStatisticsDto, UsageWindow,
};

/// 数据源页展示的 WorkBuddy 只读发现状态；不携带绝对路径。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddySourceStatusDto {
    /// 用户是否已在设置中显式开放读取。
    pub enabled: bool,
    /// 本机是否发现可枚举的 `~/.workbuddy/projects` 普通目录。
    pub installed: bool,
    /// 发现命中时的安全展示别名，未发现为 `None`。
    pub alias: Option<String>,
    /// 与物理 Agent 数据源表同形的已发现默认根；未发现为空。
    pub roots: Vec<SourceRootDto>,
}

impl WorkbuddySourceStatusDto {
    /// 开关关闭时不探测磁盘，只返回关闭且无根的状态。
    pub(crate) fn disabled() -> Self {
        Self {
            enabled: false,
            installed: false,
            alias: None,
            roots: Vec::new(),
        }
    }

    /// 组合开关、project JSONL 枚举证据与根别名，构造数据源页只读根表。
    pub(crate) fn new(
        enabled: bool,
        source: Option<WorkbuddyScanSource>,
        file_count: u64,
        skipped_count: u64,
        error_count: u64,
        ready: bool,
    ) -> Self {
        let alias = source.as_ref().map(|source| source.alias.clone());
        let roots = source
            .map(|source| {
                vec![workbuddy_default_root_dto(
                    enabled,
                    &source,
                    file_count,
                    skipped_count,
                    error_count,
                    ready,
                )]
            })
            .unwrap_or_default();
        Self {
            enabled,
            installed: !roots.is_empty(),
            alias,
            roots,
        }
    }
}

/// 把已发现的 `~/.workbuddy` 映射成数据源表的一行；不登记产品索引。
fn workbuddy_default_root_dto(
    enabled: bool,
    source: &WorkbuddyScanSource,
    file_count: u64,
    skipped_count: u64,
    error_count: u64,
    ready: bool,
) -> SourceRootDto {
    SourceRootDto {
        id: source.root_id.clone(),
        alias: source.alias.clone(),
        enabled,
        activation_state: if ready {
            RootActivationStateDto::Ready
        } else if skipped_count > 0 || error_count > 0 {
            RootActivationStateDto::ValidationFailed
        } else {
            RootActivationStateDto::ConfirmedUnindexed
        },
        is_primary: false,
        discovery_label: "默认数据目录".to_owned(),
        discovery_code: SourceDiscoveryCodeDto::DefaultRoot,
        file_count,
        skipped_count,
        error_count,
        duplicate_count: 0,
        last_scan_at_epoch_ms: None,
    }
}

/// 单日 project JSONL 请求、会话、Token 与积分。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyDailyBucketDto {
    /// 查看时间标准下的民用日期，格式 `YYYY-MM-DD`。
    pub date: String,
    /// 当日产生过请求的去重会话数量。
    pub session_count: u64,
    /// 当日上游请求数。
    pub request_count: u64,
    /// 当日顶层会话请求数。
    pub top_level_request_count: u64,
    /// 当日 subagent 请求数。
    pub subagent_request_count: u64,
    /// 输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub uncached_input_tokens: u64,
    /// 输出 Token。
    pub output_tokens: u64,
    /// 输入与输出之和。
    pub tokens: u64,
    /// 发生缓存读取的上游请求数。
    pub cached_read_request_count: u64,
    /// 逐请求积分；任一记录缺失时为 `null`。
    pub credits: Option<f64>,
    /// 该日是否仍是当前查看标准下的进行中当天。
    pub in_progress: bool,
}

impl From<WorkbuddyDailyBucket> for WorkbuddyDailyBucketDto {
    /// core 已完成逐事件日期归属，IPC 层只做无损字段映射。
    fn from(bucket: WorkbuddyDailyBucket) -> Self {
        Self {
            date: bucket.date,
            session_count: bucket.session_count,
            request_count: bucket.request_count,
            top_level_request_count: bucket.top_level_request_count,
            subagent_request_count: bucket.subagent_request_count,
            input_tokens: bucket.input_tokens,
            cached_input_tokens: bucket.cached_input_tokens,
            uncached_input_tokens: bucket.uncached_input_tokens,
            output_tokens: bucket.output_tokens,
            tokens: bucket.tokens,
            cached_read_request_count: bucket.cached_read_request_count,
            credits: bucket.credits,
            in_progress: bucket.in_progress,
        }
    }
}

/// 单日窗口内一个民用小时的请求、会话与用量。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyHourlyBucketDto {
    /// 稳定时间键，格式 `YYYY-MM-DDTHH`。
    pub key: String,
    /// 横轴短标签，格式 `HH:00`。
    pub label: String,
    /// 该小时产生过请求的去重会话数量。
    pub session_count: u64,
    /// 该小时上游请求数。
    pub request_count: u64,
    /// 该小时顶层会话请求数。
    pub top_level_request_count: u64,
    /// 该小时 subagent 请求数。
    pub subagent_request_count: u64,
    /// 该小时输入与输出 Token 合计。
    pub tokens: u64,
    /// 该小时逐请求积分；任一记录缺失时为 `null`。
    pub credits: Option<f64>,
    /// 该小时是否仍是观测时刻所在的进行中小时。
    pub in_progress: bool,
}

impl From<WorkbuddyHourlyBucket> for WorkbuddyHourlyBucketDto {
    /// 小时桶字段与 core 一一对应，不做二次计算。
    fn from(bucket: WorkbuddyHourlyBucket) -> Self {
        Self {
            key: bucket.key,
            label: bucket.label,
            session_count: bucket.session_count,
            request_count: bucket.request_count,
            top_level_request_count: bucket.top_level_request_count,
            subagent_request_count: bucket.subagent_request_count,
            tokens: bucket.tokens,
            credits: bucket.credits,
            in_progress: bucket.in_progress,
        }
    }
}

/// 今日或昨日单日窗口的完整 24 小时趋势。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyHourlyTrendDto {
    /// 只会是今日或昨日。
    pub window: UsageWindow,
    /// 该窗口对应的民用日期，格式 `YYYY-MM-DD`。
    pub date: String,
    /// 从 00:00 到 23:00 顺序排列的 24 个小时桶。
    pub buckets: Vec<WorkbuddyHourlyBucketDto>,
}

impl From<WorkbuddyHourlyTrend> for WorkbuddyHourlyTrendDto {
    /// 把 core 单日小时趋势映射为 IPC 形状。
    fn from(trend: WorkbuddyHourlyTrend) -> Self {
        Self {
            window: to_usage_window(trend.window),
            date: trend.date,
            buckets: trend
                .buckets
                .into_iter()
                .map(WorkbuddyHourlyBucketDto::from)
                .collect(),
        }
    }
}

/// 单个日历窗口的 WorkBuddy 精确用量与独立 Trace 诊断。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyWindowDto {
    /// 六个固定日历窗口之一。
    pub window: UsageWindow,
    /// 窗口内产生过请求的去重会话数量。
    pub session_count: u64,
    /// 窗口内上游请求数。
    pub request_count: u64,
    /// 窗口内顶层会话请求数。
    pub top_level_request_count: u64,
    /// 窗口内 subagent 请求数。
    pub subagent_request_count: u64,
    /// 输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub uncached_input_tokens: u64,
    /// 输出 Token。
    pub output_tokens: u64,
    /// 输入与输出之和。
    pub tokens: u64,
    /// 发生缓存读取的上游请求数。
    pub cached_read_request_count: u64,
    /// 窗口内涉及的去重 transcript 来源数。
    pub source_count: u64,
    /// 逐请求积分；任一记录缺失时为 `null`。
    pub credits: Option<f64>,
    /// 窗口内有用量的民用日。
    pub dates: Vec<String>,
    /// 活跃会话按全局首末请求计算的平均时长，单位秒。
    pub average_session_duration_seconds: f64,
    /// 窗口内开始的 Trace 数量。
    pub trace_total_count: u64,
    /// 窗口内状态为错误的 Trace 数量。
    pub trace_error_count: u64,
    /// 窗口内状态为已取消的 Trace 数量。
    pub trace_cancelled_count: u64,
    /// 窗口内 Trace 平均耗时，单位毫秒。
    pub trace_average_duration_ms: f64,
}

impl From<WorkbuddyWindowAggregate> for WorkbuddyWindowDto {
    /// 把 core 窗口合计映射为 IPC 形状。
    fn from(window: WorkbuddyWindowAggregate) -> Self {
        Self {
            window: to_usage_window(window.window),
            session_count: window.session_count,
            request_count: window.request_count,
            top_level_request_count: window.top_level_request_count,
            subagent_request_count: window.subagent_request_count,
            input_tokens: window.input_tokens,
            cached_input_tokens: window.cached_input_tokens,
            uncached_input_tokens: window.uncached_input_tokens,
            output_tokens: window.output_tokens,
            tokens: window.tokens,
            cached_read_request_count: window.cached_read_request_count,
            source_count: window.source_count,
            credits: window.credits,
            dates: window.dates,
            average_session_duration_seconds: window.average_session_duration_seconds,
            trace_total_count: window.trace_total_count,
            trace_error_count: window.trace_error_count,
            trace_cancelled_count: window.trace_cancelled_count,
            trace_average_duration_ms: window.trace_average_duration_ms,
        }
    }
}

/// 单个实际模型的 project JSONL 精确用量。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyModelUsageGroupDto {
    /// 安全实际模型名；`None` 表示上游没有提供可用模型名。
    pub model: Option<String>,
    /// 上游请求数。
    pub call_count: u64,
    /// 输入与输出 Token 合计；缓存输入不重复计入。
    pub total_tokens: u64,
    /// 全部输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub uncached_input_tokens: u64,
    /// 输出 Token。
    pub output_tokens: u64,
    /// 逐请求积分；任一记录缺失时为 `null`。
    pub credits: Option<f64>,
    /// 顶层会话请求数。
    pub top_level_call_count: u64,
    /// subagent 请求数。
    pub subagent_call_count: u64,
}

impl From<WorkbuddyModelUsageGroup> for WorkbuddyModelUsageGroupDto {
    /// 模型分项已在 core 校验并聚合，IPC 层只做字段映射。
    fn from(group: WorkbuddyModelUsageGroup) -> Self {
        Self {
            model: group.model,
            call_count: group.call_count,
            total_tokens: group.total_tokens,
            input_tokens: group.input_tokens,
            cached_input_tokens: group.cached_input_tokens,
            uncached_input_tokens: group.uncached_input_tokens,
            output_tokens: group.output_tokens,
            credits: group.credits,
            top_level_call_count: group.top_level_call_count,
            subagent_call_count: group.subagent_call_count,
        }
    }
}

/// 同一日历窗口内的 project JSONL 逐模型精确用量。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyModelUsageWindowDto {
    /// 六个固定日历窗口之一。
    pub window: UsageWindow,
    /// 按总 Token 降序稳定排列的模型明细。
    pub groups: Vec<WorkbuddyModelUsageGroupDto>,
}

impl From<WorkbuddyModelUsageWindow> for WorkbuddyModelUsageWindowDto {
    /// 保留 core 的窗口与稳定模型排序。
    fn from(window: WorkbuddyModelUsageWindow) -> Self {
        Self {
            window: to_usage_window(window.window),
            groups: window
                .groups
                .into_iter()
                .map(WorkbuddyModelUsageGroupDto::from)
                .collect(),
        }
    }
}

/// WorkBuddy 用量页一次 IPC 返回的逐请求统计与同窗口逐模型分项。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyUsageDetailsDto {
    /// 由 project JSONL 逐请求事实生成的统计页。
    pub statistics: UsageStatisticsDto,
    /// 与统计页来自同次读取、同观测时刻和日历窗口的模型分项。
    pub model_usage: WorkbuddyModelUsageWindowDto,
}

/// 把 core 日历窗口映射为 IPC 窗口枚举。
fn to_usage_window(window: LocalUsageWindow) -> UsageWindow {
    match window {
        LocalUsageWindow::Today => UsageWindow::Today,
        LocalUsageWindow::Yesterday => UsageWindow::Yesterday,
        LocalUsageWindow::ThisWeek => UsageWindow::ThisWeek,
        LocalUsageWindow::LastWeek => UsageWindow::LastWeek,
        LocalUsageWindow::ThisMonth => UsageWindow::ThisMonth,
        LocalUsageWindow::LastMonth => UsageWindow::LastMonth,
    }
}

/// WorkBuddy project JSONL 本地用量统计快照。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyStatisticsDto {
    /// 全历史产生过用量的去重会话总数。
    pub total_sessions: u64,
    /// 全历史上游请求总数。
    pub total_requests: u64,
    /// 顶层会话请求总数。
    pub top_level_requests: u64,
    /// subagent 请求总数。
    pub subagent_requests: u64,
    /// 全部输入 Token，包含缓存输入。
    pub total_input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub total_cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub total_uncached_input_tokens: u64,
    /// 输出 Token。
    pub total_output_tokens: u64,
    /// 输入与输出之和。
    pub total_tokens: u64,
    /// 全部逐请求积分；任一记录缺失时为 `null`。
    pub total_credits: Option<f64>,
    /// 去重会话按首末请求计算的平均时长，单位秒。
    pub average_session_duration_seconds: f64,
    /// 按查看时间标准民用日升序排列的每日分布。
    pub daily_buckets: Vec<WorkbuddyDailyBucketDto>,
    /// 六个日历窗口的精确请求用量。
    pub windows: Vec<WorkbuddyWindowDto>,
    /// 今日与昨日两个单日窗口的固定 24 小时趋势。
    pub hourly_trends: Vec<WorkbuddyHourlyTrendDto>,
    /// Trace 诊断记录总数。
    pub trace_total_count: u64,
    /// 状态为错误的 Trace 数量。
    pub trace_error_count: u64,
    /// 状态为已取消的 Trace 数量。
    pub trace_cancelled_count: u64,
    /// Trace 平均耗时，单位毫秒。
    pub trace_average_duration_ms: f64,
    /// 本次 JSONL 用量读取的覆盖结论。
    pub coverage: CoverageReport,
    /// 本次计算发生时刻的 Unix 毫秒时间戳。
    pub generated_at_epoch_ms: i64,
}

impl From<WorkbuddyStatisticsSnapshot> for WorkbuddyStatisticsDto {
    /// 把 core 快照映射为 IPC 形状，窗口合计携带卡片所需的时长与 trace。
    fn from(snapshot: WorkbuddyStatisticsSnapshot) -> Self {
        Self {
            total_sessions: snapshot.total_sessions,
            total_requests: snapshot.total_requests,
            top_level_requests: snapshot.top_level_requests,
            subagent_requests: snapshot.subagent_requests,
            total_input_tokens: snapshot.total_input_tokens,
            total_cached_input_tokens: snapshot.total_cached_input_tokens,
            total_uncached_input_tokens: snapshot.total_uncached_input_tokens,
            total_output_tokens: snapshot.total_output_tokens,
            total_tokens: snapshot.total_tokens,
            total_credits: snapshot.total_credits,
            average_session_duration_seconds: snapshot.average_session_duration_seconds,
            daily_buckets: snapshot
                .daily_buckets
                .into_iter()
                .map(WorkbuddyDailyBucketDto::from)
                .collect(),
            windows: snapshot
                .windows
                .into_iter()
                .map(WorkbuddyWindowDto::from)
                .collect(),
            hourly_trends: snapshot
                .hourly_trends
                .into_iter()
                .map(WorkbuddyHourlyTrendDto::from)
                .collect(),
            trace_total_count: snapshot.trace_total_count,
            trace_error_count: snapshot.trace_error_count,
            trace_cancelled_count: snapshot.trace_cancelled_count,
            trace_average_duration_ms: snapshot.trace_average_duration_ms,
            coverage: snapshot.coverage,
            generated_at_epoch_ms: snapshot.generated_at_epoch_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模型 IPC 必须逐项暴露输入、缓存、未缓存与输出，且不再混入 Trace 口径。
    #[test]
    fn model_usage_dto_preserves_exact_project_jsonl_components() {
        let value = serde_json::to_value(WorkbuddyModelUsageGroupDto::from(
            WorkbuddyModelUsageGroup {
                model: Some("model-a".to_owned()),
                call_count: 3,
                total_tokens: 140,
                input_tokens: 100,
                cached_input_tokens: 60,
                uncached_input_tokens: 40,
                output_tokens: 40,
                credits: Some(1.25),
                top_level_call_count: 2,
                subagent_call_count: 1,
            },
        ))
        .expect("model usage DTO serializes");

        assert_eq!(value["inputTokens"], 100);
        assert_eq!(value["cachedInputTokens"], 60);
        assert_eq!(value["uncachedInputTokens"], 40);
        assert_eq!(value["outputTokens"], 40);
        assert_eq!(value["totalTokens"], 140);
        assert!(value.get("traceCount").is_none());
    }
}
