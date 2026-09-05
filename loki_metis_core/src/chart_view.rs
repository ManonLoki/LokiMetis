//! 图表页的固定时间桶与多维分布纯业务装配。

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::display_label::DisplayLabelCode;
use crate::statistics_view::{WindowStatistics, build_usage_statistics_with_window};
use crate::{
    CanonicalUsageSet, CoverageReport, LocalIndexState, LocalUsageWindow, MetricFact, ProviderKind,
    SourceClientKind, TimeStandard, UsageDimension, UsageGroupDisplay, UsageMeasure,
    agent_wire_label,
};

/// 图表页允许选择的固定分组维度；不接受任意字段或表达式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartDimension {
    /// 按具体 Agent 分组，只适用于「全部」联合视图。
    Agent,
    /// 按上游记录的模型名分组。
    #[default]
    Model,
    /// 按上游记录的推理强度分组。
    ReasoningEffort,
    /// 按内容无关的项目键分组。
    Project,
    /// 按内容无关的线程键分组。
    Thread,
    /// 按 canonical provenance 数据根分组。
    Root,
}

impl UsageChartDimension {
    /// 将图表维度转换为可复用的统计维度；Agent 维度由联合视图单独处理。
    fn statistics_dimension(self) -> Option<UsageDimension> {
        match self {
            Self::Agent => None,
            Self::Model => Some(UsageDimension::Model),
            Self::ReasoningEffort => Some(UsageDimension::ReasoningEffort),
            Self::Project => Some(UsageDimension::Project),
            Self::Thread => Some(UsageDimension::Thread),
            Self::Root => Some(UsageDimension::Root),
        }
    }
}

/// 图表横轴的固定粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageChartGranularity {
    /// 单日窗口固定返回 00:00–23:00 共 24 个民用小时桶。
    Hour,
    /// 多日窗口按民用日返回一个桶。
    Day,
}

/// 图表横轴上的一个完整时间桶。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageChartBucket {
    /// 稳定时间键；小时为 `YYYY-MM-DDTHH`，日为 `YYYY-MM-DD`。
    pub key: String,
    /// 适合横轴展示的短标签；小时为 `HH:00`，日为 `YYYY-MM-DD`。
    pub label: String,
    /// 当前尚未结束的小时或自然日。
    pub in_progress: bool,
    /// 桶内可逐字段相加的计量。
    pub measure: UsageMeasure,
}

/// 单一视图、窗口和维度的一致图表快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageChartPage {
    /// 图表窗口。
    pub window: LocalUsageWindow,
    /// 用量分布所用维度。
    pub dimension: UsageChartDimension,
    /// 横轴粒度。
    pub granularity: UsageChartGranularity,
    /// 与概览和调用页一致的索引状态。
    pub index_state: LocalIndexState,
    /// 窗口下界时间戳。
    pub lower_bound_epoch_ms: i64,
    /// 观测时刻。
    pub observed_at_epoch_ms: i64,
    /// 窗口总量事实。
    pub fact: MetricFact<crate::LocalUsageAggregate>,
    /// 从最旧到最新排列的完整横轴桶。
    pub buckets: Vec<UsageChartBucket>,
    /// 按总 Token 排序的前十（或更少）用量分组。
    pub groups: Vec<UsageGroupDisplay>,
    /// 超出前十的确定性其余项。
    pub remainder: Option<UsageGroupDisplay>,
}

/// 按已保存时间标准为单个物理 Agent 生成一致图表快照。
///
/// `Agent` 维度只属于「全部」视图，物理 Agent 请求该维度会被稳定拒绝。
#[allow(clippy::too_many_arguments)]
pub fn build_usage_chart_with_standard(
    canonical: &CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    index_state: LocalIndexState,
    coverage: &CoverageReport,
    window: LocalUsageWindow,
    dimension: UsageChartDimension,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<UsageChartPage, String> {
    if dimension == UsageChartDimension::Agent {
        return Err(chart_dimension_error());
    }
    build_chart(
        canonical,
        root_aliases,
        None,
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
}

/// 为联合视图生成图表；`Agent` 维度依赖逐调用来源映射。
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_combined_chart_with_standard(
    canonical: &CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    origins: &BTreeMap<String, SourceClientKind>,
    index_state: LocalIndexState,
    coverage: &CoverageReport,
    window: LocalUsageWindow,
    dimension: UsageChartDimension,
    observed_at_epoch_ms: i64,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<UsageChartPage, String> {
    build_chart(
        canonical,
        root_aliases,
        Some(origins),
        index_state,
        coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        ProviderKind::CombinedLocalAgents,
        source_version,
        time_standard,
        device_tz,
    )
}

#[allow(clippy::too_many_arguments)]
/// 从同一规范快照构造时间桶与分布组，并在返回前完成对账。
fn build_chart(
    canonical: &CanonicalUsageSet,
    root_aliases: &BTreeMap<String, String>,
    origins: Option<&BTreeMap<String, SourceClientKind>>,
    index_state: LocalIndexState,
    coverage: &CoverageReport,
    window: LocalUsageWindow,
    dimension: UsageChartDimension,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_version: Option<&str>,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<UsageChartPage, String> {
    let statistics_dimension = dimension
        .statistics_dimension()
        .unwrap_or(UsageDimension::Model);
    let WindowStatistics {
        page: statistics,
        window_calls,
        total: expected_total,
        dates,
    } = build_usage_statistics_with_window(
        canonical,
        root_aliases,
        index_state,
        coverage,
        window,
        statistics_dimension,
        observed_at_epoch_ms,
        provider,
        source_version,
        time_standard.clone(),
        device_tz,
    )?;

    let (granularity, buckets) = match window {
        LocalUsageWindow::Today | LocalUsageWindow::Yesterday => (
            UsageChartGranularity::Hour,
            build_hourly_buckets(
                &window_calls,
                dates.first().copied().ok_or_else(chart_read_error)?,
                observed_at_epoch_ms,
                &time_standard,
                device_tz,
            )?,
        ),
        LocalUsageWindow::ThisWeek
        | LocalUsageWindow::LastWeek
        | LocalUsageWindow::ThisMonth
        | LocalUsageWindow::LastMonth => (
            UsageChartGranularity::Day,
            statistics
                .daily_buckets
                .iter()
                .rev()
                .map(|bucket| UsageChartBucket {
                    key: bucket.local_date.clone(),
                    label: bucket.local_date.clone(),
                    in_progress: bucket.in_progress,
                    measure: bucket.measure.clone(),
                })
                .collect(),
        ),
    };
    reconcile_buckets(&buckets, &expected_total)?;

    let (groups, remainder) = if dimension == UsageChartDimension::Agent {
        build_agent_groups(
            &window_calls,
            origins.ok_or_else(chart_dimension_error)?,
            &expected_total,
        )?
    } else {
        (statistics.groups.clone(), statistics.remainder.clone())
    };

    Ok(UsageChartPage {
        window,
        dimension,
        granularity,
        index_state: statistics.index_state,
        lower_bound_epoch_ms: statistics.lower_bound_epoch_ms,
        observed_at_epoch_ms,
        fact: statistics.fact,
        buckets,
        groups,
        remainder,
    })
}

/// 按民用小时建立固定 24 桶，并合并 DST 重复小时、补齐缺失小时。
fn build_hourly_buckets(
    canonical: &CanonicalUsageSet,
    selected_date: jiff::civil::Date,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<Vec<UsageChartBucket>, String> {
    let zone = time_standard.viewing_time_zone(device_tz);
    let observed = Timestamp::from_millisecond(observed_at_epoch_ms)
        .map_err(|_| chart_read_error())?
        .to_zoned(zone.clone());
    let mut calls_by_hour = (0..24)
        .map(|_| Vec::<&crate::UsageCall>::new())
        .collect::<Vec<_>>();

    for call in &canonical.calls {
        let zoned = Timestamp::from_millisecond(call.occurred_at_epoch_ms)
            .map_err(|_| chart_read_error())?
            .to_zoned(zone.clone());
        if zoned.date() != selected_date {
            return Err(chart_read_error());
        }
        let hour = usize::try_from(zoned.hour()).map_err(|_| chart_read_error())?;
        calls_by_hour
            .get_mut(hour)
            .ok_or_else(chart_read_error)?
            .push(call);
    }

    calls_by_hour
        .into_iter()
        .enumerate()
        .map(|(hour, calls)| {
            let measure = crate::statistics::measure_calls(calls.into_iter(), canonical)
                .map_err(|_| chart_read_error())?;
            Ok(UsageChartBucket {
                key: format!(
                    "{:04}-{:02}-{:02}T{hour:02}",
                    selected_date.year(),
                    selected_date.month(),
                    selected_date.day()
                ),
                label: format!("{hour:02}:00"),
                in_progress: observed.date() == selected_date
                    && usize::try_from(observed.hour()).ok() == Some(hour),
                measure,
            })
        })
        .collect()
}

/// 校验全部图表桶逐字段之和与窗口规范总量完全一致。
fn reconcile_buckets(buckets: &[UsageChartBucket], expected: &UsageMeasure) -> Result<(), String> {
    let mut measures = buckets.iter().map(|bucket| &bucket.measure);
    let first = measures.next().cloned().ok_or_else(chart_read_error)?;
    let total = measures.try_fold(first, |total, measure| {
        total.checked_add(measure).map_err(|_| chart_read_error())
    })?;
    if total != *expected {
        return Err(chart_read_error());
    }
    Ok(())
}

/// 为“全部”联合视图按物理 Agent 分组并生成可对账的 Top 10。
fn build_agent_groups(
    canonical: &CanonicalUsageSet,
    origins: &BTreeMap<String, SourceClientKind>,
    total: &UsageMeasure,
) -> Result<(Vec<UsageGroupDisplay>, Option<UsageGroupDisplay>), String> {
    let mut calls_by_client = BTreeMap::<SourceClientKind, Vec<&crate::UsageCall>>::new();
    for call in &canonical.calls {
        let client = origins
            .get(&call.logical_call_id)
            .copied()
            .ok_or_else(chart_read_error)?;
        calls_by_client.entry(client).or_default().push(call);
    }

    let mut groups = calls_by_client
        .into_iter()
        .map(|(client, calls)| {
            let measure = crate::statistics::measure_calls(calls.into_iter(), canonical)
                .map_err(|_| chart_read_error())?;
            Ok((client, measure))
        })
        .collect::<Result<Vec<_>, String>>()?;
    groups.sort_by(|(left_client, left), (right_client, right)| {
        right
            .tokens
            .total_tokens
            .cmp(&left.tokens.total_tokens)
            .then_with(|| right.call_count.cmp(&left.call_count))
            .then_with(|| left_client.cmp(right_client))
    });

    let visible = groups
        .into_iter()
        .map(|(client, measure)| UsageGroupDisplay {
            id: format!("chart-agent-{}", agent_wire_label(client)),
            label: agent_display_label(client).to_owned(),
            label_code: DisplayLabelCode::Literal,
            disambiguation_index: None,
            total_token_share_basis_points: token_share_basis_points(
                measure.tokens.total_tokens,
                total.tokens.total_tokens,
            ),
            measure,
            remainder: false,
        })
        .collect::<Vec<_>>();

    if visible.is_empty() {
        if total.call_count != 0 {
            return Err(chart_read_error());
        }
    } else {
        let mut measures = visible.iter().map(|group| &group.measure);
        let first = measures.next().cloned().ok_or_else(chart_read_error)?;
        let reconciled = measures.try_fold(first, |sum, measure| {
            sum.checked_add(measure).map_err(|_| chart_read_error())
        })?;
        if reconciled != *total {
            return Err(chart_read_error());
        }
    }

    Ok((visible, None))
}

/// 返回物理 Agent 在图表分布中的固定展示名称。
const fn agent_display_label(client: SourceClientKind) -> &'static str {
    match client {
        SourceClientKind::Codex => "Codex",
        SourceClientKind::ClaudeCode => "Claude Code",
        SourceClientKind::GrokBuildCli => "Grok",
        SourceClientKind::WorkBuddy => "WorkBuddy",
    }
}

/// 以基点计算 Token 占比，并在总量为零时保持未知。
fn token_share_basis_points(part: u64, total: u64) -> Option<u16> {
    if total == 0 {
        return None;
    }
    let rounded = (u128::from(part) * 10_000 + u128::from(total / 2)) / u128::from(total);
    Some(rounded.min(10_000).try_into().unwrap_or(10_000))
}

/// 返回图表维度不适用于当前只读视图时的稳定错误文案。
fn chart_dimension_error() -> String {
    "图表维度不适用于当前 Agent 视图。".to_owned()
}

/// 返回图表规范快照无法读取或对账时的稳定错误文案。
fn chart_read_error() -> String {
    "本机图表数据无法完成一致性校验，请重新扫描后再试。".to_owned()
}
