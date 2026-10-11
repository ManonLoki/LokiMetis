//! 把 WorkBuddy 逐请求日桶映射为六个窗口，并并入「全部」本机概览。

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::Date;
use jiff::tz::TimeZone;

use super::record::ValidatedWorkbuddyUsageRecord;
use super::{WorkbuddyDailyBucket, WorkbuddySessionTiming};
use crate::{
    Completeness, Confidence, CoverageReport, CoverageState, LocalIndexState, LocalRecordsSummary,
    LocalUsageWindow, MetricFact, MetricScope, ProviderKind, TimeStandard, TimelineError,
    TokenUsage, WindowUsage, civil_date_for_timestamp, coverage_completeness, empty_coverage,
    inclusive_calendar_range, local_fact_quality,
};

/// 单个日历窗口的 WorkBuddy 精确请求用量、会话与 Trace 诊断。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyWindowAggregate {
    /// 六个固定日历窗口之一。
    pub window: LocalUsageWindow,
    /// 窗口内产生过请求的去重会话数量。
    pub session_count: u64,
    /// 上游请求数。
    pub request_count: u64,
    /// 顶层会话请求数。
    pub top_level_request_count: u64,
    /// subagent 请求数。
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
    /// 发生缓存读取的 usage 事件所声明的请求数。
    pub cached_read_request_count: u64,
    /// 窗口内涉及的去重 transcript 来源数量。
    pub source_count: u64,
    /// 逐请求积分；任一记录缺失时保持未提供。
    pub credits: Option<f64>,
    /// 窗口内有用量的民用日，格式 `YYYY-MM-DD`，升序。
    pub dates: Vec<String>,
    /// 窗口内活跃会话按全局首末请求计算的平均时长，单位秒。
    pub average_session_duration_seconds: f64,
    /// 窗口内开始的 trace 数量。
    pub trace_total_count: u64,
    /// 窗口内状态为 `Error` 的 trace 数量。
    pub trace_error_count: u64,
    /// 窗口内状态为 `Cancelled` 的 trace 数量。
    pub trace_cancelled_count: u64,
    /// 窗口内 trace 平均耗时，单位毫秒。
    pub trace_average_duration_ms: f64,
}

/// 按查看时间标准把逐请求记录折叠成六个可对账窗口。
pub(super) fn workbuddy_windows_from_usage(
    records: &[ValidatedWorkbuddyUsageRecord],
    session_timings: &BTreeMap<&str, WorkbuddySessionTiming>,
    buckets: &[WorkbuddyDailyBucket],
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<Vec<WorkbuddyWindowAggregate>, TimelineError> {
    let today = civil_date_for_timestamp(observed_at_epoch_ms, time_standard, device_tz)
        .ok_or(TimelineError::InvalidObservedTimestamp)?;
    LocalUsageWindow::OVERVIEW_WINDOWS
        .into_iter()
        .map(|window| {
            let (from, to) = inclusive_calendar_range(window, today)?;
            let mut aggregate = WorkbuddyWindowAggregate {
                window,
                session_count: 0,
                request_count: 0,
                top_level_request_count: 0,
                subagent_request_count: 0,
                input_tokens: 0,
                cached_input_tokens: 0,
                uncached_input_tokens: 0,
                output_tokens: 0,
                tokens: 0,
                cached_read_request_count: 0,
                source_count: 0,
                credits: Some(0.0),
                dates: Vec::new(),
                average_session_duration_seconds: 0.0,
                trace_total_count: 0,
                trace_error_count: 0,
                trace_cancelled_count: 0,
                trace_average_duration_ms: 0.0,
            };
            let mut trace_duration_sum_ms = 0_i64;
            for bucket in buckets {
                let Ok(date) = bucket.date.parse::<Date>() else {
                    continue;
                };
                if date < from || date > to {
                    continue;
                }
                aggregate.request_count =
                    aggregate.request_count.saturating_add(bucket.request_count);
                aggregate.top_level_request_count = aggregate
                    .top_level_request_count
                    .saturating_add(bucket.top_level_request_count);
                aggregate.subagent_request_count = aggregate
                    .subagent_request_count
                    .saturating_add(bucket.subagent_request_count);
                aggregate.input_tokens = aggregate.input_tokens.saturating_add(bucket.input_tokens);
                aggregate.cached_input_tokens = aggregate
                    .cached_input_tokens
                    .saturating_add(bucket.cached_input_tokens);
                aggregate.uncached_input_tokens = aggregate
                    .uncached_input_tokens
                    .saturating_add(bucket.uncached_input_tokens);
                aggregate.output_tokens =
                    aggregate.output_tokens.saturating_add(bucket.output_tokens);
                aggregate.tokens = aggregate.tokens.saturating_add(bucket.tokens);
                aggregate.cached_read_request_count = aggregate
                    .cached_read_request_count
                    .saturating_add(bucket.cached_read_request_count);
                aggregate.credits =
                    super::model_usage::sum_optional_credit(aggregate.credits, bucket.credits);
                aggregate.trace_total_count = aggregate
                    .trace_total_count
                    .saturating_add(bucket.trace_total_count);
                aggregate.trace_error_count = aggregate
                    .trace_error_count
                    .saturating_add(bucket.trace_error_count);
                aggregate.trace_cancelled_count = aggregate
                    .trace_cancelled_count
                    .saturating_add(bucket.trace_cancelled_count);
                trace_duration_sum_ms =
                    trace_duration_sum_ms.saturating_add(bucket.trace_duration_sum_ms);
                aggregate.dates.push(bucket.date.clone());
            }

            let mut active_sessions = BTreeSet::<&str>::new();
            let mut source_ids = BTreeSet::<&str>::new();
            for record in records {
                let Some(date) =
                    civil_date_for_timestamp(record.occurred_at_epoch_ms, time_standard, device_tz)
                else {
                    continue;
                };
                if date < from || date > to {
                    continue;
                }
                active_sessions.insert(record.session_key.as_str());
                source_ids.extend(record.source_ids.iter().map(String::as_str));
            }
            aggregate.session_count = u64::try_from(active_sessions.len()).unwrap_or(u64::MAX);
            aggregate.source_count = u64::try_from(source_ids.len()).unwrap_or(u64::MAX);
            let duration_sum_ms = active_sessions.iter().fold(0_i64, |sum, session_key| {
                sum.saturating_add(
                    session_timings
                        .get(*session_key)
                        .map_or(0, WorkbuddySessionTiming::duration_ms),
                )
            });
            aggregate.average_session_duration_seconds =
                average_or_zero(duration_sum_ms, aggregate.session_count, 1_000.0);
            aggregate.trace_average_duration_ms =
                average_or_zero(trace_duration_sum_ms, aggregate.trace_total_count, 1.0);
            Ok(aggregate)
        })
        .collect()
}

/// 把 WorkBuddy 精确输入/缓存/输出/总量与请求数并入已有本机概览。
pub fn merge_workbuddy_into_local_windows(
    mut summary: LocalRecordsSummary,
    workbuddy_windows: &[WorkbuddyWindowAggregate],
    coverage: &CoverageReport,
) -> LocalRecordsSummary {
    if summary.index_state == LocalIndexState::ReadyNoCalls
        && workbuddy_windows
            .iter()
            .any(|window| window.request_count > 0)
    {
        summary.index_state = LocalIndexState::Ready;
    }
    for window_usage in &mut summary.windows {
        let Some(workbuddy) = workbuddy_windows
            .iter()
            .find(|item| item.window == window_usage.window)
        else {
            continue;
        };
        let aggregate = &mut window_usage.fact.value;
        aggregate.tokens.input_tokens = aggregate
            .tokens
            .input_tokens
            .saturating_add(workbuddy.input_tokens);
        aggregate.tokens.cached_input_tokens = aggregate
            .tokens
            .cached_input_tokens
            .map(|value| value.saturating_add(workbuddy.cached_input_tokens));
        aggregate.tokens.cache_write_input_tokens = None;
        aggregate.tokens.output_tokens = aggregate
            .tokens
            .output_tokens
            .saturating_add(workbuddy.output_tokens);
        aggregate.tokens.reasoning_output_tokens = None;
        aggregate.tokens.total_tokens = aggregate
            .tokens
            .total_tokens
            .saturating_add(workbuddy.tokens);
        aggregate.call_count = aggregate.call_count.saturating_add(workbuddy.request_count);
        aggregate.cached_read_call_count = aggregate
            .cached_read_call_count
            .map(|count| count.saturating_add(workbuddy.cached_read_request_count));
        aggregate.thread_count = aggregate
            .thread_count
            .saturating_add(workbuddy.session_count);
        if workbuddy.request_count > 0 {
            aggregate.root_count = aggregate.root_count.saturating_add(1);
        }
        aggregate.source_count = aggregate
            .source_count
            .saturating_add(workbuddy.source_count);
        aggregate.cache_read_basis_points = aggregate.tokens.cache_read_basis_points();
        window_usage.fact.completeness = combine_completeness(
            window_usage.fact.completeness,
            coverage_completeness(coverage.state),
        );
    }
    summary
}

/// WorkBuddy 已开启但当次不可读时，保留其他 Agent 的已确认合计，
/// 同时把每个窗口降为部分覆盖，禁止把缺失的 WorkBuddy 用量解释为零。
pub fn mark_workbuddy_unavailable_in_local_windows(
    mut summary: LocalRecordsSummary,
) -> LocalRecordsSummary {
    for window_usage in &mut summary.windows {
        window_usage.fact.completeness =
            combine_completeness(window_usage.fact.completeness, Completeness::Partial);
    }
    summary
}

/// 在没有任何物理 Agent 开启、仅 WorkBuddy 可用时构造「全部」六个窗口。
pub fn build_workbuddy_only_local_windows(
    workbuddy_windows: &[WorkbuddyWindowAggregate],
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
) -> LocalRecordsSummary {
    let has_calls = workbuddy_windows
        .iter()
        .any(|window| window.request_count > 0);
    let index_state = if has_calls {
        LocalIndexState::Ready
    } else {
        LocalIndexState::ReadyNoCalls
    };
    let (freshness, completeness, confidence) =
        local_fact_quality(index_state, coverage.state, Confidence::Exact);
    let windows = LocalUsageWindow::OVERVIEW_WINDOWS
        .into_iter()
        .map(|window| {
            let workbuddy = workbuddy_windows.iter().find(|item| item.window == window);
            let aggregate = workbuddy.map_or_else(
                || crate::empty_local_usage_aggregate_for_provider(provider),
                aggregate_from_workbuddy_window,
            );
            WindowUsage {
                window,
                fact: MetricFact::new(
                    aggregate,
                    provider,
                    MetricScope::DeviceObserved,
                    observed_at_epoch_ms,
                    freshness,
                    completeness,
                    confidence,
                    Some(match provider {
                        ProviderKind::WorkbuddyProjectJsonl => "workbuddy-project-jsonl".to_owned(),
                        ProviderKind::CombinedLocalAgents => {
                            "combined-local-agents-workbuddy".to_owned()
                        }
                        _ => provider.parser_source_label(1),
                    }),
                ),
            }
        })
        .collect();
    LocalRecordsSummary {
        index_state,
        windows,
    }
}

/// 把一个 WorkBuddy 窗口映射成通用本机聚合。
fn aggregate_from_workbuddy_window(
    window: &WorkbuddyWindowAggregate,
) -> crate::LocalUsageAggregate {
    let tokens = TokenUsage::new_with_availability(
        window.input_tokens,
        Some(window.cached_input_tokens),
        None,
        window.output_tokens,
        None,
        Some(window.tokens),
    )
    .unwrap_or_else(|_| TokenUsage::zero_with_component_availability(true, false, false));
    crate::LocalUsageAggregate {
        cached_read_call_count: Some(window.cached_read_request_count),
        cache_read_basis_points: tokens.cache_read_basis_points(),
        tokens,
        call_count: window.request_count,
        thread_count: window.session_count,
        root_count: u64::from(window.request_count > 0),
        source_count: window.source_count,
        duplicate_source_count: 0,
        cross_root_duplicate_source_count: 0,
        confidence: Confidence::Exact,
    }
}

/// 组合两个来源的覆盖语义；任一未知优先保留未知，随后是部分覆盖。
fn combine_completeness(left: Completeness, right: Completeness) -> Completeness {
    match (left, right) {
        (Completeness::Unknown, _) | (_, Completeness::Unknown) => Completeness::Unknown,
        (Completeness::Partial, _) | (_, Completeness::Partial) => Completeness::Partial,
        (Completeness::Complete, Completeness::Complete) => Completeness::Complete,
    }
}

/// 把合计值换成平均数；计数为 0 时返回 0，避免除零。
pub(super) fn average_or_zero(sum: i64, count: u64, scale: f64) -> f64 {
    if count == 0 {
        0.0
    } else {
        sum as f64 / count as f64 / scale
    }
}

/// 测试与无来源回退使用的完整空覆盖。
pub fn complete_workbuddy_coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        ..empty_coverage()
    }
}
