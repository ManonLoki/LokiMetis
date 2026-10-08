//! 图表时间桶、时区和维度对账回归。

use std::collections::BTreeMap;

use jiff::{Timestamp, tz::TimeZone};

use super::*;
use crate::{
    Confidence, CoverageState, SourceProvenance, TokenUsage, UsageCall, canonicalize_usage_calls,
    empty_token_usage_for_provider,
};

/// 把 RFC 3339 测试时间转换为毫秒时间戳。
fn epoch_ms(value: &str) -> i64 {
    value
        .parse::<Timestamp>()
        .expect("fixture timestamp is valid")
        .as_millisecond()
}

/// 返回图表测试使用的完整覆盖报告。
fn coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

/// 构造带时间、模型和输入量的图表测试调用。
fn call(id: &str, occurred_at_epoch_ms: i64, model: &str, input: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(),
        occurred_at_epoch_ms,
        model: Some(model.to_owned()),
        reasoning_effort: Some("high".to_owned()),
        project_key: Some(format!("project-{id}")),
        project_label: Some(format!("project-{id}")),
        thread_key: format!("thread-{id}"),
        thread_label: None,
        usage: TokenUsage::new(input, input / 2, Some(1), 5, 1, Some(input + 10))
            .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{id}"),
            root_id: "root-a".to_owned(),
            relative_label: format!("sessions/{id}.jsonl"),
            archived: false,
        }],
    }
}

/// 使用固定查询参数构造图表测试结果。
fn chart(
    calls: Vec<UsageCall>,
    window: LocalUsageWindow,
    dimension: UsageChartDimension,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
    device_tz: &TimeZone,
) -> Result<UsageChartPage, String> {
    let mut canonical = canonicalize_usage_calls(calls);
    canonical.empty_tokens = empty_token_usage_for_provider(ProviderKind::RolloutJsonl);
    canonical.total_token_accounting = ProviderKind::RolloutJsonl.local_total_token_accounting();
    build_usage_chart_with_standard(
        &canonical,
        &BTreeMap::new(),
        LocalIndexState::Ready,
        &coverage(),
        window,
        dimension,
        observed_at_epoch_ms,
        ProviderKind::RolloutJsonl,
        Some("rollout-parser-v8"),
        time_standard,
        device_tz,
    )
}

#[test]
/// 验证当天与昨天始终返回顺序固定的 24 个民用小时桶。
fn today_and_yesterday_are_exactly_twenty_four_ordered_hours() {
    let observed = epoch_ms("2026-08-23T12:30:00Z");
    let today = chart(
        vec![
            call("midnight", epoch_ms("2026-08-23T00:01:00Z"), "a", 10),
            call("late", epoch_ms("2026-08-23T23:59:00Z"), "a", 20),
            call("prior", epoch_ms("2026-08-22T10:00:00Z"), "b", 50),
        ],
        LocalUsageWindow::Today,
        UsageChartDimension::Model,
        observed,
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("today chart reconciles");

    assert_eq!(today.granularity, UsageChartGranularity::Hour);
    assert_eq!(today.buckets.len(), 24);
    assert_eq!(today.buckets[0].key, "2026-08-23T00");
    assert_eq!(today.buckets[23].label, "23:00");
    assert_eq!(today.buckets[0].measure.call_count, 1);
    assert_eq!(today.buckets[23].measure.call_count, 1);
    assert!(today.buckets[12].in_progress);
    assert_eq!(today.fact.value.call_count, 2);

    let yesterday = chart(
        vec![
            call("today", epoch_ms("2026-08-23T00:01:00Z"), "a", 10),
            call("prior", epoch_ms("2026-08-22T10:00:00Z"), "b", 50),
            call("older", epoch_ms("2026-08-21T10:00:00Z"), "c", 70),
        ],
        LocalUsageWindow::Yesterday,
        UsageChartDimension::Model,
        observed,
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("yesterday chart reconciles");
    assert_eq!(yesterday.buckets.len(), 24);
    assert_eq!(yesterday.buckets[0].key, "2026-08-22T00");
    assert_eq!(yesterday.fact.value.call_count, 1);
    assert!(yesterday.buckets.iter().all(|bucket| !bucket.in_progress));
}

#[test]
/// 验证本周/本月按从旧到新输出完整自然日桶，且不含窗口外日期。
fn multi_day_windows_are_oldest_to_newest_complete_civil_days() {
    let observed = epoch_ms("2026-08-23T12:30:00Z");
    for (window, expected_count, first, expected_calls) in [
        (LocalUsageWindow::ThisWeek, 7, "2026-08-17", 2),
        (LocalUsageWindow::ThisMonth, 23, "2026-08-01", 2),
        (LocalUsageWindow::LastWeek, 7, "2026-08-10", 1),
        (LocalUsageWindow::LastMonth, 31, "2026-07-01", 1),
    ] {
        let page = chart(
            vec![
                call("oldest", epoch_ms(&format!("{first}T01:00:00Z")), "a", 10),
                call("today", epoch_ms("2026-08-23T11:00:00Z"), "b", 20),
                call("future", epoch_ms("2026-08-24T00:00:00Z"), "c", 30),
            ],
            window,
            UsageChartDimension::Model,
            observed,
            TimeStandard::utc(),
            &TimeZone::UTC,
        )
        .expect("multi-day chart reconciles");
        assert_eq!(page.granularity, UsageChartGranularity::Day);
        assert_eq!(page.buckets.len(), expected_count);
        assert_eq!(page.buckets[0].key, first);
        let expected_last = match window {
            LocalUsageWindow::LastWeek => "2026-08-16",
            LocalUsageWindow::LastMonth => "2026-07-31",
            _ => "2026-08-23",
        };
        assert_eq!(
            page.buckets.last().map(|bucket| bucket.key.as_str()),
            Some(expected_last)
        );
        assert!(
            page.buckets
                .windows(2)
                .all(|pair| pair[0].key < pair[1].key)
        );
        assert_eq!(page.fact.value.call_count, expected_calls);
    }
}

#[test]
/// 验证 DST 春季缺失小时补零、秋季重复小时合并。
fn spring_gap_stays_zero_and_fall_fold_merges_into_one_civil_hour() {
    let new_york = TimeZone::get("America/New_York").expect("IANA fixture exists");
    let spring = chart(
        vec![
            call("before-gap", epoch_ms("2026-03-08T06:30:00Z"), "a", 10),
            call("after-gap", epoch_ms("2026-03-08T07:30:00Z"), "a", 20),
        ],
        LocalUsageWindow::Today,
        UsageChartDimension::Model,
        epoch_ms("2026-03-08T16:00:00Z"),
        TimeStandard::local(),
        &new_york,
    )
    .expect("spring chart reconciles");
    assert_eq!(spring.buckets[1].measure.call_count, 1);
    assert_eq!(spring.buckets[2].measure.call_count, 0);
    assert_eq!(spring.buckets[3].measure.call_count, 1);

    let fall = chart(
        vec![
            call("first-fold", epoch_ms("2026-11-01T05:30:00Z"), "a", 10),
            call("second-fold", epoch_ms("2026-11-01T06:30:00Z"), "a", 20),
        ],
        LocalUsageWindow::Today,
        UsageChartDimension::Model,
        epoch_ms("2026-11-01T17:00:00Z"),
        TimeStandard::local(),
        &new_york,
    )
    .expect("fall chart reconciles");
    assert_eq!(fall.buckets[1].measure.call_count, 2);
    assert_eq!(fall.fact.value.call_count, 2);
}

#[test]
/// 验证固定分布维度保持 Top 10 加其余项且逐字段对账。
fn fixed_dimensions_keep_top_ten_and_reconciled_remainder() {
    let observed = epoch_ms("2026-08-23T12:30:00Z");
    let calls = (0..12)
        .map(|index| {
            call(
                &format!("call-{index}"),
                epoch_ms("2026-08-23T10:00:00Z"),
                &format!("model-{index:02}"),
                100 - index,
            )
        })
        .collect();
    let page = chart(
        calls,
        LocalUsageWindow::Today,
        UsageChartDimension::Model,
        observed,
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("dimension chart reconciles");
    assert_eq!(page.groups.len(), 10);
    let remainder = page.remainder.expect("two groups become remainder");
    assert_eq!(remainder.measure.call_count, 2);
    let displayed_calls = page
        .groups
        .iter()
        .map(|group| group.measure.call_count)
        .sum::<u64>()
        + remainder.measure.call_count;
    assert_eq!(displayed_calls, page.fact.value.call_count);
}

#[test]
/// 验证联合 Agent 维度按来源分组，并拒绝缺失物理来源的调用。
fn agent_dimension_groups_combined_calls_and_rejects_missing_origin() {
    let observed = epoch_ms("2026-08-23T12:30:00Z");
    let codex = call("codex", epoch_ms("2026-08-23T10:00:00Z"), "a", 10);
    let mut claude = call("claude", epoch_ms("2026-08-23T11:00:00Z"), "b", 20);
    claude.usage = TokenUsage::new_with_availability(20, Some(10), Some(1), 5, None, Some(30))
        .expect("Claude fixture omits reasoning tokens");
    let mut canonical = canonicalize_usage_calls(vec![codex, claude]);
    canonical.empty_tokens = TokenUsage::zero_with_component_availability(true, true, false);
    let origins = BTreeMap::from([
        ("codex".to_owned(), SourceClientKind::Codex),
        ("claude".to_owned(), SourceClientKind::ClaudeCode),
    ]);
    let page = build_combined_chart_with_standard(
        &canonical,
        &BTreeMap::new(),
        &origins,
        LocalIndexState::Ready,
        &coverage(),
        LocalUsageWindow::Today,
        UsageChartDimension::Agent,
        observed,
        Some("combined-local-agents-v1"),
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("agent chart reconciles");
    assert_eq!(page.groups.len(), 2);
    assert_eq!(page.groups[0].label, "Claude Code");
    assert_eq!(page.groups[1].label, "Codex");
    assert!(page.remainder.is_none());

    let missing = build_combined_chart_with_standard(
        &canonical,
        &BTreeMap::new(),
        &BTreeMap::new(),
        LocalIndexState::Ready,
        &coverage(),
        LocalUsageWindow::Today,
        UsageChartDimension::Agent,
        observed,
        Some("combined-local-agents-v1"),
        TimeStandard::utc(),
        &TimeZone::UTC,
    );
    assert_eq!(missing, Err(chart_read_error()));
}

#[test]
/// 验证物理 Agent 视图不能请求仅供“全部”使用的 Agent 维度。
fn physical_agent_dimension_is_rejected_in_core() {
    let result = chart(
        Vec::new(),
        LocalUsageWindow::Today,
        UsageChartDimension::Agent,
        epoch_ms("2026-08-23T12:30:00Z"),
        TimeStandard::utc(),
        &TimeZone::UTC,
    );
    assert_eq!(result, Err(chart_dimension_error()));
}
