//! WorkBuddy 本机 project JSONL 用量的纯聚合；文件系统读取留在 GUI adapter。

mod hourly;
mod model_usage;
mod record;
mod usage_page;
mod windows;

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::timeline::civil_date_for_timestamp;
use crate::{
    CoverageReport, CoverageState, LocalUsageWindow, TimeStandard, UsageDimension,
    UsageStatisticsPage,
};

pub use hourly::{WorkbuddyHourlyBucket, WorkbuddyHourlyTrend};
pub use model_usage::{WorkbuddyModelUsageGroup, WorkbuddyModelUsageWindow};
pub use record::{WorkbuddyUsageEventRecord, WorkbuddyUsageOrigin, WorkbuddyUsageQuality};
use windows::average_or_zero;
pub use windows::{
    WorkbuddyWindowAggregate, build_workbuddy_only_local_windows, complete_workbuddy_coverage,
    mark_workbuddy_unavailable_in_local_windows, merge_workbuddy_into_local_windows,
};

/// 「全部」保留其他 Agent、但 WorkBuddy 当次不可读时的稳定降级说明。
pub const fn workbuddy_combined_usage_unavailable_message() -> &'static str {
    "WorkBuddy 本机用量暂时无法读取；当前“全部”仅展示其他已开启 Agent，覆盖标记为不完整，请重试。"
}

/// Trace 整体状态；只作为独立健康诊断，不参与 Token 或模型统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbuddyTraceStatus {
    /// trace 内全部 span 正常完成。
    Ok,
    /// trace 内出现未捕获错误。
    Error,
    /// trace 被取消。
    Cancelled,
}

/// 单条 trace 的最小诊断字段。
#[derive(Debug, Clone, PartialEq)]
pub struct WorkbuddyTraceRecord {
    /// trace 起始时刻的 Unix 毫秒时间戳。
    pub started_at_epoch_ms: i64,
    /// trace 总耗时，单位毫秒。
    pub duration_ms: i64,
    /// trace 整体状态。
    pub status: WorkbuddyTraceStatus,
}

/// 单日 WorkBuddy 请求、会话、Token、积分与 Trace 诊断。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyDailyBucket {
    /// 查看时间标准下的民用日期，格式 `YYYY-MM-DD`。
    pub date: String,
    /// 当日产生过请求的去重会话数量。
    pub session_count: u64,
    /// 当日上游请求数。
    pub request_count: u64,
    /// 顶层会话请求数。
    pub top_level_request_count: u64,
    /// subagent 请求数。
    pub subagent_request_count: u64,
    /// 全部输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub uncached_input_tokens: u64,
    /// 输出 Token。
    pub output_tokens: u64,
    /// 输入与输出之和。
    pub tokens: u64,
    /// 发生缓存读取的 usage 事件所声明请求数。
    pub cached_read_request_count: u64,
    /// 当日逐请求积分；任一记录缺失时保持未提供。
    pub credits: Option<f64>,
    /// 该日是否仍是进行中当天。
    pub in_progress: bool,
    /// 当日开始的 trace 数量。
    pub trace_total_count: u64,
    /// 当日错误 trace 数量。
    pub trace_error_count: u64,
    /// 当日取消 trace 数量。
    pub trace_cancelled_count: u64,
    /// 当日 trace 耗时合计，单位毫秒。
    pub trace_duration_sum_ms: i64,
}

/// WorkBuddy 本地统计快照；所有用量字段来自同一批 project JSONL 事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyStatisticsSnapshot {
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
    /// 全部逐请求积分；任一记录缺失时保持未提供。
    pub total_credits: Option<f64>,
    /// 去重会话按首末请求计算的平均时长，单位秒。
    pub average_session_duration_seconds: f64,
    /// 按查看时间标准民用日升序排列的每日分布。
    pub daily_buckets: Vec<WorkbuddyDailyBucket>,
    /// 六个日历窗口的精确请求用量。
    pub windows: Vec<WorkbuddyWindowAggregate>,
    /// 今日与昨日的固定 24 小时趋势。
    pub hourly_trends: Vec<WorkbuddyHourlyTrend>,
    /// trace 诊断记录总数。
    pub trace_total_count: u64,
    /// 错误 trace 数量。
    pub trace_error_count: u64,
    /// 取消 trace 数量。
    pub trace_cancelled_count: u64,
    /// trace 平均耗时，单位毫秒。
    pub trace_average_duration_ms: f64,
    /// 本次 JSONL 用量读取的覆盖结论。
    pub coverage: CoverageReport,
    /// 本次计算发生时刻的 Unix 毫秒时间戳。
    pub generated_at_epoch_ms: i64,
}

/// core 内部去重会话的首末请求时刻。
#[derive(Debug, Clone, Copy)]
struct WorkbuddySessionTiming {
    first_epoch_ms: i64,
    last_epoch_ms: i64,
}

impl WorkbuddySessionTiming {
    /// 使用新请求时间扩展会话边界。
    fn observe(&mut self, occurred_at_epoch_ms: i64) {
        self.first_epoch_ms = self.first_epoch_ms.min(occurred_at_epoch_ms);
        self.last_epoch_ms = self.last_epoch_ms.max(occurred_at_epoch_ms);
    }

    /// 返回非负会话跨度。
    fn duration_ms(&self) -> i64 {
        self.last_epoch_ms
            .saturating_sub(self.first_epoch_ms)
            .max(0)
    }
}

/// 按设备当地民用日计算统计快照；测试与无 adapter 调用使用完整覆盖。
#[cfg(test)]
fn compute_workbuddy_statistics(
    records: &[WorkbuddyUsageEventRecord],
    traces: &[WorkbuddyTraceRecord],
    now_epoch_ms: i64,
) -> WorkbuddyStatisticsSnapshot {
    compute_workbuddy_statistics_with_standard(
        records,
        traces,
        &complete_workbuddy_coverage(),
        now_epoch_ms,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 一次读取内同时需要统计页与逐模型明细时的单次校验入口。
///
/// 与分别调用两个构造器相比，这里只做一遍校验/去重/排序，并且只聚合调用方
/// 请求的那个模型窗口，避免为丢弃的五个窗口重复扫描全部记录。
#[allow(clippy::too_many_arguments)]
pub fn build_workbuddy_usage_details(
    records: &[WorkbuddyUsageEventRecord],
    coverage: &CoverageReport,
    root_id: &str,
    root_alias: &str,
    window: LocalUsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<(UsageStatisticsPage, WorkbuddyModelUsageWindow), String> {
    let (records, quality) = record::prepare_workbuddy_usage_records(records);
    let coverage = coverage_with_quality(coverage, quality);
    let statistics = usage_page::build_workbuddy_usage_statistics(
        &records,
        &coverage,
        root_id,
        root_alias,
        window,
        dimension,
        observed_at_epoch_ms,
        time_standard,
        device_tz,
    )?;
    let today = civil_date_for_timestamp(observed_at_epoch_ms, time_standard, device_tz)
        .ok_or_else(|| "无法按当前时间标准解析观测时刻".to_owned())?;
    let model_usage =
        model_usage::build_model_usage_window(&records, window, today, time_standard, device_tz)
            .ok_or_else(|| "无法解析所选窗口的日历范围".to_owned())?;
    Ok((statistics, model_usage))
}

/// 按逐请求用量与独立 Trace 诊断生成完整快照。
pub fn compute_workbuddy_statistics_with_standard(
    records: &[WorkbuddyUsageEventRecord],
    traces: &[WorkbuddyTraceRecord],
    coverage: &CoverageReport,
    now_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> WorkbuddyStatisticsSnapshot {
    let (records, quality) = record::prepare_workbuddy_usage_records(records);
    let coverage = coverage_with_quality(coverage, quality);
    let mut daily = BTreeMap::<Date, WorkbuddyDailyBucket>::new();
    let mut sessions_by_day = BTreeMap::<Date, BTreeSet<&str>>::new();
    let mut session_timings = BTreeMap::<&str, WorkbuddySessionTiming>::new();
    let mut total_requests = 0_u64;
    let mut top_level_requests = 0_u64;
    let mut subagent_requests = 0_u64;
    let mut total_input_tokens = 0_u64;
    let mut total_cached_input_tokens = 0_u64;
    let mut total_output_tokens = 0_u64;
    let mut total_tokens = 0_u64;
    let mut total_credits = Some(0.0_f64);

    for record in &records {
        session_timings
            .entry(record.session_key.as_str())
            .and_modify(|timing| timing.observe(record.occurred_at_epoch_ms))
            .or_insert(WorkbuddySessionTiming {
                first_epoch_ms: record.occurred_at_epoch_ms,
                last_epoch_ms: record.occurred_at_epoch_ms,
            });
        let request_count = u64::from(record.request_count);
        total_requests = total_requests.saturating_add(request_count);
        match record.origin {
            WorkbuddyUsageOrigin::TopLevel => {
                top_level_requests = top_level_requests.saturating_add(request_count);
            }
            WorkbuddyUsageOrigin::Subagent => {
                subagent_requests = subagent_requests.saturating_add(request_count);
            }
        }
        total_input_tokens = total_input_tokens.saturating_add(record.usage.input_tokens);
        total_cached_input_tokens = total_cached_input_tokens
            .saturating_add(record.usage.cached_input_tokens.unwrap_or_default());
        total_output_tokens = total_output_tokens.saturating_add(record.usage.output_tokens);
        total_tokens = total_tokens.saturating_add(record.usage.total_tokens);
        total_credits = model_usage::sum_optional_credit(total_credits, record.credit);

        let Some(date) =
            civil_date_for_timestamp(record.occurred_at_epoch_ms, time_standard, device_tz)
        else {
            continue;
        };
        sessions_by_day
            .entry(date)
            .or_default()
            .insert(record.session_key.as_str());
        let bucket = daily
            .entry(date)
            .or_insert_with(|| empty_daily_bucket(date));
        bucket.request_count = bucket.request_count.saturating_add(request_count);
        match record.origin {
            WorkbuddyUsageOrigin::TopLevel => {
                bucket.top_level_request_count =
                    bucket.top_level_request_count.saturating_add(request_count);
            }
            WorkbuddyUsageOrigin::Subagent => {
                bucket.subagent_request_count =
                    bucket.subagent_request_count.saturating_add(request_count);
            }
        }
        bucket.input_tokens = bucket
            .input_tokens
            .saturating_add(record.usage.input_tokens);
        let cached = record.usage.cached_input_tokens.unwrap_or_default();
        bucket.cached_input_tokens = bucket.cached_input_tokens.saturating_add(cached);
        bucket.uncached_input_tokens = bucket
            .uncached_input_tokens
            .saturating_add(record.usage.uncached_input_tokens().unwrap_or_default());
        bucket.output_tokens = bucket
            .output_tokens
            .saturating_add(record.usage.output_tokens);
        bucket.tokens = bucket.tokens.saturating_add(record.usage.total_tokens);
        if cached > 0 {
            bucket.cached_read_request_count = bucket
                .cached_read_request_count
                .saturating_add(request_count);
        }
        bucket.credits = model_usage::sum_optional_credit(bucket.credits, record.credit);
    }

    for trace in traces {
        let Some(date) =
            civil_date_for_timestamp(trace.started_at_epoch_ms, time_standard, device_tz)
        else {
            continue;
        };
        let bucket = daily
            .entry(date)
            .or_insert_with(|| empty_daily_bucket(date));
        bucket.trace_total_count = bucket.trace_total_count.saturating_add(1);
        match trace.status {
            WorkbuddyTraceStatus::Error => {
                bucket.trace_error_count = bucket.trace_error_count.saturating_add(1);
            }
            WorkbuddyTraceStatus::Cancelled => {
                bucket.trace_cancelled_count = bucket.trace_cancelled_count.saturating_add(1);
            }
            WorkbuddyTraceStatus::Ok => {}
        }
        bucket.trace_duration_sum_ms = bucket
            .trace_duration_sum_ms
            .saturating_add(trace.duration_ms.max(0));
    }

    for (date, sessions) in sessions_by_day {
        if let Some(bucket) = daily.get_mut(&date) {
            bucket.session_count = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
        }
    }
    if let Some(today) = civil_date_for_timestamp(now_epoch_ms, time_standard, device_tz) {
        let today_text = today.to_string();
        for bucket in daily.values_mut() {
            bucket.in_progress = bucket.date == today_text;
        }
    }

    let duration_sum_ms = session_timings.values().fold(0_i64, |sum, timing| {
        sum.saturating_add(timing.duration_ms())
    });
    let total_sessions = u64::try_from(session_timings.len()).unwrap_or(u64::MAX);
    let average_session_duration_seconds =
        average_or_zero(duration_sum_ms, total_sessions, 1_000.0);
    let trace_total_count = u64::try_from(traces.len()).unwrap_or(u64::MAX);
    let trace_error_count = u64::try_from(
        traces
            .iter()
            .filter(|trace| trace.status == WorkbuddyTraceStatus::Error)
            .count(),
    )
    .unwrap_or(u64::MAX);
    let trace_cancelled_count = u64::try_from(
        traces
            .iter()
            .filter(|trace| trace.status == WorkbuddyTraceStatus::Cancelled)
            .count(),
    )
    .unwrap_or(u64::MAX);
    let trace_duration_sum_ms = traces.iter().fold(0_i64, |sum, trace| {
        sum.saturating_add(trace.duration_ms.max(0))
    });
    let trace_average_duration_ms = average_or_zero(trace_duration_sum_ms, trace_total_count, 1.0);
    let daily_buckets: Vec<WorkbuddyDailyBucket> = daily.into_values().collect();
    let windows = windows::workbuddy_windows_from_usage(
        &records,
        &session_timings,
        &daily_buckets,
        now_epoch_ms,
        time_standard,
        device_tz,
    )
    .unwrap_or_default();
    let hourly_trends =
        hourly::build_workbuddy_hourly_trends(&records, now_epoch_ms, time_standard, device_tz);
    WorkbuddyStatisticsSnapshot {
        total_sessions,
        total_requests,
        top_level_requests,
        subagent_requests,
        total_input_tokens,
        total_cached_input_tokens,
        total_uncached_input_tokens: total_input_tokens.saturating_sub(total_cached_input_tokens),
        total_output_tokens,
        total_tokens,
        total_credits,
        average_session_duration_seconds,
        daily_buckets,
        windows,
        hourly_trends,
        trace_total_count,
        trace_error_count,
        trace_cancelled_count,
        trace_average_duration_ms,
        coverage,
        generated_at_epoch_ms: now_epoch_ms,
    }
}

/// 只把影响 Token 完整性的记录错误计入 coverage；积分缺失由 Option 单独表达。
fn coverage_with_quality(
    coverage: &CoverageReport,
    quality: WorkbuddyUsageQuality,
) -> CoverageReport {
    let mut coverage = coverage.clone();
    let warnings = quality
        .invalid_record_count
        .saturating_add(quality.conflicting_duplicate_record_count);
    coverage.warning_count = coverage.warning_count.saturating_add(warnings);
    if warnings > 0 && coverage.state == CoverageState::Complete {
        coverage.state = CoverageState::Partial;
    }
    coverage
}

/// 为指定民用日构造全零日桶。
fn empty_daily_bucket(date: Date) -> WorkbuddyDailyBucket {
    WorkbuddyDailyBucket {
        date: date.to_string(),
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
        credits: Some(0.0),
        in_progress: false,
        trace_total_count: 0,
        trace_error_count: 0,
        trace_cancelled_count: 0,
        trace_duration_sum_ms: 0,
    }
}

#[cfg(test)]
mod tests;
