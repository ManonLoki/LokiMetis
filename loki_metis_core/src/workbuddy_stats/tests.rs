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

/// Trace 只能影响状态与耗时诊断，不能产生 Token、请求或模型行。
#[test]
fn traces_never_contribute_usage_or_models() {
    let traces = vec![
        trace(WorkbuddyTraceStatus::Ok, 1_000),
        trace(WorkbuddyTraceStatus::Error, 3_000),
        trace(WorkbuddyTraceStatus::Cancelled, 5_000),
    ];
    let snapshot = compute_workbuddy_statistics(&[], &traces, OBSERVED_EPOCH_MS);

    assert_eq!(snapshot.total_tokens, 0);
    assert_eq!(snapshot.total_requests, 0);
    assert_eq!(snapshot.trace_total_count, 3);
    assert_eq!(snapshot.trace_error_count, 1);
    assert_eq!(snapshot.trace_cancelled_count, 1);
    assert_eq!(snapshot.trace_average_duration_ms, 3_000.0);
}

/// 「全部」必须合并 WorkBuddy 的精确输入/缓存/输出/总量与请求数。
#[test]
fn combined_windows_merge_exact_components_and_partial_coverage() {
    use crate::{
        Completeness, Confidence, Freshness, LocalIndexState, LocalRecordsSummary,
        LocalUsageWindow, MetricFact, MetricScope, ProviderKind, WindowUsage,
        empty_local_usage_aggregate_for_provider,
    };

    let mut aggregate = empty_local_usage_aggregate_for_provider(ProviderKind::CombinedLocalAgents);
    aggregate.tokens.input_tokens = 10;
    aggregate.tokens.cached_input_tokens = Some(4);
    aggregate.tokens.output_tokens = 5;
    aggregate.tokens.total_tokens = 15;
    aggregate.call_count = 2;
    let summary = LocalRecordsSummary {
        index_state: LocalIndexState::Ready,
        windows: vec![WindowUsage {
            window: LocalUsageWindow::Today,
            fact: MetricFact::new(
                aggregate,
                ProviderKind::CombinedLocalAgents,
                MetricScope::DeviceObserved,
                OBSERVED_EPOCH_MS,
                Freshness::Fresh,
                Completeness::Complete,
                Confidence::Exact,
                None,
            ),
        }],
    };
    let mut coverage = complete_workbuddy_coverage();
    coverage.state = crate::CoverageState::Partial;
    let merged = merge_workbuddy_into_local_windows(
        summary,
        &[WorkbuddyWindowAggregate {
            window: LocalUsageWindow::Today,
            session_count: 3,
            request_count: 4,
            top_level_request_count: 3,
            subagent_request_count: 1,
            input_tokens: 40,
            cached_input_tokens: 30,
            uncached_input_tokens: 10,
            output_tokens: 6,
            tokens: 46,
            cached_read_request_count: 3,
            source_count: 2,
            credits: Some(1.5),
            dates: vec!["2026-07-27".to_owned()],
            average_session_duration_seconds: 0.0,
            trace_total_count: 0,
            trace_error_count: 0,
            trace_cancelled_count: 0,
            trace_average_duration_ms: 0.0,
        }],
        &coverage,
    );

    let today = &merged.windows[0];
    assert_eq!(today.fact.value.tokens.input_tokens, 50);
    assert_eq!(today.fact.value.tokens.cached_input_tokens, Some(34));
    assert_eq!(today.fact.value.tokens.output_tokens, 11);
    assert_eq!(today.fact.value.tokens.total_tokens, 61);
    assert_eq!(today.fact.value.call_count, 6);
    assert_eq!(today.fact.completeness, Completeness::Partial);
}

/// WorkBuddy 读取失败时不得丢掉其他 Agent 的已确认数字，也不得把联合结果标成完整。
#[test]
fn combined_windows_keep_other_agents_but_mark_missing_workbuddy_partial() {
    use crate::{
        Completeness, Confidence, Freshness, LocalIndexState, LocalRecordsSummary,
        LocalUsageWindow, MetricFact, MetricScope, ProviderKind, WindowUsage,
        empty_local_usage_aggregate_for_provider,
    };

    let mut aggregate = empty_local_usage_aggregate_for_provider(ProviderKind::CombinedLocalAgents);
    aggregate.tokens.input_tokens = 10;
    aggregate.tokens.output_tokens = 5;
    aggregate.tokens.total_tokens = 15;
    aggregate.call_count = 2;
    let summary = LocalRecordsSummary {
        index_state: LocalIndexState::Ready,
        windows: vec![WindowUsage {
            window: LocalUsageWindow::Today,
            fact: MetricFact::new(
                aggregate,
                ProviderKind::CombinedLocalAgents,
                MetricScope::DeviceObserved,
                OBSERVED_EPOCH_MS,
                Freshness::Fresh,
                Completeness::Complete,
                Confidence::Exact,
                None,
            ),
        }],
    };

    let downgraded = mark_workbuddy_unavailable_in_local_windows(summary);
    let today = &downgraded.windows[0];
    assert_eq!(today.fact.value.tokens.input_tokens, 10);
    assert_eq!(today.fact.value.tokens.output_tokens, 5);
    assert_eq!(today.fact.value.tokens.total_tokens, 15);
    assert_eq!(today.fact.value.call_count, 2);
    assert_eq!(today.fact.completeness, Completeness::Partial);
}

/// 物理 Agent 当前无调用但 WorkBuddy 有调用时，「全部」不能继续显示为空索引。
#[test]
fn combined_index_becomes_ready_when_workbuddy_has_calls() {
    use crate::{
        Completeness, Confidence, Freshness, LocalIndexState, LocalRecordsSummary,
        LocalUsageWindow, MetricFact, MetricScope, ProviderKind, WindowUsage,
        empty_local_usage_aggregate_for_provider,
    };

    let summary = LocalRecordsSummary {
        index_state: LocalIndexState::ReadyNoCalls,
        windows: vec![WindowUsage {
            window: LocalUsageWindow::Today,
            fact: MetricFact::new(
                empty_local_usage_aggregate_for_provider(ProviderKind::CombinedLocalAgents),
                ProviderKind::CombinedLocalAgents,
                MetricScope::DeviceObserved,
                OBSERVED_EPOCH_MS,
                Freshness::Fresh,
                Completeness::Complete,
                Confidence::Exact,
                None,
            ),
        }],
    };
    let workbuddy = WorkbuddyWindowAggregate {
        window: LocalUsageWindow::Today,
        session_count: 1,
        request_count: 1,
        top_level_request_count: 1,
        subagent_request_count: 0,
        input_tokens: 10,
        cached_input_tokens: 8,
        uncached_input_tokens: 2,
        output_tokens: 1,
        tokens: 11,
        cached_read_request_count: 1,
        source_count: 1,
        credits: Some(0.1),
        dates: vec!["2026-07-27".to_owned()],
        average_session_duration_seconds: 0.0,
        trace_total_count: 0,
        trace_error_count: 0,
        trace_cancelled_count: 0,
        trace_average_duration_ms: 0.0,
    };

    let merged =
        merge_workbuddy_into_local_windows(summary, &[workbuddy], &complete_workbuddy_coverage());
    assert_eq!(merged.index_state, LocalIndexState::Ready);
    assert_eq!(merged.windows[0].fact.value.call_count, 1);
}

/// 通用统计页按事件实际模型分组，并保留精确缓存输入。
#[test]
fn usage_page_groups_actual_models_with_cache_components() {
    let records = vec![
        usage(
            "a",
            "same-session",
            TODAY_EPOCH_MS,
            Some("model-a"),
            WorkbuddyUsageOrigin::TopLevel,
            10,
            8,
            2,
            Some(0.1),
        ),
        usage(
            "b",
            "same-session",
            TODAY_EPOCH_MS + 1,
            Some("model-b"),
            WorkbuddyUsageOrigin::TopLevel,
            20,
            5,
            3,
            Some(0.2),
        ),
    ];
    let (page, model_usage) = build_workbuddy_usage_details(
        &records,
        &complete_workbuddy_coverage(),
        "workbuddy-root-test",
        ".workbuddy",
        crate::LocalUsageWindow::Today,
        crate::UsageDimension::Model,
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    )
    .expect("statistics page");

    assert_eq!(page.fact.value.call_count, 2);
    assert_eq!(page.fact.value.tokens.input_tokens, 30);
    assert_eq!(page.fact.value.tokens.cached_input_tokens, Some(13));
    assert_eq!(page.fact.value.tokens.output_tokens, 5);
    assert_eq!(page.fact.value.tokens.total_tokens, 35);
    assert_eq!(page.groups.len(), 2);
    assert_eq!(page.groups[0].label, "model-b");
    assert_eq!(page.groups[1].label, "model-a");

    // 同一次校验产出的模型表必须与统计页一致，防止两条聚合路径漂移。
    assert_eq!(model_usage.window, crate::LocalUsageWindow::Today);
    assert_eq!(model_usage.groups.len(), 2);
    let model_total: u64 = model_usage
        .groups
        .iter()
        .map(|group| group.total_tokens)
        .sum();
    assert_eq!(model_total, page.fact.value.tokens.total_tokens);
}
