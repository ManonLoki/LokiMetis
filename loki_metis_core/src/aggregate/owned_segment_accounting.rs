//! ADR-104：生产聚合只累计归属段内 canonical 调用的原始总量。
//!
//! 兼容 cumulative 快照仍可挂到集合上，但不得覆盖窗口、日桶、分组、
//! 调用表或 Collect 口径。578,438,961 只作为禁止回退的历史对照。

use crate::{
    CanonicalUsageSet, Confidence, CoverageReport, CoverageState, IncrementalTokenUsageDecision,
    LocalIndexState, LocalUsageWindow, ProviderKind, SessionTokenSnapshot, SourceClientKind,
    SourceProvenance, TokenUsage, TotalTokenAccounting, UsageCall, UsageCallSortDirection,
    UsageCallSortField, UsageCallsQuery, UsageDimension, aggregate_canonical_usage,
    attach_session_snapshots, build_usage_calls_page, build_usage_statistics,
    canonicalize_usage_calls, group_usage, select_incremental_call_usage,
};
use jiff::civil::Date;
use jiff::tz::TimeZone;
use std::collections::BTreeMap;

/// 被 ADR-104 覆盖的 8/12 根线程 latest 对照值，生产合计不得回退到该数。
const FORBIDDEN_AUG12_SESSION_LATEST: u64 = 578_438_961;

/// 将测试使用的本地民用日期时间转换为毫秒时间戳。
fn local_civil_ms(date: Date, hour: i8, minute: i8, second: i8) -> i64 {
    date.at(hour, minute, second, 0)
        .to_zoned(TimeZone::system())
        .expect("civil instant is representable")
        .timestamp()
        .as_millisecond()
}

/// 构造带父级累计量的测试快照，用于验证区段所有权会计。
fn snapshot(
    thread_key: &str,
    occurred_at_epoch_ms: i64,
    cumulative_total_tokens: u64,
    logical_call_id: &str,
    source_id: &str,
) -> SessionTokenSnapshot {
    SessionTokenSnapshot {
        thread_key: thread_key.to_owned(),
        occurred_at_epoch_ms,
        cumulative_total_tokens,
        logical_call_id: logical_call_id.to_owned(),
        model: Some("gpt-5".to_owned()),
        reasoning_effort: Some("medium".to_owned()),
        project_key: Some("proj".to_owned()),
        provenance: vec![SourceProvenance {
            source_id: source_id.to_owned(),
            root_id: "root".to_owned(),
            relative_label: "sessions/rollout.jsonl".to_owned(),
            archived: false,
        }],
    }
}

/// 构造只包含当前区段增量的调用，模拟 fork 后独立用量。
fn incremental_call(
    logical_call_id: &str,
    thread_key: &str,
    occurred_at_epoch_ms: i64,
    total_tokens: u64,
) -> UsageCall {
    UsageCall {
        logical_call_id: logical_call_id.to_owned(),
        occurred_at_epoch_ms,
        model: Some("gpt-5".to_owned()),
        reasoning_effort: Some("medium".to_owned()),
        project_key: Some("proj".to_owned()),
        project_label: Some("proj".to_owned()),
        thread_key: thread_key.to_owned(),
        thread_label: None,
        usage: TokenUsage::new(100, 40, None, 20, 5, Some(total_tokens))
            .expect("incremental usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{logical_call_id}"),
            root_id: "root".to_owned(),
            relative_label: "sessions/rollout.jsonl".to_owned(),
            archived: false,
        }],
    }
}

/// 返回测试所需的完整覆盖报告，排除缺失覆盖对结果的干扰。
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

/// 将测试调用包装成已观察的规范用量集合。
fn observed(calls: Vec<UsageCall>) -> CanonicalUsageSet {
    canonicalize_usage_calls(calls).with_total_token_accounting(TotalTokenAccounting::Observed)
}

/// 构造显式输入、缓存、输出与上游总量的测试 Token 用量。
fn usage(input: u64, cached: u64, output: u64, total: u64) -> TokenUsage {
    TokenUsage::new(input, cached, Some(0), output, 0, Some(total)).expect("fixture usage is valid")
}

/// 挂上会把公式 D 拉回 578M 的兼容快照后，生产合计仍只认归属调用原始总量。
#[test]
fn attached_snapshots_do_not_override_owned_raw_calls() {
    let today = Date::new(2026, 8, 12).expect("lock date");
    let calls = (0..24)
        .map(|index| {
            incremental_call(
                &format!("call-{index}"),
                &format!("thread-{index:02}"),
                local_civil_ms(today, 10, i8::try_from(index).unwrap_or(0), 0),
                120,
            )
        })
        .collect();
    let snapshots = (0..24)
        .map(|index| {
            snapshot(
                &format!("thread-{index:02}"),
                local_civil_ms(today, 11, i8::try_from(index).unwrap_or(0), 0),
                if index == 0 {
                    FORBIDDEN_AUG12_SESSION_LATEST
                } else {
                    0
                },
                &format!("snap-{index}"),
                &format!("source-{index}"),
            )
        })
        .collect();
    let canonical = attach_session_snapshots(observed(calls), snapshots);
    let aggregate = aggregate_canonical_usage(&canonical).expect("owned calls aggregate");

    assert_eq!(aggregate.tokens.total_tokens, 24 * 120);
    assert_ne!(
        aggregate.tokens.total_tokens,
        FORBIDDEN_AUG12_SESSION_LATEST
    );
}

/// 同一会话多条累计快照不得改写归属调用合计。
#[test]
fn later_cumulative_snapshot_is_ignored_by_production_total() {
    let today = Date::new(2026, 8, 12).expect("date");
    let canonical = attach_session_snapshots(
        observed(vec![incremental_call(
            "call-1128",
            "thread-1128",
            local_civil_ms(today, 11, 28, 0),
            120,
        )]),
        vec![
            snapshot(
                "thread-1128",
                local_civil_ms(today, 11, 28, 0),
                104_659,
                "early",
                "active",
            ),
            snapshot(
                "thread-1128",
                local_civil_ms(today, 11, 40, 0),
                696_019,
                "late",
                "active",
            ),
        ],
    );

    assert_eq!(
        aggregate_canonical_usage(&canonical)
            .expect("production aggregate")
            .tokens
            .total_tokens,
        120
    );
}

/// 同一会话快照存在活动文件与归档副本时，兼容快照仍只保留一次。
#[test]
fn same_thread_snapshot_from_two_sources_is_canonicalized_once() {
    let today = Date::new(2026, 8, 12).expect("date");
    let ts = local_civil_ms(today, 11, 40, 0);
    let canonical = attach_session_snapshots(
        observed(Vec::new()),
        vec![
            snapshot("thread-shared", ts, 696_019, "same-event", "active"),
            snapshot("thread-shared", ts, 696_019, "same-event", "archive"),
        ],
    );

    assert_eq!(canonical.snapshots.len(), 1);
    assert_eq!(canonical.snapshots[0].provenance.len(), 2);
}

/// Codex 的原始口径保留缓存输入，兼容快照不能改变任何分项。
#[test]
fn codex_observed_total_includes_cached_input() {
    let today = Date::new(2026, 8, 12).expect("date");
    let canonical = observed(vec![
        incremental_call("a", "thread-a", local_civil_ms(today, 10, 0, 0), 120),
        incremental_call("b", "thread-b", local_civil_ms(today, 11, 0, 0), 120),
    ]);
    let aggregate = aggregate_canonical_usage(&canonical).expect("raw aggregate");

    assert_eq!(aggregate.tokens.input_tokens, 200);
    assert_eq!(aggregate.tokens.cached_input_tokens, Some(80));
    assert_eq!(aggregate.tokens.output_tokens, 40);
    assert_eq!(aggregate.tokens.total_tokens, 240);
}

/// 本地日桶直接按调用发生日相加，日桶之和必须等于窗口总量。
#[test]
fn daily_buckets_sum_owned_raw_calls() {
    let day1 = Date::new(2026, 8, 11).expect("day1");
    let day2 = Date::new(2026, 8, 12).expect("day2");
    let observed_at = local_civil_ms(day2, 15, 0, 0);
    let calls = vec![
        incremental_call("c1", "thread-span", local_civil_ms(day1, 10, 0, 0), 120),
        incremental_call("c2", "thread-span", local_civil_ms(day2, 11, 28, 0), 120),
    ];
    let snapshots = vec![
        snapshot(
            "thread-span",
            local_civil_ms(day1, 10, 0, 0),
            1_000_000,
            "d1",
            "src",
        ),
        snapshot(
            "thread-span",
            local_civil_ms(day2, 11, 40, 0),
            1_696_019,
            "d2",
            "src",
        ),
    ];
    let canonical = attach_session_snapshots(observed(calls), snapshots);
    let page = build_usage_statistics(
        &canonical,
        &BTreeMap::from([("root".to_owned(), "root".to_owned())]),
        LocalIndexState::Ready,
        &coverage(),
        LocalUsageWindow::ThisWeek,
        UsageDimension::Thread,
        observed_at,
        ProviderKind::RolloutJsonl,
        None,
    )
    .expect("statistics page");

    assert_eq!(page.fact.value.tokens.total_tokens, 240);
    assert_eq!(
        page.daily_buckets
            .iter()
            .map(|bucket| bucket.measure.tokens.total_tokens)
            .sum::<u64>(),
        240
    );
    assert_eq!(
        page.daily_buckets
            .iter()
            .find(|bucket| bucket.local_date == day1.to_string())
            .expect("day1 bucket")
            .measure
            .tokens
            .total_tokens,
        120
    );
    assert_eq!(
        page.daily_buckets
            .iter()
            .find(|bucket| bucket.local_date == day2.to_string())
            .expect("day2 bucket")
            .measure
            .tokens
            .total_tokens,
        120
    );
}

/// 同一调用在 Codex 与 Claude 行都展示上游 total，缓存只作输入子集。
#[test]
fn calls_view_uses_observed_total_for_all_providers() {
    let today = Date::new(2026, 8, 12).expect("date");
    let call = incremental_call(
        "row-call",
        "thread-1128",
        local_civil_ms(today, 11, 28, 0),
        120,
    );
    let observed_at = local_civil_ms(today, 15, 0, 0);
    let codex = build_usage_calls_page(
        std::slice::from_ref(&call),
        LocalIndexState::Ready,
        &BTreeMap::new(),
        &UsageCallsQuery::default(),
        observed_at,
        SourceClientKind::Codex,
        ProviderKind::RolloutJsonl,
        None,
    )
    .expect("Codex calls page");
    let claude = build_usage_calls_page(
        std::slice::from_ref(&call),
        LocalIndexState::Ready,
        &BTreeMap::new(),
        &UsageCallsQuery::default(),
        observed_at,
        SourceClientKind::ClaudeCode,
        ProviderKind::ClaudeTranscriptJsonl,
        None,
    )
    .expect("Claude calls page");

    assert_eq!(codex.items[0].usage.total_tokens, 120);
    assert_eq!(claude.items[0].usage.total_tokens, 120);
}

/// 总 Token 排序必须在三 provider 中都使用上游总量，不能因缓存子集改变顺序。
#[test]
fn calls_view_sorts_by_observed_total_for_all_providers() {
    let today = Date::new(2026, 8, 12).expect("date");
    let mut cached = incremental_call("cached", "thread-a", local_civil_ms(today, 10, 0, 0), 120);
    cached.usage = TokenUsage::new(110, 100, None, 10, 2, Some(120)).expect("cached call");
    let mut uncached =
        incremental_call("uncached", "thread-b", local_civil_ms(today, 11, 0, 0), 120);
    uncached.usage = TokenUsage::new(90, 0, None, 10, 2, Some(100)).expect("uncached call");
    let query = UsageCallsQuery {
        sort_field: UsageCallSortField::TotalTokens,
        sort_direction: UsageCallSortDirection::Desc,
        ..UsageCallsQuery::default()
    };
    let observed_at = local_civil_ms(today, 15, 0, 0);
    let calls = vec![cached, uncached];
    let codex = build_usage_calls_page(
        &calls,
        LocalIndexState::Ready,
        &BTreeMap::new(),
        &query,
        observed_at,
        SourceClientKind::Codex,
        ProviderKind::RolloutJsonl,
        None,
    )
    .expect("Codex calls page");
    let claude = build_usage_calls_page(
        &calls,
        LocalIndexState::Ready,
        &BTreeMap::new(),
        &query,
        observed_at,
        SourceClientKind::ClaudeCode,
        ProviderKind::ClaudeTranscriptJsonl,
        None,
    )
    .expect("Claude calls page");

    assert_eq!(codex.items[0].id, "cached");
    assert_eq!(codex.items[0].usage.total_tokens, 120);
    assert_eq!(claude.items[0].id, "cached");
    assert_eq!(claude.items[0].usage.total_tokens, 120);
}

/// 分组与窗口总计必须按调用自身维度归属，并逐字段精确对账。
#[test]
fn groups_sum_owned_calls_without_snapshot_reassignment() {
    let today = Date::new(2026, 8, 12).expect("date");
    let mut gpt5 = incremental_call("a", "thread-a", local_civil_ms(today, 10, 0, 0), 120);
    let mut codex = incremental_call("b", "thread-a", local_civil_ms(today, 11, 0, 0), 130);
    gpt5.model = Some("gpt-5".to_owned());
    codex.model = Some("gpt-5-codex".to_owned());
    let canonical = attach_session_snapshots(
        observed(vec![gpt5, codex]),
        vec![snapshot(
            "thread-a",
            local_civil_ms(today, 11, 40, 0),
            696_019,
            "compatibility-snapshot",
            "src",
        )],
    );
    let grouped = group_usage(&canonical, UsageDimension::Model, 10).expect("groups");

    assert_eq!(grouped.total.tokens.total_tokens, 250);
    assert_eq!(
        grouped
            .groups
            .iter()
            .map(|group| group.measure.tokens.total_tokens)
            .sum::<u64>(),
        250
    );
    assert_eq!(
        grouped
            .groups
            .iter()
            .find(|group| group.key.as_deref() == Some("gpt-5"))
            .expect("gpt-5 group")
            .measure
            .tokens
            .total_tokens,
        120
    );
    assert_eq!(
        grouped
            .groups
            .iter()
            .find(|group| group.key.as_deref() == Some("gpt-5-codex"))
            .expect("gpt-5-codex group")
            .measure
            .tokens
            .total_tokens,
        130
    );
}

/// 复制前缀只作为已交付选择器的基线；自有 last-copy / same-last / total-only 后合计不含前缀。
#[test]
fn owned_segment_selector_and_aggregate_ignore_prefix_and_v7_noise() {
    let prefix = usage(400, 300, 100, 500);
    let owned_last_copy = usage(412, 304, 108, 520);
    let same_last = usage(412, 304, 108, 520);
    let estimate = usage(0, 0, 0, 50_000);
    let owned_increment = usage(6, 2, 4, 10);
    let owned_cumulative = usage(418, 306, 112, 530);
    let events = [
        (Some(owned_last_copy.clone()), Some(owned_last_copy.clone())),
        (Some(same_last), Some(usage(412, 304, 108, 600))),
        (Some(estimate.clone()), Some(estimate)),
        (
            Some(owned_increment.clone()),
            Some(owned_cumulative.clone()),
        ),
    ];
    // parser v8 越过复制前缀后把前缀累计写入基线、不 Emit；此处复用同一已交付选择器。
    let mut previous_cumulative = Some(prefix.clone());
    let mut previous_last = Some(prefix.clone());
    let mut emitted = Vec::new();
    for (last, cumulative) in events {
        let decision = select_incremental_call_usage(
            last.clone(),
            cumulative.clone(),
            previous_cumulative.as_ref(),
            previous_last.as_ref(),
        )
        .expect("representative fixture is valid");
        if let IncrementalTokenUsageDecision::Emit(selected) = decision {
            emitted.push(selected.usage);
            previous_cumulative = cumulative;
            previous_last = last;
        }
    }

    // 复制前缀 500 只建立基线；自有 last-copy 发出 20，same-last 与估算忽略，追加 10。
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].total_tokens, 20);
    assert_eq!(emitted[1].total_tokens, 10);

    let calls = emitted
        .into_iter()
        .enumerate()
        .map(|(index, usage)| UsageCall {
            logical_call_id: format!("owned-{index}"),
            occurred_at_epoch_ms: i64::try_from(index).expect("index fits"),
            model: Some("gpt-5".to_owned()),
            reasoning_effort: None,
            project_key: None,
            project_label: None,
            thread_key: "fork-owner".to_owned(),
            thread_label: None,
            usage,
            adapter_consistency_key: None,
            confidence: Confidence::Exact,
            provenance: vec![SourceProvenance {
                source_id: format!("source-{index}"),
                root_id: "root".to_owned(),
                relative_label: "sessions/fork.jsonl".to_owned(),
                archived: false,
            }],
        })
        .collect();
    let canonical = attach_session_snapshots(
        observed(calls),
        vec![snapshot(
            "fork-owner",
            4,
            owned_cumulative.total_tokens,
            "latest-cumulative",
            "fork",
        )],
    );
    let aggregate = aggregate_canonical_usage(&canonical).expect("owned aggregate");

    assert_eq!(aggregate.tokens.total_tokens, 30);
    assert_ne!(aggregate.tokens.total_tokens, prefix.total_tokens);
    assert_ne!(aggregate.tokens.total_tokens, owned_cumulative.total_tokens);
    assert_ne!(
        aggregate.tokens.total_tokens,
        FORBIDDEN_AUG12_SESSION_LATEST
    );
}

/// 根 110 与 fork 自有 20 相加得到 130；整文件累计 630 不得进入生产合计。
#[test]
fn root_and_owned_fork_calls_sum_without_copied_prefix() {
    let today = Date::new(2026, 8, 12).expect("date");
    let mut root = incremental_call("root", "root-owner", local_civil_ms(today, 7, 30, 0), 140);
    root.usage = TokenUsage::new(100, 40, None, 10, 2, Some(110)).expect("root usage is valid");
    let mut fork_owned =
        incremental_call("fork", "fork-owner", local_civil_ms(today, 8, 0, 2), 120);
    fork_owned.usage = TokenUsage::new(12, 4, None, 8, 3, Some(20)).expect("fork usage is valid");
    let canonical = attach_session_snapshots(
        observed(vec![root, fork_owned]),
        vec![
            snapshot(
                "root-owner",
                local_civil_ms(today, 7, 30, 0),
                110,
                "root-snap",
                "root",
            ),
            snapshot(
                "fork-owner",
                local_civil_ms(today, 8, 0, 2),
                630,
                "fork-whole-file",
                "fork",
            ),
        ],
    );
    let aggregate = aggregate_canonical_usage(&canonical).expect("owned root+fork");

    assert_eq!(aggregate.tokens.total_tokens, 130);
    assert_ne!(aggregate.tokens.total_tokens, 630);
    assert_ne!(aggregate.tokens.total_tokens, 500 + 130);
}

/// Claude / Grok 与 Codex 一样保留上游总量，不得因缓存子集反而小于输入加输出。
#[test]
fn claude_and_grok_keep_observed_totals() {
    let today = Date::new(2026, 8, 12).expect("date");
    let call = incremental_call("shared", "thread-a", local_civil_ms(today, 10, 0, 0), 120);
    for (client, provider) in [
        (
            SourceClientKind::ClaudeCode,
            ProviderKind::ClaudeTranscriptJsonl,
        ),
        (
            SourceClientKind::GrokBuildCli,
            ProviderKind::GrokSessionJsonl,
        ),
    ] {
        let page = build_usage_calls_page(
            std::slice::from_ref(&call),
            LocalIndexState::Ready,
            &BTreeMap::new(),
            &UsageCallsQuery::default(),
            local_civil_ms(today, 15, 0, 0),
            client,
            provider,
            None,
        )
        .expect("observed provider page");
        assert_eq!(page.items[0].usage.total_tokens, 120);
    }
}
