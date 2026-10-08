use std::collections::BTreeMap;

use crate::display_label::DisplayLabelCode;
use crate::{
    CoverageState, LocalUsageAggregate, MetricFact, MetricScope, ProviderKind, UsageCall,
    UsageDimension, UsageMeasure, safe_path_basename, safe_thread_title,
};

#[derive(Debug)]
/// 保存统计分组的稳定键、安全标签与完整度量。
pub(crate) struct LabeledUsageGroup {
    id: String,
    key: Option<String>,
    label: String,
    label_code: DisplayLabelCode,
    /// 当多个分组消毒后的展示标签重复时，标记该分组在重复集合内从 1
    /// 开始的消歧序号；标签唯一时为 `None`。
    disambiguation_index: Option<usize>,
    measure: UsageMeasure,
}

/// 按 project_key 聚合观测到的项目末段；同一 key 出现多个不同标签时取
/// 字典序最小值，保证跨扫描/跨调用的确定性输出。
pub(crate) fn collect_project_labels(calls: &[UsageCall]) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for call in calls {
        let Some(key) = call.project_key.as_deref() else {
            continue;
        };
        let Some(label) = call.project_label.as_deref().and_then(safe_path_basename) else {
            continue;
        };
        labels
            .entry(key.to_owned())
            .and_modify(|existing: &mut String| {
                if label < *existing {
                    *existing = label.clone();
                }
            })
            .or_insert(label);
    }
    labels
}

/// 按 thread_key 聚合观测到的线程标题；同一 key 出现多个不同标签时取
/// 字典序最小值，保证跨扫描/跨调用的确定性输出。
pub(crate) fn collect_thread_labels(calls: &[UsageCall]) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for call in calls {
        let Some(label) = call.thread_label.as_deref().and_then(safe_thread_title) else {
            continue;
        };
        labels
            .entry(call.thread_key.clone())
            .and_modify(|existing: &mut String| {
                if label < *existing {
                    *existing = label.clone();
                }
            })
            .or_insert(label);
    }
    labels
}

/// 从规范调用与覆盖信息组装可验证的统计事实。
pub(crate) fn assemble_fact(
    aggregate: LocalUsageAggregate,
    index_state: crate::LocalIndexState,
    coverage_state: CoverageState,
    provider: ProviderKind,
    observed_at_epoch_ms: i64,
    source_version: Option<&str>,
) -> Result<crate::MetricFact<LocalUsageAggregate>, String> {
    let (freshness, completeness, confidence) =
        crate::local_fact_quality(index_state, coverage_state, aggregate.confidence);
    Ok(MetricFact::new(
        aggregate,
        provider,
        MetricScope::DeviceObserved,
        observed_at_epoch_ms,
        freshness,
        completeness,
        confidence,
        source_version.map(str::to_owned),
    ))
}

/// 为原始分组补齐安全标签，并按度量与稳定键排序。
pub(crate) fn label_and_sort_groups(
    groups: Vec<crate::UsageGroup>,
    root_aliases: &BTreeMap<String, String>,
    dimension: UsageDimension,
    project_labels: &BTreeMap<String, String>,
    thread_labels: &BTreeMap<String, String>,
) -> Result<Vec<LabeledUsageGroup>, String> {
    let mut labeled = groups
        .into_iter()
        .map(|group| {
            let (label, label_code) = group_label(
                dimension,
                group.key.as_deref(),
                root_aliases,
                project_labels,
                thread_labels,
            )?;
            let id = crate::stable_group_id(dimension, group.key.as_deref());
            Ok(LabeledUsageGroup {
                id,
                label,
                label_code,
                disambiguation_index: None,
                key: group.key,
                measure: group.measure,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut indices_by_label = BTreeMap::<String, Vec<usize>>::new();
    for (index, group) in labeled.iter().enumerate() {
        indices_by_label
            .entry(group.label.clone())
            .or_default()
            .push(index);
    }
    for indices in indices_by_label
        .values_mut()
        .filter(|indices| indices.len() > 1)
    {
        indices.sort_by(|left, right| {
            stable_optional_key(&labeled[*left].key).cmp(stable_optional_key(&labeled[*right].key))
        });
        for (offset, index) in indices.iter().enumerate() {
            labeled[*index].disambiguation_index = Some(offset + 1);
        }
    }

    labeled.sort_by(|left, right| {
        right
            .measure
            .tokens
            .total_tokens
            .cmp(&left.measure.tokens.total_tokens)
            .then_with(|| right.measure.call_count.cmp(&left.measure.call_count))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| stable_optional_key(&left.key).cmp(stable_optional_key(&right.key)))
    });

    Ok(labeled)
}

/// 将可选分组键转换为稳定排序键，缺失值保持空串。
fn stable_optional_key(key: &Option<String>) -> &str {
    key.as_deref().unwrap_or("")
}

/// 截取 Top 10、合并其余项并校验逐字段总量。
pub(crate) fn bound_and_reconcile_groups(
    mut groups: Vec<LabeledUsageGroup>,
    total: &UsageMeasure,
    display_limit: usize,
) -> Result<
    (
        Vec<crate::UsageGroupDisplay>,
        Option<crate::UsageGroupDisplay>,
    ),
    String,
> {
    let remainder_groups = if groups.len() > display_limit {
        groups.split_off(display_limit)
    } else {
        Vec::new()
    };

    let remainder_measure = if remainder_groups.is_empty() {
        None
    } else {
        Some(sum_measures(
            remainder_groups.iter().map(|group| &group.measure),
        )?)
    };

    let mut displayed_measures = groups
        .iter()
        .map(|group| &group.measure)
        .collect::<Vec<_>>();
    if let Some(measure) = remainder_measure.as_ref() {
        displayed_measures.push(measure);
    }

    // 展示分组 + 其余项之和必须与统计总量完全对账；任何不一致都直接返回
    // 错误而不是静默展示错误数字或悄悄丢弃差额——这里的分组来自本地隐私
    // 敏感聚合，宁可拒绝展示也不能展示不可信的数字。
    if displayed_measures.is_empty() {
        if total.call_count != 0 {
            return Err(statistics_reconciliation_error());
        }
    } else if sum_measures(displayed_measures.into_iter())? != *total {
        return Err(statistics_reconciliation_error());
    }

    let total_tokens = total.tokens.total_tokens;
    let groups = groups
        .into_iter()
        .map(|group| crate::UsageGroupDisplay {
            id: group.id,
            label: group.label,
            label_code: group.label_code,
            disambiguation_index: group.disambiguation_index,
            total_token_share_basis_points: token_share_basis_points(
                group.measure.tokens.total_tokens,
                total_tokens,
            ),
            measure: group.measure,
            remainder: false,
        })
        .collect();

    let remainder = remainder_measure.map(|measure| crate::UsageGroupDisplay {
        id: "statistics-remainder".to_owned(),
        label: "其余项".to_owned(),
        label_code: DisplayLabelCode::Remainder,
        disambiguation_index: None,
        total_token_share_basis_points: token_share_basis_points(
            measure.tokens.total_tokens,
            total_tokens,
        ),
        measure,
        remainder: true,
    });

    Ok((groups, remainder))
}

/// 按所选标准民用日新→旧组装完整日桶；「进行中」只标观测民用日。
///
/// 单次遍历把每条调用的民用日算一次并分桶，而不是对每个日期重新扫描
/// 并重复做时区换算（对 N 天窗口曾是 O(调用数 × N) 的时区转换）。
/// 昨日、上周和上月都已结束，因此不会标为进行中。
pub(crate) fn build_daily_buckets(
    canonical: &crate::CanonicalUsageSet,
    dates: &[jiff::civil::Date],
    observed_at_epoch_ms: i64,
    time_standard: crate::TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<Vec<crate::UsageDailyBucket>, String> {
    let observed_date =
        crate::civil_date_for_timestamp(observed_at_epoch_ms, &time_standard, device_tz);

    let mut calls_by_date: BTreeMap<jiff::civil::Date, Vec<&crate::UsageCall>> = BTreeMap::new();
    for call in &canonical.calls {
        if let Some(date) =
            crate::civil_date_for_timestamp(call.occurred_at_epoch_ms, &time_standard, device_tz)
        {
            calls_by_date.entry(date).or_default().push(call);
        }
    }

    dates
        .iter()
        .rev()
        .map(|date| {
            let empty: Vec<&crate::UsageCall> = Vec::new();
            let date_calls = calls_by_date.get(date).unwrap_or(&empty);
            let measure = crate::statistics::measure_calls(date_calls.iter().copied(), canonical)
                .map_err(|_| local_statistics_read_error())?;
            Ok(crate::UsageDailyBucket {
                local_date: format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
                in_progress: Some(*date) == observed_date,
                measure,
            })
        })
        .collect()
}

/// 对一组统计度量执行溢出安全的逐字段求和。
pub(crate) fn sum_measures<'a>(
    mut measures: impl Iterator<Item = &'a UsageMeasure>,
) -> Result<UsageMeasure, String> {
    let first = measures
        .next()
        .cloned()
        .ok_or_else(statistics_reconciliation_error)?;
    measures.try_fold(first, |total, measure| {
        total
            .checked_add(measure)
            .map_err(|_| local_statistics_read_error())
    })
}

/// 校验统计度量与规范聚合量在所有可用字段上相等。
pub(crate) fn measure_matches_aggregate(
    measure: &UsageMeasure,
    aggregate: &LocalUsageAggregate,
) -> bool {
    measure.tokens == aggregate.tokens
        && measure.call_count == aggregate.call_count
        && measure.cached_read_call_count == aggregate.cached_read_call_count
        && measure.duplicate_source_count == aggregate.duplicate_source_count
        && measure.cache_read_basis_points == aggregate.cache_read_basis_points
        && measure.confidence == aggregate.confidence
}

/// 根据统计维度和来源元数据生成不泄露路径的分组标签。
fn group_label(
    dimension: UsageDimension,
    key: Option<&str>,
    root_aliases: &BTreeMap<String, String>,
    project_labels: &BTreeMap<String, String>,
    thread_labels: &BTreeMap<String, String>,
) -> Result<(String, DisplayLabelCode), String> {
    let (label, code) = match dimension {
        UsageDimension::Model => key
            .and_then(crate::safe_technical_label)
            .map(|label| (label, DisplayLabelCode::Literal))
            .unwrap_or_else(|| ("未知模型".to_owned(), DisplayLabelCode::UnknownModel)),
        UsageDimension::ReasoningEffort => key.map_or_else(
            || {
                (
                    "未知推理强度".to_owned(),
                    DisplayLabelCode::UnknownReasoningEffort,
                )
            },
            // 与 calls_view/row.rs::reasoning_effort_display_label 里的同一张表
            // 保持逐字一致；新增推理强度取值时两处必须同步修改。
            |value| match value {
                "none" => ("无".to_owned(), DisplayLabelCode::ReasoningNone),
                "minimal" => ("最低".to_owned(), DisplayLabelCode::ReasoningMinimal),
                "low" => ("低".to_owned(), DisplayLabelCode::ReasoningLow),
                "medium" => ("中".to_owned(), DisplayLabelCode::ReasoningMedium),
                "high" => ("高".to_owned(), DisplayLabelCode::ReasoningHigh),
                "xhigh" => ("很高".to_owned(), DisplayLabelCode::ReasoningXHigh),
                other => crate::safe_technical_label(other)
                    .map(|label| (label, DisplayLabelCode::Literal))
                    .unwrap_or_else(|| {
                        (
                            "未知推理强度".to_owned(),
                            DisplayLabelCode::UnknownReasoningEffort,
                        )
                    }),
            },
        ),
        UsageDimension::Project => key.map_or_else(
            || {
                (
                    "未归类项目".to_owned(),
                    DisplayLabelCode::UncategorizedProject,
                )
            },
            |value| {
                project_labels
                    .get(value)
                    .cloned()
                    .map(|label| (label, DisplayLabelCode::Literal))
                    .unwrap_or_else(|| (crate::safe_short_value(value), DisplayLabelCode::Project))
            },
        ),
        UsageDimension::Thread => key.map_or_else(
            || ("未知线程".to_owned(), DisplayLabelCode::UnknownThread),
            |value| {
                thread_labels
                    .get(value)
                    .cloned()
                    .map(|label| (label, DisplayLabelCode::Literal))
                    .unwrap_or_else(|| (crate::safe_short_value(value), DisplayLabelCode::Thread))
            },
        ),
        UsageDimension::Root => key
            .and_then(|value| root_aliases.get(value))
            .and_then(|value| crate::safe_root_label(value))
            .map(|label| (label, DisplayLabelCode::Literal))
            .unwrap_or_else(|| ("未命名数据根".to_owned(), DisplayLabelCode::UnnamedRoot)),
    };
    Ok((label, code))
}

/// 以基点计算分组 Token 占比，总量为零时保持未知。
fn token_share_basis_points(part: u64, total: u64) -> Option<u16> {
    if total == 0 {
        return None;
    }
    let basis_points = u128::from(part) * 10_000 / u128::from(total);
    u16::try_from(basis_points).ok()
}

/// 返回统计分组无法与规范总量对账时的稳定错误。
pub(crate) fn statistics_reconciliation_error() -> String {
    "本机统计无法完成一致性对账；请重试或重新扫描已授权数据根。".to_owned()
}

/// 返回本机统计事务读取失败时的稳定错误。
fn local_statistics_read_error() -> String {
    "无法读取本机统计数据；Agent 客户端原始记录未被修改。".to_owned()
}
