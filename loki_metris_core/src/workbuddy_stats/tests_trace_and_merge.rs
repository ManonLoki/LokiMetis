// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：Trace 只能影响状态与耗时诊断，不能产生 Token、请求或模型行。；「全部」必须合并 WorkBuddy 的精确输入/缓存/输出/总量与请求数。；WorkBuddy 读取失败时不得丢掉其他 Agent 的已确认数字，也不得把联合结果标成完整。

/// Trace 只能影响状态与耗时诊断，不能产生 Token、请求或模型行。
#[test]
fn traces_never_contribute_usage_or_models() {
    let traces = vec![
        trace(WorkBuddyTraceStatus::Ok, 1_000),
        trace(WorkBuddyTraceStatus::Error, 3_000),
        trace(WorkBuddyTraceStatus::Cancelled, 5_000),
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
        &[WorkBuddyWindowAggregate {
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
    let workbuddy = WorkBuddyWindowAggregate {
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
            WorkBuddyUsageOrigin::TopLevel,
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
            WorkBuddyUsageOrigin::TopLevel,
            20,
            5,
            3,
            Some(0.2),
        ),
    ];
    let (page, model_usage) = build_workbuddy_usage_details(
        &records,
        &complete_workbuddy_coverage(),
        &std::collections::BTreeMap::from([(
            "workbuddy-root-test".to_owned(),
            ".workbuddy".to_owned(),
        )]),
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

/// 构造一条属于指定 WorkBuddy 数据根的 usage 事件，供双版本合并测试使用。
fn usage_in_root(
    root_id: &str,
    id: &str,
    session: &str,
    occurred_at_epoch_ms: i64,
    input_tokens: i64,
    output_tokens: i64,
) -> WorkBuddyUsageEventRecord {
    let mut record = usage(
        id,
        session,
        occurred_at_epoch_ms,
        Some("model-a"),
        WorkBuddyUsageOrigin::TopLevel,
        input_tokens,
        0,
        output_tokens,
        None,
    );
    record.root_id = root_id.to_owned();
    record
}

/// 国内版与国际版记录合并后必须按各自数据根解析别名，缺别名时失败关闭。
#[test]
fn merged_editions_resolve_each_root_alias_and_reject_missing_alias() {
    let records = vec![
        usage_in_root(
            "workbuddy-root-cn",
            "cn-1",
            "cn-session",
            TODAY_EPOCH_MS,
            10,
            5,
        ),
        usage_in_root(
            "workbuddy-root-intl",
            "intl-1",
            "intl-session",
            TODAY_EPOCH_MS,
            20,
            7,
        ),
    ];
    let aliases = std::collections::BTreeMap::from([
        ("workbuddy-root-cn".to_owned(), ".workbuddy".to_owned()),
        ("workbuddy-root-intl".to_owned(), ".workbuddy-ai".to_owned()),
    ]);

    let (page, model_usage) = build_workbuddy_usage_details(
        &records,
        &complete_workbuddy_coverage(),
        &aliases,
        crate::LocalUsageWindow::Today,
        crate::UsageDimension::Model,
        OBSERVED_EPOCH_MS,
        &crate::TimeStandard::utc(),
        &jiff::tz::TimeZone::UTC,
    )
    .expect("merged statistics page");

    // 两条根各自的请求都必须计入同一个 WorkBuddy 统计页。
    assert_eq!(page.fact.value.call_count, 2);
    assert_eq!(page.fact.value.tokens.total_tokens, 42);
    assert_eq!(model_usage.groups.len(), 1);

    // 缺少任一已出现数据根的别名时必须报错，不能用空标签冒充来源。
    let incomplete = std::collections::BTreeMap::from([(
        "workbuddy-root-cn".to_owned(),
        ".workbuddy".to_owned(),
    )]);
    assert!(
        build_workbuddy_usage_details(
            &records,
            &complete_workbuddy_coverage(),
            &incomplete,
            crate::LocalUsageWindow::Today,
            crate::UsageDimension::Model,
            OBSERVED_EPOCH_MS,
            &crate::TimeStandard::utc(),
            &jiff::tz::TimeZone::UTC,
        )
        .is_err()
    );
}

/// 两个固定根的覆盖结论必须按根求和，任一 Partial 都不能被完整根掩盖。
#[test]
fn root_coverage_merges_by_root_and_degrades_on_partial() {
    let complete = crate::CoverageReport {
        state: crate::CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    };
    let partial = crate::CoverageReport {
        state: crate::CoverageState::Partial,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 1,
        warning_count: 2,
    };

    let merged = combine_workbuddy_root_coverage(&[complete.clone(), complete.clone()]);
    assert_eq!(merged.state, crate::CoverageState::Complete);
    assert_eq!(merged.roots_scanned, 2);
    assert_eq!(merged.roots_discovered, 2);

    let degraded = combine_workbuddy_root_coverage(&[complete, partial]);
    assert_eq!(degraded.state, crate::CoverageState::Partial);
    assert_eq!(degraded.roots_scanned, 2);
    assert_eq!(degraded.roots_discovered, 2);
    assert_eq!(degraded.skipped_count, 1);
    assert_eq!(degraded.warning_count, 2);
}
