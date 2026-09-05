//! 把 WorkBuddy project JSONL 的逐请求事实折叠成六个按模型日历窗口。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::WorkbuddyUsageOrigin;
use super::record::ValidatedWorkbuddyUsageRecord;
use crate::{LocalUsageWindow, TimeStandard, civil_date_for_timestamp, inclusive_calendar_range};

/// 单个实际模型（或未提供模型）的精确逐请求用量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyModelUsageGroup {
    /// 安全实际模型名；`None` 表示上游缺失或名称未通过安全校验。
    pub model: Option<String>,
    /// 上游请求数合计。
    pub call_count: u64,
    /// 输入与输出之和；缓存输入不重复加入。
    pub total_tokens: u64,
    /// 全部输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: u64,
    /// 输入 Token 中未命中缓存的部分。
    pub uncached_input_tokens: u64,
    /// 输出 Token。
    pub output_tokens: u64,
    /// 逐请求积分合计；任一记录缺失时保持未提供。
    pub credits: Option<f64>,
    /// 顶层会话请求数。
    pub top_level_call_count: u64,
    /// subagent 请求数。
    pub subagent_call_count: u64,
}

/// 单个日历窗口内的 WorkBuddy 逐模型精确用量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyModelUsageWindow {
    /// 六个固定日历窗口之一。
    pub window: LocalUsageWindow,
    /// 按总 Token 降序、同量时按安全模型名稳定排序的明细行。
    pub groups: Vec<WorkbuddyModelUsageGroup>,
}

/// 只聚合调用方请求的那个窗口；日历范围无法解析时返回 None。
pub(super) fn build_model_usage_window(
    records: &[ValidatedWorkbuddyUsageRecord],
    window: LocalUsageWindow,
    today: Date,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Option<WorkbuddyModelUsageWindow> {
    let (from, to) = inclusive_calendar_range(window, today).ok()?;
    let mut groups = BTreeMap::<Option<&str>, WorkbuddyModelUsageGroup>::new();
    for record in records {
        let Some(date) =
            civil_date_for_timestamp(record.occurred_at_epoch_ms, time_standard, device_tz)
        else {
            continue;
        };
        if date < from || date > to {
            continue;
        }
        let call_count = u64::from(record.request_count);
        let group =
            groups
                .entry(record.model.as_deref())
                .or_insert_with(|| WorkbuddyModelUsageGroup {
                    model: record.model.clone(),
                    call_count: 0,
                    total_tokens: 0,
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    uncached_input_tokens: 0,
                    output_tokens: 0,
                    credits: Some(0.0),
                    top_level_call_count: 0,
                    subagent_call_count: 0,
                });
        group.call_count = group.call_count.saturating_add(call_count);
        group.total_tokens = group.total_tokens.saturating_add(record.usage.total_tokens);
        group.input_tokens = group.input_tokens.saturating_add(record.usage.input_tokens);
        group.cached_input_tokens = group
            .cached_input_tokens
            .saturating_add(record.usage.cached_input_tokens.unwrap_or_default());
        group.uncached_input_tokens = group
            .uncached_input_tokens
            .saturating_add(record.usage.uncached_input_tokens().unwrap_or_default());
        group.output_tokens = group
            .output_tokens
            .saturating_add(record.usage.output_tokens);
        group.credits = sum_optional_credit(group.credits, record.credit);
        match record.origin {
            WorkbuddyUsageOrigin::TopLevel => {
                group.top_level_call_count = group.top_level_call_count.saturating_add(call_count);
            }
            WorkbuddyUsageOrigin::Subagent => {
                group.subagent_call_count = group.subagent_call_count.saturating_add(call_count);
            }
        }
    }

    let mut groups: Vec<WorkbuddyModelUsageGroup> = groups.into_values().collect();
    groups.sort_by(compare_groups);
    Some(WorkbuddyModelUsageWindow { window, groups })
}

/// 只有两侧积分都可用时才相加，避免把部分积分误报为完整。
pub(super) fn sum_optional_credit(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    left.zip(right)
        .map(|(left, right)| left + right)
        .filter(|sum| sum.is_finite())
}

/// 总量高的行在前；同量时命名模型按字典序排列，未归属行稳定放在其后。
fn compare_groups(left: &WorkbuddyModelUsageGroup, right: &WorkbuddyModelUsageGroup) -> Ordering {
    right
        .total_tokens
        .cmp(&left.total_tokens)
        .then_with(|| match (&left.model, &right.model) {
            (Some(left), Some(right)) => left.cmp(right),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        })
}

#[cfg(test)]
#[path = "model_usage_tests.rs"]
mod tests;
