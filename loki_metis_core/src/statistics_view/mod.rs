//! 纯业务的统计视图装配：窗口、分组、重试对账与展示标签。

mod detail;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::display_label::DisplayLabelCode;
use crate::{
    CoverageReport, MetricFact, ProviderKind, TimeStandard, UsageDimension, UsageMeasure,
    aggregate_canonical_usage, dates_for_window, day_start_epoch_ms,
    empty_token_usage_for_provider, filter_canonical_usage_for_dates, group_usage,
};
use detail::{
    assemble_fact, bound_and_reconcile_groups, build_daily_buckets, collect_project_labels,
    collect_thread_labels, label_and_sort_groups, measure_matches_aggregate,
    statistics_reconciliation_error,
};

/// 单条统计分组展示模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageGroupDisplay {
    /// 稳定 ID。
    pub id: String,
    /// 排序与展示标签。
    pub label: String,
    /// 标签语义。
    pub label_code: DisplayLabelCode,
    /// 同名标签消歧索引。
    pub disambiguation_index: Option<usize>,
    /// 分组可加总计量。
    pub measure: UsageMeasure,
    /// 对应窗口总 Token 占比。
    pub total_token_share_basis_points: Option<u16>,
    /// 是否为其余项。
    pub remainder: bool,
}

/// 单日统计桶。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageDailyBucket {
    /// 年月日日期。
    pub local_date: String,
    /// 是否为当前自然日（尚未结束）。
    pub in_progress: bool,
    /// 桶内计量。
    pub measure: UsageMeasure,
}

/// 本地窗口统计页面模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageStatisticsPage {
    /// 统计窗口。
    pub window: crate::LocalUsageWindow,
    /// 按哪个维度聚合。
    pub dimension: UsageDimension,
    /// 与调用页一致的索引状态。
    pub index_state: crate::LocalIndexState,
    /// 窗口下界时间戳。
    pub lower_bound_epoch_ms: i64,
    /// 观测时刻。
    pub observed_at_epoch_ms: i64,
    /// 窗口汇总事实。
    pub fact: MetricFact<crate::LocalUsageAggregate>,
    /// 每日桶。
    pub daily_buckets: Vec<UsageDailyBucket>,
    /// 前十（或更少）分组。
    pub groups: Vec<UsageGroupDisplay>,
    /// 截断后其余项。
    pub remainder: Option<UsageGroupDisplay>,
}

/// 对单一核心快照按窗口与维度返回一致统计。
#[allow(clippy::too_many_arguments)]
pub fn build_usage_statistics(
    canonical: &crate::CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    index_state: crate::LocalIndexState,
    coverage: &CoverageReport,
    window: crate::LocalUsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
) -> Result<UsageStatisticsPage, String> {
    build_usage_statistics_with_standard(
        canonical,
        root_aliases,
        index_state,
        coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        provider,
        source_version,
        TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
}

/// 按已保存时间标准对单一核心快照返回一致统计。
#[allow(clippy::too_many_arguments)]
pub fn build_usage_statistics_with_standard(
    canonical: &crate::CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    index_state: crate::LocalIndexState,
    coverage: &CoverageReport,
    window: crate::LocalUsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<UsageStatisticsPage, String> {
    build_usage_statistics_with_window(
        canonical,
        root_aliases,
        index_state,
        coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        provider,
        source_version,
        time_standard,
        device_tz,
    )
    .map(|statistics| statistics.page)
}

/// 一次窗口过滤与聚合的完整产出：统计页面，连同图表页复用所需的窗口内
/// canonical 调用集合、总计量与窗口日期序列，避免再次过滤和聚合同一份数据。
pub(crate) struct WindowStatistics {
    pub page: UsageStatisticsPage,
    pub window_calls: crate::CanonicalUsageSet,
    pub total: UsageMeasure,
    pub dates: Vec<jiff::civil::Date>,
}

/// 与 `build_usage_statistics_with_standard` 共享同一次窗口过滤与聚合逻辑。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_usage_statistics_with_window(
    canonical: &crate::CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    index_state: crate::LocalIndexState,
    coverage: &CoverageReport,
    window: crate::LocalUsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<WindowStatistics, String> {
    let dates = dates_for_window(window, observed_at_epoch_ms, &time_standard, device_tz)
        .map_err(|_| statistics_reconciliation_error())?;
    let first_date = dates
        .first()
        .copied()
        .ok_or_else(statistics_reconciliation_error)?;
    let lower_bound_epoch_ms = day_start_epoch_ms(first_date, &time_standard, device_tz)
        .ok_or_else(statistics_reconciliation_error)?;

    let mut window_calls =
        filter_canonical_usage_for_dates(canonical, &dates, &time_standard, device_tz);

    // 窗口过滤后的调用可能为空（例如今日无调用），此时聚合会退回
    // `empty_tokens` 表达的零值形状；这里预先把它订正为当前 provider
    // 的正确形状（例如 Claude 不提供 reasoning_output_tokens），
    // 避免复用推断自历史调用、可能不匹配当前 provider 的默认形状。
    window_calls.empty_tokens = if provider == ProviderKind::CombinedLocalAgents {
        canonical.empty_tokens.clone()
    } else {
        empty_token_usage_for_provider(provider)
    };
    window_calls.total_token_accounting = provider.local_total_token_accounting();

    let aggregate =
        aggregate_canonical_usage(&window_calls).map_err(|_| statistics_reconciliation_error())?;

    // 空日/小时桶必须沿用当前窗口实际总量的分项可用性。联合视图中某个 Agent
    // 本窗口没有调用时，不能让它的 provider 空单位元把其余 Agent 已观察到的
    // 可选分项降为“未提供”，否则完整桶求和会与窗口总量失去可加性。
    window_calls.empty_tokens = aggregate.tokens.zero_matching_availability();

    let grouped = group_usage(&window_calls, dimension, usize::MAX)
        .map_err(|_| statistics_reconciliation_error())?;
    if grouped.remainder.is_some() {
        return Err(statistics_reconciliation_error());
    }
    if !measure_matches_aggregate(&grouped.total, &aggregate) {
        return Err(statistics_reconciliation_error());
    }
    let total = grouped.total.clone();

    let daily_buckets = build_daily_buckets(
        &window_calls,
        &dates,
        observed_at_epoch_ms,
        time_standard,
        device_tz,
    )?;
    let daily_total = detail::sum_measures(daily_buckets.iter().map(|bucket| &bucket.measure))?;
    if daily_total != total {
        return Err(statistics_reconciliation_error());
    }

    let project_labels = if dimension == UsageDimension::Project {
        collect_project_labels(&window_calls.calls)
    } else {
        BTreeMap::new()
    };
    let thread_labels = if dimension == UsageDimension::Thread {
        collect_thread_labels(&window_calls.calls)
    } else {
        BTreeMap::new()
    };
    let labeled = label_and_sort_groups(
        grouped.groups,
        root_aliases,
        dimension,
        &project_labels,
        &thread_labels,
    )?;
    let (groups, remainder) =
        bound_and_reconcile_groups(labeled, &total, crate::statistics::STATISTICS_GROUP_LIMIT)?;
    let fact = assemble_fact(
        aggregate,
        index_state,
        coverage.state,
        provider,
        observed_at_epoch_ms,
        source_version,
    )?;

    Ok(WindowStatistics {
        page: UsageStatisticsPage {
            window,
            dimension,
            index_state,
            lower_bound_epoch_ms,
            observed_at_epoch_ms,
            fact,
            daily_buckets,
            groups,
            remainder,
        },
        window_calls,
        total,
        dates,
    })
}
