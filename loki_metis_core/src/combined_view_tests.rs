//! 「全部」只读联合视图的核心边界回归。

use std::collections::{BTreeMap, BTreeSet};

use jiff::tz::TimeZone;

use super::*;
use crate::{
    Confidence, DiscoveryMethod, LocalUsageWindow, RootActivationState, RootRecord,
    SourceProvenance, TokenUsage, UsageCallFilters, UsageCallSortDirection, UsageCallSortField,
    canonicalize_usage_calls, empty_token_usage_for_provider,
};

/// 构造完整覆盖，避免质量字段掩盖联合求和断言。
fn complete_coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

/// 构造一条带固定安全身份的 canonical 调用。
fn call(id: &str, occurred_at_epoch_ms: i64, total_tokens: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(),
        occurred_at_epoch_ms,
        model: Some("model-a".to_owned()),
        reasoning_effort: Some("high".to_owned()),
        project_key: Some(format!("project-{id}")),
        project_label: Some("demo".to_owned()),
        thread_key: format!("thread-{id}"),
        thread_label: None,
        usage: TokenUsage::new(6, 2, Some(1), 4, 1, Some(total_tokens))
            .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{id}"),
            root_id: format!("root-{id}"),
            relative_label: "session.jsonl".to_owned(),
            archived: false,
        }],
    }
}

/// 把调用装入指定 Agent 的单库快照，并按其 provider 修正空单位元。
fn agent_snapshot(
    client: SourceClientKind,
    index_state: LocalIndexState,
    calls: Vec<UsageCall>,
) -> AgentUsageSnapshot {
    agent_snapshot_with_version(client, index_state, calls, None)
}

/// 构造可显式替换 parser 来源版本的单库快照。
fn agent_snapshot_with_version(
    client: SourceClientKind,
    index_state: LocalIndexState,
    calls: Vec<UsageCall>,
    source_version: Option<&str>,
) -> AgentUsageSnapshot {
    let provider = provider_for_client(client);
    let mut canonical = canonicalize_usage_calls(calls);
    canonical.empty_tokens = empty_token_usage_for_provider(provider);
    canonical.total_token_accounting = provider.local_total_token_accounting();
    AgentUsageSnapshot::new(
        client,
        UsageSnapshot {
            canonical,
            index_state,
            roots: vec![RootRecord {
                root_id: format!("root-{}", agent_wire_label(client)),
                alias: format!("{} root", agent_wire_label(client)),
                enabled: true,
                activation_state: RootActivationState::Ready,
                is_primary: false,
                discovery_method: DiscoveryMethod::Registered,
                last_coverage: Some(CoverageState::Complete),
                source_file_count: 1,
                call_observation_count: 1,
            }],
        },
        complete_coverage(),
        Some(
            source_version
                .map(str::to_owned)
                .unwrap_or_else(|| provider.parser_source_label(client.parser_version())),
        ),
    )
}

/// 构造已开启但没有任何可用数据源的 Agent，用于锁定「全部」视图的忽略规则。
fn unconfigured_agent_snapshot(client: SourceClientKind) -> AgentUsageSnapshot {
    let mut snapshot = agent_snapshot(client, LocalIndexState::NotScanned, Vec::new());
    snapshot.snapshot.roots.clear();
    snapshot
}

/// 「全部」只解析已开启子集并固定 Agent 顺序，空集合和关闭单项都拒绝。
#[test]
fn resolves_only_enabled_members_and_rejects_empty_or_disabled_views() {
    let enabled = EnabledAgents::empty()
        .with(SourceClientKind::ClaudeCode, true)
        .with(SourceClientKind::GrokBuildCli, true);
    assert_eq!(
        resolve_usage_view_members(UsageViewKind::All, enabled),
        Ok(vec![
            SourceClientKind::ClaudeCode,
            SourceClientKind::GrokBuildCli,
        ])
    );
    assert_eq!(
        resolve_usage_view_members(UsageViewKind::All, EnabledAgents::empty()),
        Err(UsageViewError::NoEnabledAgents)
    );
    assert_eq!(
        resolve_usage_view_members(UsageViewKind::Agent(SourceClientKind::Codex), enabled),
        Err(UsageViewError::AgentDisabled)
    );
}

/// 只开启一个 Agent 时，联合视图除联合 provider 与命名空间身份外必须保持原值。
#[test]
fn single_agent_combined_view_matches_the_physical_metrics() {
    let observed = 1_777_000_000_000_i64;
    let input = agent_snapshot_with_version(
        SourceClientKind::Codex,
        LocalIndexState::Ready,
        vec![call("single", observed, 12)],
        Some("rollout-parser-v8"),
    );
    let physical_overview = crate::build_local_windows_with_standard(
        &input.snapshot.canonical,
        &input.coverage,
        input.snapshot.index_state,
        observed,
        ProviderKind::RolloutJsonl,
        input.source_version.as_deref(),
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("physical overview");
    let physical_calls = crate::build_usage_calls_page(
        &input.snapshot.canonical.calls,
        input.snapshot.index_state,
        &BTreeMap::new(),
        &UsageCallsQuery::default(),
        observed,
        SourceClientKind::Codex,
        ProviderKind::RolloutJsonl,
        input.source_version.as_deref(),
    )
    .expect("physical calls");
    let combined = combine_agent_usage_snapshots(vec![input]).expect("single combined snapshot");
    let combined_overview = build_combined_local_windows_with_standard(
        &combined,
        observed,
        TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("combined overview");
    let combined_calls =
        build_combined_usage_calls_page(&combined, &UsageCallsQuery::default(), observed)
            .expect("combined calls");

    assert_eq!(combined_overview.index_state, physical_overview.index_state);
    assert_eq!(
        combined_overview.windows.len(),
        physical_overview.windows.len()
    );
    for (combined_window, physical_window) in combined_overview
        .windows
        .iter()
        .zip(&physical_overview.windows)
    {
        assert_eq!(combined_window.window, physical_window.window);
        assert_eq!(combined_window.fact.value, physical_window.fact.value);
    }
    assert_eq!(combined_calls.total_count, physical_calls.total_count);
    assert_eq!(combined_calls.items.len(), 1);
    assert_eq!(combined_calls.items[0].client, SourceClientKind::Codex);
    assert_eq!(
        combined_calls.items[0].fact.value,
        physical_calls.items[0].fact.value
    );
}

/// 跨 Agent 相同逻辑 ID 必须保留两条，概览逐字段求和并使用联合 provider。
#[test]
fn preserves_cross_agent_id_collisions_and_sums_overview_tokens() {
    let observed = 1_777_000_000_000_i64;
    let combined = combine_agent_usage_snapshots(vec![
        agent_snapshot(
            SourceClientKind::Codex,
            LocalIndexState::Ready,
            vec![call("same", observed, 12)],
        ),
        agent_snapshot(
            SourceClientKind::ClaudeCode,
            LocalIndexState::Ready,
            vec![call("same", observed, 20)],
        ),
    ])
    .expect("combined snapshot");

    assert_eq!(combined.canonical.calls.len(), 2);
    assert_ne!(
        combined.canonical.calls[0].logical_call_id,
        combined.canonical.calls[1].logical_call_id
    );
    assert_ne!(
        combined.canonical.calls[0].thread_key,
        combined.canonical.calls[1].thread_key
    );
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
    assert_eq!(today.fact.provider, ProviderKind::CombinedLocalAgents);
    assert_eq!(today.fact.value.call_count, 2);
    assert_eq!(today.fact.value.tokens.input_tokens, 12);
    assert_eq!(today.fact.value.tokens.output_tokens, 8);
    assert_eq!(today.fact.value.tokens.total_tokens, 32);
    assert!(
        today.fact.value.tokens.total_tokens
            >= today.fact.value.tokens.input_tokens + today.fact.value.tokens.output_tokens
    );
}

/// 联合状态按保守优先级合成，未扫描或待重扫时不得返回其余 Agent 部分事实。
#[test]
fn combines_index_states_conservatively_and_hides_partial_usage() {
    let observed = 1_777_000_000_000_i64;
    for (left, right, expected, expected_calls) in [
        (
            LocalIndexState::Ready,
            LocalIndexState::NeedsRescan,
            LocalIndexState::NeedsRescan,
            0,
        ),
        (
            LocalIndexState::Ready,
            LocalIndexState::NotScanned,
            LocalIndexState::NotScanned,
            0,
        ),
        (
            LocalIndexState::Ready,
            LocalIndexState::ReadyNoCalls,
            LocalIndexState::Ready,
            1,
        ),
        (
            LocalIndexState::ReadyNoCalls,
            LocalIndexState::ReadyNoCalls,
            LocalIndexState::ReadyNoCalls,
            0,
        ),
    ] {
        let left_calls = if left == LocalIndexState::Ready {
            vec![call("left", observed, 12)]
        } else {
            Vec::new()
        };
        let combined = combine_agent_usage_snapshots(vec![
            agent_snapshot(SourceClientKind::Codex, left, left_calls),
            agent_snapshot(SourceClientKind::ClaudeCode, right, Vec::new()),
        ])
        .expect("state combination");
        assert_eq!(combined.index_state, expected);
        assert_eq!(combined.canonical.calls.len(), expected_calls);
        assert_eq!(combined.origins.len(), expected_calls);
    }
}

/// 未配置数据源的 Agent 不得把其他 Agent 的已索引数据清空或降级为未扫描。
#[test]
fn ignores_unconfigured_agents_without_hiding_other_usage() {
    let observed = 1_777_000_000_000_i64;
    let combined = combine_agent_usage_snapshots(vec![
        agent_snapshot(
            SourceClientKind::Codex,
            LocalIndexState::Ready,
            vec![call("configured", observed, 12)],
        ),
        unconfigured_agent_snapshot(SourceClientKind::ClaudeCode),
    ])
    .expect("configured member keeps the combined view available");

    assert_eq!(combined.index_state, LocalIndexState::Ready);
    assert_eq!(combined.canonical.calls.len(), 1);
    assert_eq!(combined.origins.len(), 1);
}

/// 仍有启用数据源但尚未扫描的 Agent 必须继续阻断部分联合结果，不能被误判为未配置。
#[test]
fn keeps_configured_unscanned_agents_in_combined_state() {
    let observed = 1_777_000_000_000_i64;
    let combined = combine_agent_usage_snapshots(vec![
        agent_snapshot(
            SourceClientKind::Codex,
            LocalIndexState::Ready,
            vec![call("configured", observed, 12)],
        ),
        agent_snapshot(
            SourceClientKind::ClaudeCode,
            LocalIndexState::NotScanned,
            Vec::new(),
        ),
    ])
    .expect("configured members remain part of the combined view");

    assert_eq!(combined.index_state, LocalIndexState::NotScanned);
    assert!(combined.canonical.calls.is_empty());
    assert!(combined.origins.is_empty());
}

/// 联合调用页必须全局排序分页，并为每行保留具体 Agent、provider 与 parser 版本。
#[test]
fn globally_sorts_pages_and_keeps_each_call_origin() {
    let codex_calls = (0_i64..51)
        .map(|index| call(&format!("codex-{index}"), 10_000 + index * 2, 12))
        .collect();
    let claude_calls = (0_i64..50)
        .map(|index| call(&format!("claude-{index}"), 10_001 + index * 2, 13))
        .collect();
    let combined = combine_agent_usage_snapshots(vec![
        agent_snapshot(SourceClientKind::Codex, LocalIndexState::Ready, codex_calls),
        agent_snapshot(
            SourceClientKind::ClaudeCode,
            LocalIndexState::Ready,
            claude_calls,
        ),
    ])
    .expect("combined calls");
    let query = UsageCallsQuery {
        filters: UsageCallFilters::default(),
        sort_field: UsageCallSortField::OccurredAt,
        sort_direction: UsageCallSortDirection::Desc,
        cursor: None,
    };
    let first =
        build_combined_usage_calls_page(&combined, &query, 20_000).expect("first combined page");
    assert_eq!(first.total_count, 101);
    assert_eq!(first.items.len(), 100);
    assert!(
        first
            .items
            .windows(2)
            .all(|pair| pair[0].occurred_at_epoch_ms >= pair[1].occurred_at_epoch_ms)
    );
    let clients = first
        .items
        .iter()
        .map(|item| item.client)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        clients,
        BTreeSet::from([SourceClientKind::Codex, SourceClientKind::ClaudeCode])
    );
    for item in &first.items {
        match item.client {
            SourceClientKind::Codex => {
                assert_eq!(item.fact.provider, ProviderKind::RolloutJsonl);
                assert_eq!(
                    item.fact.source_version.as_deref(),
                    Some("rollout-parser-v8")
                );
            }
            SourceClientKind::ClaudeCode => {
                assert_eq!(item.fact.provider, ProviderKind::ClaudeTranscriptJsonl);
                assert_eq!(
                    item.fact.source_version.as_deref(),
                    Some("claude-transcript-parser-v4")
                );
            }
            SourceClientKind::GrokBuildCli => panic!("unexpected Grok row"),
            SourceClientKind::WorkBuddy => panic!("unexpected WorkBuddy row"),
        }
    }

    let second = build_combined_usage_calls_page(
        &combined,
        &UsageCallsQuery {
            cursor: first.next_cursor.clone(),
            ..query
        },
        20_000,
    )
    .expect("second combined page");
    assert_eq!(second.items.len(), 1);
}

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
