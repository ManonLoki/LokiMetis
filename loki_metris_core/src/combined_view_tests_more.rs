// 本文件是 combined_view_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：联合游标绑定完整成员与 parser 来源，只有来源版本变化也必须拒绝旧游标。；联合装配必须拒绝空输入和重复物理 Agent。；联合概览窗口只有 Codex 有调用时，Today 事实必须反映实际观测到的分项，

/// 联合游标绑定完整成员与 parser 来源，只有来源版本变化也必须拒绝旧游标。
#[test]
fn rejects_cursor_when_any_member_source_version_changes() {
    let calls = (0_i64..101)
        .map(|index| call(&format!("call-{index}"), index, 12))
        .collect::<Vec<_>>();
    let original = combine_agent_usage_snapshots(vec![agent_snapshot_with_version(
        SourceClientKind::Codex,
        LocalIndexState::Ready,
        calls.clone(),
        Some("rollout-parser-v8"),
    )])
    .expect("original combined snapshot");
    let first = build_combined_usage_calls_page(&original, &UsageCallsQuery::default(), 200)
        .expect("first page");
    let changed = combine_agent_usage_snapshots(vec![agent_snapshot_with_version(
        SourceClientKind::Codex,
        LocalIndexState::Ready,
        calls,
        Some("rollout-parser-v9"),
    )])
    .expect("changed combined snapshot");
    let error = build_combined_usage_calls_page(
        &changed,
        &UsageCallsQuery {
            cursor: first.next_cursor,
            ..UsageCallsQuery::default()
        },
        200,
    )
    .expect_err("stale cursor must fail");
    assert_eq!(error, crate::cursor_error());
}

/// 联合装配必须拒绝空输入和重复物理 Agent。
#[test]
fn rejects_empty_and_duplicate_agent_snapshots() {
    assert_eq!(
        combine_agent_usage_snapshots(Vec::new()),
        Err(UsageViewError::NoEnabledAgents)
    );
    assert_eq!(
        combine_agent_usage_snapshots(vec![unconfigured_agent_snapshot(SourceClientKind::Codex,)]),
        Err(UsageViewError::NoConfiguredDataSources)
    );
    assert_eq!(
        combine_agent_usage_snapshots(vec![
            agent_snapshot(
                SourceClientKind::Codex,
                LocalIndexState::ReadyNoCalls,
                Vec::new(),
            ),
            agent_snapshot(
                SourceClientKind::Codex,
                LocalIndexState::ReadyNoCalls,
                Vec::new(),
            ),
        ]),
        Err(UsageViewError::DuplicateAgent)
    );
}

/// 联合概览窗口只有 Codex 有调用时，Today 事实必须反映实际观测到的分项，
/// 不能被 combined canonical 的跨 Agent 交集空单位元（Claude 不含
/// reasoning_output_tokens）降级为“未提供”——概览窗口没有子桶分解，聚合
/// 直接从真实调用折叠而来，因此 empty_tokens 的交集形状不会污染非空窗口。
#[test]
fn combined_today_overview_uses_the_observed_shape_when_only_codex_has_calls() {
    let observed = 1_777_000_000_000_i64;
    let combined = combine_agent_usage_snapshots(vec![
        agent_snapshot(
            SourceClientKind::Codex,
            LocalIndexState::Ready,
            vec![call("codex-today", observed, 12)],
        ),
        agent_snapshot(
            SourceClientKind::ClaudeCode,
            LocalIndexState::ReadyNoCalls,
            Vec::new(),
        ),
    ])
    .expect("combined snapshot");

    let summary = build_combined_local_windows_with_standard(
        &combined,
        observed,
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("combined overview");
    let today = summary
        .windows
        .iter()
        .find(|window| window.window == LocalUsageWindow::Today)
        .expect("today window");
    assert_eq!(today.fact.value.tokens.reasoning_output_tokens, Some(1));
}
