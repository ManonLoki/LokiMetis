//! 本文件的测试覆盖：固定观测时刻 2026-07-27T12:00:00Z；观测日 10 时内的事件时刻；构造一条已投影的 WorkBuddy usage 事件。

use super::*;

/// 固定观测时刻 2026-07-27T12:00:00Z。
const OBSERVED_EPOCH_MS: i64 = 1_785_153_600_000;
/// 观测日 10 时内的事件时刻。
const TODAY_EPOCH_MS: i64 = 1_785_148_280_179;

/// 构造一条已投影的 WorkBuddy usage 事件。
#[allow(clippy::too_many_arguments)]
fn usage(
    id: &str,
    session: &str,
    occurred_at_epoch_ms: i64,
    model: Option<&str>,
    origin: WorkbuddyUsageOrigin,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    credit: Option<f64>,
) -> WorkbuddyUsageEventRecord {
    WorkbuddyUsageEventRecord {
        logical_call_id: id.to_owned(),
        session_key: session.to_owned(),
        source_id: format!("source-{id}"),
        root_id: "workbuddy-root-test".to_owned(),
        occurred_at_epoch_ms,
        model: model.map(ToOwned::to_owned),
        project_key: Some("project-safe".to_owned()),
        project_label: Some("dashboard".to_owned()),
        request_count: 1,
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        credit,
        origin,
    }
}

/// 构造只参与健康诊断的 trace。
fn trace(status: WorkbuddyTraceStatus, duration_ms: i64) -> WorkbuddyTraceRecord {
    WorkbuddyTraceRecord {
        started_at_epoch_ms: TODAY_EPOCH_MS,
        duration_ms,
        status,
    }
}

/// 空来源仍返回完整的零值快照与固定小时窗口。
#[test]
fn empty_inputs_produce_zeroed_usage() {
    let snapshot = compute_workbuddy_statistics(&[], &[], OBSERVED_EPOCH_MS);

    assert_eq!(snapshot.total_sessions, 0);
    assert_eq!(snapshot.total_requests, 0);
    assert_eq!(snapshot.total_input_tokens, 0);
    assert_eq!(snapshot.total_cached_input_tokens, 0);
    assert_eq!(snapshot.total_output_tokens, 0);
    assert_eq!(snapshot.total_tokens, 0);
    assert_eq!(snapshot.total_credits, Some(0.0));
    assert_eq!(snapshot.hourly_trends.len(), 2);
    assert_eq!(snapshot.generated_at_epoch_ms, OBSERVED_EPOCH_MS);
}

/// 每个事件必须按自身时间、实际模型与来源层级累加，而不是按会话模型或创建日。
#[test]
fn usage_events_drive_exact_totals_dates_and_origins() {
    let yesterday = TODAY_EPOCH_MS - 24 * 60 * 60 * 1_000;
    let records = vec![
        usage(
            "a",
            "session-one",
            TODAY_EPOCH_MS,
            Some("minimax-m3"),
            WorkbuddyUsageOrigin::TopLevel,
            100,
            80,
            20,
            Some(1.5),
        ),
        usage(
            "b",
            "session-one",
            TODAY_EPOCH_MS + 1_000,
            Some("deepseek-v4-flash"),
            WorkbuddyUsageOrigin::Subagent,
            50,
            20,
            10,
            Some(0.5),
        ),
        usage(
            "c",
            "session-two",
            yesterday,
            Some("minimax-m3"),
            WorkbuddyUsageOrigin::TopLevel,
            25,
            5,
            5,
            Some(0.25),
        ),
    ];

    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );

    assert_eq!(snapshot.total_sessions, 2);
    assert_eq!(snapshot.total_requests, 3);
    assert_eq!(snapshot.top_level_requests, 2);
    assert_eq!(snapshot.subagent_requests, 1);
    assert_eq!(snapshot.total_input_tokens, 175);
    assert_eq!(snapshot.total_cached_input_tokens, 105);
    assert_eq!(snapshot.total_uncached_input_tokens, 70);
    assert_eq!(snapshot.total_output_tokens, 35);
    assert_eq!(snapshot.total_tokens, 210);
    assert_eq!(snapshot.total_credits, Some(2.25));
    assert_eq!(snapshot.average_session_duration_seconds, 0.5);

    let today = snapshot
        .windows
        .iter()
        .find(|window| window.window == crate::LocalUsageWindow::Today)
        .expect("today exists");
    assert_eq!(today.session_count, 1);
    assert_eq!(today.request_count, 2);
    assert_eq!(today.top_level_request_count, 1);
    assert_eq!(today.subagent_request_count, 1);
    assert_eq!(today.input_tokens, 150);
    assert_eq!(today.cached_input_tokens, 100);
    assert_eq!(today.uncached_input_tokens, 50);
    assert_eq!(today.output_tokens, 30);
    assert_eq!(today.tokens, 180);
}

/// 完全重复观察只算一次；同 ID 冲突事实整组排除并把覆盖降为 Partial。
#[test]
fn duplicates_fold_and_conflicts_fail_closed() {
    let exact = usage(
        "exact",
        "s1",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        10,
        5,
        2,
        Some(0.1),
    );
    let mut exact_copy = exact.clone();
    exact_copy.source_id = "source-copy".to_owned();
    let conflict = usage(
        "conflict",
        "s2",
        TODAY_EPOCH_MS,
        Some("model-b"),
        WorkbuddyUsageOrigin::TopLevel,
        20,
        10,
        3,
        Some(0.2),
    );
    let mut conflict_copy = conflict.clone();
    conflict_copy.output_tokens = 4;
    conflict_copy.total_tokens = 24;
    let records = vec![exact, exact_copy, conflict, conflict_copy];

    let quality = record::workbuddy_usage_quality(&records);
    assert_eq!(quality.duplicate_record_count, 1);
    assert_eq!(quality.conflicting_duplicate_record_count, 2);
    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 12);
    assert_eq!(snapshot.coverage.state, crate::CoverageState::Partial);
    assert_eq!(snapshot.coverage.warning_count, 2);
}

/// 同文件同调用的晚到用量可修正输出分量，但只把总量递增的最终事实计一次。
#[test]
fn later_workbuddy_usage_observation_replaces_preliminary_totals() {
    let first = usage(
        "updated-call",
        "same-session",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        88_551,
        88_320,
        5_061,
        Some(0.0),
    );
    let mut final_observation = first.clone();
    final_observation.occurred_at_epoch_ms += 9_625;
    final_observation.input_tokens = 93_805;
    final_observation.cached_input_tokens = 93_568;
    final_observation.output_tokens = 2_525;
    final_observation.total_tokens = 96_330;
    let records = [final_observation, first];

    let quality = record::workbuddy_usage_quality(&records);
    assert_eq!(quality.duplicate_record_count, 1);
    assert_eq!(quality.conflicting_duplicate_record_count, 0);
    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_input_tokens, 93_805);
    assert_eq!(snapshot.total_cached_input_tokens, 93_568);
    assert_eq!(snapshot.total_output_tokens, 2_525);
    assert_eq!(snapshot.total_tokens, 96_330);
    assert_eq!(snapshot.coverage.state, crate::CoverageState::Complete);
}

/// 晚到的完全相同用量只保留一次，并使用最后的观察时刻。
#[test]
fn later_identical_workbuddy_observation_is_one_call() {
    let first = usage(
        "same-call",
        "same-session",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        100,
        50,
        20,
        Some(0.1),
    );
    let mut later = first.clone();
    later.occurred_at_epoch_ms += 1_000;
    let records = [later.clone(), first];

    let (prepared, quality) = record::prepare_workbuddy_usage_records(&records);
    assert_eq!(quality.duplicate_record_count, 1);
    assert_eq!(quality.conflicting_duplicate_record_count, 0);
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].occurred_at_epoch_ms, later.occurred_at_epoch_ms);
    assert_eq!(prepared[0].usage.total_tokens, 120);
}

/// Token 不变时允许同源晚到积分由缺失变为已知，最终积分只计一次。
#[test]
fn later_workbuddy_credit_enrichment_keeps_tokens_once() {
    let first = usage(
        "credit-call",
        "same-session",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        100,
        50,
        20,
        None,
    );
    let mut later = first.clone();
    later.occurred_at_epoch_ms += 1_000;
    later.credit = Some(0.25);
    let records = [later, first];

    let snapshot = compute_workbuddy_statistics_with_standard(
        &records,
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 120);
    assert_eq!(snapshot.total_credits, Some(0.25));
    assert_eq!(snapshot.coverage.state, crate::CoverageState::Complete);
}

/// 时间较晚仍不足以覆盖回退、等总量分量变化、换模型或异源冲突。
#[test]
fn later_conflicting_workbuddy_observations_still_fail_closed() {
    let first = usage(
        "conflicting-call",
        "same-session",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        100,
        50,
        20,
        Some(0.1),
    );
    let mut lower_total = first.clone();
    lower_total.occurred_at_epoch_ms += 1_000;
    lower_total.input_tokens = 90;
    lower_total.total_tokens = 110;
    let mut same_total_changed_components = first.clone();
    same_total_changed_components.occurred_at_epoch_ms += 1_000;
    same_total_changed_components.input_tokens = 101;
    same_total_changed_components.output_tokens = 19;
    let mut changed_model = first.clone();
    changed_model.occurred_at_epoch_ms += 1_000;
    changed_model.input_tokens = 110;
    changed_model.total_tokens = 130;
    changed_model.model = Some("model-b".to_owned());
    let mut different_source = first.clone();
    different_source.occurred_at_epoch_ms += 1_000;
    different_source.source_id = "another-source".to_owned();
    let mut changed_request_count = first.clone();
    changed_request_count.occurred_at_epoch_ms += 1_000;
    changed_request_count.request_count = 2;
    let mut lower_credit = first.clone();
    lower_credit.occurred_at_epoch_ms += 1_000;
    lower_credit.credit = Some(0.05);

    for conflicting in [
        lower_total,
        same_total_changed_components,
        changed_model,
        different_source,
        changed_request_count,
        lower_credit,
    ] {
        let records = [first.clone(), conflicting];
        let quality = record::workbuddy_usage_quality(&records);
        assert_eq!(quality.conflicting_duplicate_record_count, 2);
        assert_eq!(quality.duplicate_record_count, 0);
        assert!(
            record::prepare_workbuddy_usage_records(&records)
                .0
                .is_empty()
        );
    }
}

/// 缓存违反输入子集关系时整条排除；缺积分不影响 Token 但积分保持不可用。
#[test]
fn invalid_tokens_are_omitted_and_missing_credit_stays_unknown() {
    let valid = usage(
        "valid",
        "s1",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        10,
        4,
        2,
        None,
    );
    let invalid = usage(
        "invalid",
        "s2",
        TODAY_EPOCH_MS,
        Some("model-b"),
        WorkbuddyUsageOrigin::TopLevel,
        5,
        6,
        1,
        Some(1.0),
    );
    let snapshot = compute_workbuddy_statistics_with_standard(
        &[valid, invalid],
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );

    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 12);
    assert_eq!(snapshot.total_credits, None);
    assert_eq!(snapshot.coverage.state, crate::CoverageState::Partial);
    assert_eq!(snapshot.coverage.warning_count, 1);
}

/// 缺少数据根身份的记录必须被排除，避免快照与来源明细对同一批数据得出不同结论。
#[test]
fn empty_root_id_is_omitted_from_usage_statistics() {
    let valid = usage(
        "valid-root",
        "s1",
        TODAY_EPOCH_MS,
        Some("model-a"),
        WorkbuddyUsageOrigin::TopLevel,
        10,
        4,
        2,
        Some(1.0),
    );
    let mut invalid = usage(
        "missing-root",
        "s2",
        TODAY_EPOCH_MS,
        Some("model-b"),
        WorkbuddyUsageOrigin::TopLevel,
        20,
        5,
        3,
        Some(2.0),
    );
    invalid.root_id = "  ".to_owned();

    let snapshot = compute_workbuddy_statistics_with_standard(
        &[valid, invalid],
        &[],
        &complete_workbuddy_coverage(),
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    );

    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 12);
    assert_eq!(snapshot.total_credits, Some(1.0));
    assert_eq!(snapshot.coverage.state, crate::CoverageState::Partial);
    assert_eq!(snapshot.coverage.warning_count, 1);
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - tests_more.rs：Trace 只能影响状态与耗时诊断，不能产生 Token、请求或模型行。；「全部」必须合并 WorkBuddy 的精确输入/缓存/输出/总量与请求数。；WorkBuddy 读取失败时不得丢掉其他 Agent 的已确…
include!("tests_more.rs");
