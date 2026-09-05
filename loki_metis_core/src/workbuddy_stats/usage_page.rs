//! 把 WorkBuddy project JSONL 逐请求事实转成 canonical 统计页。

use std::collections::BTreeMap;

use jiff::tz::TimeZone;

use super::record::ValidatedWorkbuddyUsageRecord;
use crate::{
    Confidence, CoverageReport, LocalIndexState, LocalUsageWindow, ProviderKind, SourceProvenance,
    TimeStandard, TokenUsage, UsageCall, UsageDimension, UsageStatisticsPage,
    build_usage_statistics_with_standard, canonicalize_usage_calls, stable_id,
};

/// 把已校验、去重的 WorkBuddy usage 事件转成 canonical 调用。
///
/// 上游当前每条 usage 事件的 `requests` 均为 1；若未来出现聚合事件，为保持
/// canonical `call_count` 精确，会生成一个承载 Token 的调用和若干零 Token 调用。
fn workbuddy_usage_calls(
    records: &[ValidatedWorkbuddyUsageRecord],
    root_id: &str,
    root_alias: &str,
) -> Vec<UsageCall> {
    // 来源标签对整批记录恒定，只拼一次，避免逐来源重复 format!。
    let relative_label = format!("{root_alias}/projects JSONL");
    let mut calls = Vec::new();
    for record in records {
        for request_index in 0..record.request_count {
            let logical_call_id = if request_index == 0 {
                record.logical_call_id.clone()
            } else {
                stable_id(
                    "workbuddy-request",
                    &format!("{}:{request_index}", record.logical_call_id),
                )
            };
            let usage = if request_index == 0 {
                record.usage.clone()
            } else {
                TokenUsage::zero_with_component_availability(true, false, false)
            };
            calls.push(UsageCall {
                logical_call_id,
                occurred_at_epoch_ms: record.occurred_at_epoch_ms,
                model: record.model.clone(),
                reasoning_effort: None,
                project_key: record.project_key.clone(),
                project_label: record.project_label.clone(),
                thread_key: record.session_key.clone(),
                thread_label: None,
                usage,
                adapter_consistency_key: Some(record.logical_call_id.clone()),
                confidence: Confidence::Exact,
                provenance: record
                    .source_ids
                    .iter()
                    .map(|source_id| SourceProvenance {
                        source_id: source_id.clone(),
                        root_id: root_id.to_owned(),
                        relative_label: relative_label.clone(),
                        archived: false,
                    })
                    .collect(),
            });
        }
    }
    calls
}

/// 按窗口与维度装配 WorkBuddy 用量统计页，复用本机 Agent 的日桶与 Top-N 分组。
#[allow(clippy::too_many_arguments)]
pub(super) fn build_workbuddy_usage_statistics(
    records: &[ValidatedWorkbuddyUsageRecord],
    coverage: &CoverageReport,
    root_id: &str,
    root_alias: &str,
    window: LocalUsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<UsageStatisticsPage, String> {
    let calls = workbuddy_usage_calls(records, root_id, root_alias);
    let index_state = if calls.is_empty() {
        LocalIndexState::ReadyNoCalls
    } else {
        LocalIndexState::Ready
    };
    let canonical = canonicalize_usage_calls(calls);
    let mut root_aliases = BTreeMap::new();
    root_aliases.insert(root_id.to_owned(), root_alias.to_owned());
    let source_label = ProviderKind::WorkbuddyProjectJsonl
        .parser_source_label(crate::SourceClientKind::WorkBuddy.parser_version());
    build_usage_statistics_with_standard(
        &canonical,
        &root_aliases,
        index_state,
        coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        ProviderKind::WorkbuddyProjectJsonl,
        Some(source_label.as_str()),
        time_standard.clone(),
        device_tz,
    )
}
