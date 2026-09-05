use std::collections::BTreeMap;

use jiff::{civil::Date, tz::TimeZone};

use crate::{
    Completeness, Confidence, CoverageReport, CoverageState, Freshness, LocalIndexState,
    ProviderKind, SourceProvenance, TokenUsage, UsageCall, UsageDimension, build_usage_statistics,
    canonicalize_usage_calls,
};

/// 将测试民用日期时间转换为本地毫秒时间戳。
fn local_epoch_ms(year: i16, month: i8, day: i8, hour: i8, minute: i8, second: i8) -> i64 {
    Date::new(year, month, day)
        .expect("test date is valid")
        .at(hour, minute, second, 0)
        .to_zoned(TimeZone::system())
        .expect("test instant is representable")
        .timestamp()
        .as_millisecond()
}

/// 构造带模型、数据根和时间的统计测试调用。
fn call(id: &str, timestamp: i64, model: &str, root_id: &str, input: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(),
        occurred_at_epoch_ms: timestamp,
        model: Some(model.to_owned()),
        reasoning_effort: Some("high".to_owned()),
        project_key: Some(format!("project-{id}")),
        thread_key: format!("thread-{id}"),
        project_label: None,
        thread_label: None,
        usage: TokenUsage::new(input, input / 2, Some(1), 10, 2, Some(input + 10))
            .expect("synthetic usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{id}"),
            root_id: root_id.to_owned(),
            relative_label: format!("sessions/{id}.jsonl"),
            archived: false,
        }],
    }
}

/// 返回统计测试使用的完整覆盖报告。
fn coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Partial,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

#[test]
/// 验证未扫描统计保持未知而不会伪装成精确零值。
fn not_scanned_statistics_are_marked_unknown_instead_of_exact_zero() {
    let observed = local_epoch_ms(2026, 7, 31, 12, 0, 0);
    let canonical = canonicalize_usage_calls(Vec::new());

    let statistics = build_usage_statistics(
        &canonical,
        &BTreeMap::new(),
        LocalIndexState::NotScanned,
        &coverage(),
        crate::LocalUsageWindow::ThisWeek,
        UsageDimension::Model,
        observed,
        ProviderKind::RolloutJsonl,
        Some("rollout-parser-v1"),
    )
    .expect("not-scanned response keeps a stable shape");

    assert_eq!(statistics.index_state, LocalIndexState::NotScanned);
    assert_eq!(statistics.fact.freshness, Freshness::Unknown);
    assert_eq!(statistics.fact.completeness, Completeness::Unknown);
    assert_eq!(statistics.fact.confidence, Confidence::Derived);
    assert_eq!(statistics.fact.value.call_count, 0);
    assert_eq!(statistics.daily_buckets.len(), 5);
}

#[test]
/// 验证本周统计完整对账且排除下一个本地自然日。
fn this_week_statistics_reconcile_and_exclude_next_local_day() {
    let observed = local_epoch_ms(2026, 7, 31, 12, 0, 0);
    let yesterday = local_epoch_ms(2026, 7, 30, 12, 0, 0);
    let today_late = local_epoch_ms(2026, 7, 31, 23, 59, 0);
    let next_day = local_epoch_ms(2026, 8, 1, 0, 1, 0);
    let canonical = canonicalize_usage_calls(vec![
        call("today", observed, "model-a", "root-a", 100),
        call("today-late", today_late, "model-a", "root-a", 20),
        call("yesterday", yesterday, "model-b", "root-a", 50),
        call("next-day", next_day, "model-c", "root-a", 500),
    ]);

    let statistics = build_usage_statistics(
        &canonical,
        &BTreeMap::new(),
        LocalIndexState::Ready,
        &coverage(),
        crate::LocalUsageWindow::ThisWeek,
        UsageDimension::Model,
        observed,
        ProviderKind::RolloutJsonl,
        Some("rollout-parser-v1"),
    )
    .expect("statistics reconcile");

    assert_eq!(statistics.daily_buckets.len(), 5);
    assert_eq!(statistics.fact.value.call_count, 3);
    assert_eq!(statistics.fact.value.tokens.input_tokens, 170);
    assert_eq!(statistics.groups.len(), 2);
    assert!(statistics.remainder.is_none());
    assert_eq!(
        statistics
            .daily_buckets
            .iter()
            .filter(|bucket| bucket.measure.call_count == 0)
            .count(),
        3
    );
    let first = statistics
        .daily_buckets
        .first()
        .expect("this-week window has a newest bucket");
    let last = statistics
        .daily_buckets
        .last()
        .expect("this-week window has an oldest bucket");
    assert!(
        first.local_date > last.local_date,
        "daily trend must list newest local date first"
    );
    assert!(first.in_progress);
    assert!(
        statistics
            .daily_buckets
            .iter()
            .skip(1)
            .all(|day| !day.in_progress)
    );
}

/// 昨天窗口只生成前一民用日的一个日桶，且合计与该日调用一致。
#[test]
fn yesterday_statistics_use_one_prior_civil_day_bucket() {
    let observed = local_epoch_ms(2026, 7, 31, 12, 0, 0);
    let yesterday = local_epoch_ms(2026, 7, 30, 12, 0, 0);
    let yesterday_late = local_epoch_ms(2026, 7, 30, 23, 59, 0);
    let day_before = local_epoch_ms(2026, 7, 29, 23, 59, 0);
    let today_late = local_epoch_ms(2026, 7, 31, 23, 59, 0);
    let next_day = local_epoch_ms(2026, 8, 1, 0, 1, 0);
    let canonical = canonicalize_usage_calls(vec![
        call("today", observed, "model-a", "root-a", 100),
        call("today-late", today_late, "model-a", "root-a", 20),
        call("yesterday", yesterday, "model-b", "root-a", 50),
        call("yesterday-late", yesterday_late, "model-b", "root-a", 30),
        call("day-before", day_before, "model-c", "root-a", 200),
        call("next-day", next_day, "model-c", "root-a", 500),
    ]);

    let statistics = build_usage_statistics(
        &canonical,
        &BTreeMap::new(),
        LocalIndexState::Ready,
        &coverage(),
        crate::LocalUsageWindow::Yesterday,
        UsageDimension::Model,
        observed,
        ProviderKind::RolloutJsonl,
        Some("rollout-parser-v1"),
    )
    .expect("yesterday statistics reconcile");

    assert_eq!(statistics.daily_buckets.len(), 1);
    assert_eq!(statistics.daily_buckets[0].local_date, "2026-07-30");
    assert!(!statistics.daily_buckets[0].in_progress);
    assert_eq!(statistics.fact.value.call_count, 2);
    assert_eq!(statistics.fact.value.tokens.input_tokens, 80);
    assert_eq!(
        statistics.fact.value.tokens.total_tokens,
        statistics.daily_buckets[0].measure.tokens.total_tokens
    );
}
