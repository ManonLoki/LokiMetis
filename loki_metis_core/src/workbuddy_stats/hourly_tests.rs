use super::*;
use crate::workbuddy_stats::{
    WorkbuddyUsageEventRecord, WorkbuddyUsageOrigin, complete_workbuddy_coverage,
    compute_workbuddy_statistics_with_standard,
};

/// 固定观测时刻 2026-07-27T12:00:00Z。
const OBSERVED_EPOCH_MS: i64 = 1_785_153_600_000;
/// 当天 UTC 10 时的事件。
const TODAY_10H_MS: i64 = 1_785_148_280_179;
/// 前一天 UTC 23:30 的事件。
const YESTERDAY_23H_MS: i64 = 1_785_108_600_000;

/// 构造小时趋势使用的请求事件。
fn usage(
    id: &str,
    session: &str,
    timestamp: i64,
    tokens: i64,
    credit: Option<f64>,
    origin: WorkbuddyUsageOrigin,
) -> WorkbuddyUsageEventRecord {
    WorkbuddyUsageEventRecord {
        logical_call_id: id.to_owned(),
        session_key: session.to_owned(),
        source_id: format!("source-{id}"),
        occurred_at_epoch_ms: timestamp,
        model: Some("model".to_owned()),
        project_key: None,
        project_label: None,
        request_count: 1,
        input_tokens: tokens,
        cached_input_tokens: 0,
        output_tokens: 0,
        total_tokens: tokens,
        credit,
        origin,
    }
}

/// 无事件时仍输出今日与昨日各 24 个桶，并只标记当前小时。
#[test]
fn empty_usage_still_produces_fixed_hour_buckets() {
    let snapshot = compute_workbuddy_statistics_with_standard(
        &[],
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &TimeStandard::utc(),
        &TimeZone::UTC,
    );

    assert_eq!(snapshot.hourly_trends.len(), 2);
    let today = &snapshot.hourly_trends[0];
    assert_eq!(today.window, LocalUsageWindow::Today);
    assert_eq!(today.date, "2026-07-27");
    assert_eq!(today.buckets.len(), 24);
    assert!(today.buckets[12].in_progress);
    assert_eq!(today.buckets[0].credits, Some(0.0));
    assert!(today.buckets.iter().all(|bucket| bucket.request_count == 0));
    assert!(
        snapshot.hourly_trends[1]
            .buckets
            .iter()
            .all(|bucket| !bucket.in_progress)
    );
}

/// 请求按事件小时累计；会话在同一小时去重，顶层和 subagent 请求分别保留。
#[test]
fn events_fold_by_hour_with_sessions_and_origins() {
    let records = vec![
        usage(
            "a",
            "same",
            TODAY_10H_MS,
            100,
            Some(1.5),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "b",
            "same",
            TODAY_10H_MS + 60_000,
            50,
            Some(0.5),
            WorkbuddyUsageOrigin::Subagent,
        ),
        usage(
            "c",
            "other",
            YESTERDAY_23H_MS,
            7,
            Some(0.25),
            WorkbuddyUsageOrigin::TopLevel,
        ),
    ];
    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &TimeStandard::utc(),
        &TimeZone::UTC,
    );

    let hour = &snapshot.hourly_trends[0].buckets[10];
    assert_eq!(hour.session_count, 1);
    assert_eq!(hour.request_count, 2);
    assert_eq!(hour.top_level_request_count, 1);
    assert_eq!(hour.subagent_request_count, 1);
    assert_eq!(hour.tokens, 150);
    assert_eq!(hour.credits, Some(2.0));
    let yesterday = &snapshot.hourly_trends[1].buckets[23];
    assert_eq!(yesterday.request_count, 1);
    assert_eq!(yesterday.tokens, 7);
}

/// 小时请求与 Token 总和必须和同一日窗口精确对账。
#[test]
fn hourly_totals_reconcile_with_window_totals() {
    let records = vec![
        usage(
            "a",
            "one",
            TODAY_10H_MS,
            40,
            Some(1.0),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "b",
            "two",
            TODAY_10H_MS + 3 * 60 * 60 * 1_000,
            60,
            Some(2.0),
            WorkbuddyUsageOrigin::Subagent,
        ),
    ];
    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &TimeStandard::utc(),
        &TimeZone::UTC,
    );
    let today = snapshot
        .windows
        .iter()
        .find(|window| window.window == LocalUsageWindow::Today)
        .expect("today window exists");
    let buckets = &snapshot.hourly_trends[0].buckets;

    assert_eq!(
        buckets
            .iter()
            .map(|bucket| bucket.request_count)
            .sum::<u64>(),
        today.request_count
    );
    assert_eq!(
        buckets.iter().map(|bucket| bucket.tokens).sum::<u64>(),
        today.tokens
    );
}

/// 当地时间标准改变时，小时归属跟随设备时区。
#[test]
fn hour_membership_follows_viewing_time_zone() {
    let device_tz = TimeZone::get("Asia/Shanghai").expect("timezone exists");
    let records = vec![usage(
        "late",
        "s",
        YESTERDAY_23H_MS,
        7,
        Some(0.25),
        WorkbuddyUsageOrigin::TopLevel,
    )];
    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &TimeStandard::Local,
        &device_tz,
    );

    assert_eq!(snapshot.hourly_trends[0].buckets[7].request_count, 1);
    assert!(
        snapshot.hourly_trends[1]
            .buckets
            .iter()
            .all(|bucket| bucket.request_count == 0)
    );
}
