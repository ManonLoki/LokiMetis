// 本文件是 owned_segment_accounting.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：复制前缀只作为已交付选择器的基线；自有 last-copy / same-last / total-only 后合计不含前缀。；根 110 与 fork 自有 20 相加得到 130；整文件累计 630 不得进入生产合计。；Claude / Grok 与 Codex 一样保留上游总量，不得因缓存子集反而小于输入加输出。

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
