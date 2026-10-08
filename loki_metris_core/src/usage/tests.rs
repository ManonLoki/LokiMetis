use super::*;

/// 验证缓存读取占比使用 Token 子集口径，并在零输入时返回不适用。
#[test]
fn calculates_cache_read_share_without_call_hit_claims() {
    let usage = TokenUsage::new(1_000, 250, None, 100, 20, Some(1_100)).expect("fixture is valid");
    let zero = TokenUsage::new(0, 0, None, 0, 0, Some(0)).expect("zero fixture is valid");

    assert_eq!(usage.cache_read_basis_points(), Some(2_500));
    assert_eq!(usage.uncached_input_tokens(), Some(750));
    assert_eq!(zero.cache_read_basis_points(), None);
}

/// 验证 provider 口径保留上游总量，缓存只作输入子集。
#[test]
fn accounts_observed_total_without_subtracting_cached_input() {
    let usage = TokenUsage::new(100, 40, None, 20, 5, Some(120)).expect("fixture is valid");

    assert_eq!(
        usage.accounted_total_tokens(TotalTokenAccounting::Observed),
        120
    );
    assert_eq!(usage.cached_input_tokens, Some(40));
}

/// 验证上游总量小于输入加输出时保守拒绝，不让矛盾事实进入聚合或 wire。
#[test]
fn rejects_total_below_input_plus_output() {
    assert_eq!(
        TokenUsage::new(100, 90, None, 10, 2, Some(20)),
        Err(TokenUsageError::TotalBelowInputAndOutput)
    );
}

/// 验证未知缓存与推理分项不会被构造或聚合为零。
#[test]
fn preserves_unavailable_token_components() {
    let cursor = TokenUsage::new_with_availability(20, None, None, 5, None, None)
        .expect("unavailable components are valid");
    let known = TokenUsage::new(10, 2, Some(1), 3, 1, None).expect("known usage is valid");
    let aggregate = TokenUsage::zero()
        .checked_add(&cursor)
        .and_then(|usage| usage.checked_add(&known))
        .expect("aggregate must not overflow");

    assert_eq!(cursor.uncached_input_tokens(), None);
    assert_eq!(cursor.had_cache_read(), None);
    assert_eq!(aggregate.cached_input_tokens, None);
    assert_eq!(aggregate.cache_write_input_tokens, None);
    assert_eq!(aggregate.reasoning_output_tokens, None);
}

/// 验证缓存输入字段矛盾会成为显式质量错误，而不是被静默截断。
#[test]
fn rejects_cached_input_above_input() {
    assert_eq!(
        TokenUsage::new(10, 11, None, 0, 0, None),
        Err(TokenUsageError::CachedInputExceedsInput)
    );
}

/// 验证同一事件同时包含单次和累计值时只选择单次事实。
#[test]
fn prefers_last_usage_over_cumulative_usage() {
    let last = TokenUsage::new(10, 5, None, 2, 1, Some(12)).expect("last is valid");
    let cumulative = TokenUsage::new(100, 50, None, 20, 10, Some(120)).expect("total is valid");

    let selected = select_single_call_usage(Some(last.clone()), Some(cumulative))
        .expect("last usage must be selected");

    assert_eq!(selected.usage, last);
    assert_eq!(selected.confidence, Confidence::Exact);
    assert!(!selected.used_cumulative_fallback);
}

/// 验证累计快照没有增长时，即使 `last` 仍存在也不能重复建立调用。
#[test]
fn ignores_replayed_last_usage_when_cumulative_is_unchanged() {
    let last = TokenUsage::new(10, 5, None, 2, 1, Some(12)).expect("last is valid");
    let cumulative = TokenUsage::new(100, 50, None, 20, 10, Some(120)).expect("total is valid");

    let decision = select_incremental_call_usage(
        Some(last),
        Some(cumulative.clone()),
        Some(&cumulative),
        None,
    )
    .expect("replay is classified");

    assert_eq!(
        decision,
        IncrementalTokenUsageDecision::IgnoreNonIncremental
    );
}

/// 验证 compact 后只填充上下文总量的估算不会冒充模型调用。
#[test]
fn ignores_total_only_context_estimate() {
    let estimate = TokenUsage::new(0, 0, Some(0), 0, 0, Some(200_000)).expect("estimate is valid");
    let cumulative = estimate.clone();

    let decision = select_incremental_call_usage(Some(estimate), Some(cumulative), None, None)
        .expect("estimate is classified");

    assert_eq!(
        decision,
        IncrementalTokenUsageDecision::IgnoreNonIncremental
    );
}

/// 验证累计快照增长时仍采用真实 `last`，不会把整个累计量重复相加。
#[test]
fn emits_fresh_last_usage_when_cumulative_advances() {
    let previous = TokenUsage::new(100, 50, None, 20, 10, Some(120)).expect("previous is valid");
    let last = TokenUsage::new(10, 5, None, 2, 1, Some(12)).expect("last is valid");
    let cumulative = TokenUsage::new(110, 55, None, 22, 11, Some(132)).expect("total is valid");

    let decision =
        select_incremental_call_usage(Some(last.clone()), Some(cumulative), Some(&previous), None)
            .expect("fresh usage is selected");

    assert_eq!(
        decision,
        IncrementalTokenUsageDecision::Emit(SelectedTokenUsage {
            usage: last,
            confidence: Confidence::Exact,
            used_cumulative_fallback: false,
        })
    );
}

/// 验证缺失 `last` 的旧格式仍显式回退累计值并保持 Derived 置信度。
#[test]
fn preserves_cumulative_fallback_when_last_is_missing() {
    let cumulative = TokenUsage::new(100, 50, None, 20, 10, Some(120)).expect("total is valid");

    let decision = select_incremental_call_usage(None, Some(cumulative.clone()), None, None)
        .expect("fallback is selected");

    assert_eq!(
        decision,
        IncrementalTokenUsageDecision::Emit(SelectedTokenUsage {
            usage: cumulative,
            confidence: Confidence::Derived,
            used_cumulative_fallback: true,
        })
    );
}

/// 一次 `token_count` 快照：`last` 与会话 `total`，字段均来自同一 fixture。
#[derive(Clone)]
struct TokenCountSnapshot {
    last: Option<TokenUsage>,
    cumulative: Option<TokenUsage>,
}

/// 按解析器同序折叠选择器，并返回发出的用量与最新非估算会话总量。
fn fold_incremental_snapshots(snapshots: &[TokenCountSnapshot]) -> (Vec<SelectedTokenUsage>, u64) {
    let mut previous_cumulative = None;
    let mut previous_last = None;
    let mut emitted = Vec::new();
    for snapshot in snapshots {
        let decision = select_incremental_call_usage(
            snapshot.last.clone(),
            snapshot.cumulative.clone(),
            previous_cumulative.as_ref(),
            previous_last.as_ref(),
        )
        .expect("fixture snapshots are valid");
        if let IncrementalTokenUsageDecision::Emit(selected) = decision {
            emitted.push(selected);
            if let Some(cumulative) = snapshot.cumulative.clone() {
                previous_cumulative = Some(cumulative);
            }
            if let Some(last) = snapshot.last.clone() {
                previous_last = Some(last);
            }
        }
    }
    let latest_non_estimate = snapshots
        .iter()
        .rev()
        .find_map(|snapshot| {
            snapshot.cumulative.as_ref().and_then(|usage| {
                (!is_total_only_context_estimate(usage)).then_some(usage.total_tokens)
            })
        })
        .unwrap_or(0);
    (emitted, latest_non_estimate)
}

/// 构造 `last` 与 `total` 完全相同的会话快照，复现 last 复制 running total。
fn last_copies_total_snapshot(
    input: u64,
    cached: u64,
    output: u64,
    total: u64,
) -> TokenCountSnapshot {
    let usage = TokenUsage::new(input, cached, Some(0), output, 0, Some(total))
        .expect("last-copies-total fixture is valid");
    TokenCountSnapshot {
        last: Some(usage.clone()),
        cumulative: Some(usage),
    }
}

/// 未修复时会把每个 running total 当作新调用：5 回合合计 1900，相对最新 700 虚高 2.71 倍。
// 根因：Codex 部分 `token_count` 把 `last_token_usage` 写成当前
// `total_token_usage` 的拷贝。选择器在累计变化时仍 emit 整个 `last`，
// 于是 100+220+360+520+700=1900，而会话最新非估算 total 只有 700。
#[test]
fn last_copying_running_total_sums_to_latest_non_estimate_total() {
    let snapshots = [
        last_copies_total_snapshot(80, 20, 20, 100),
        last_copies_total_snapshot(180, 50, 40, 220),
        last_copies_total_snapshot(300, 90, 60, 360),
        last_copies_total_snapshot(440, 140, 80, 520),
        last_copies_total_snapshot(600, 200, 100, 700),
    ];
    let unfixed_sum: u64 = snapshots
        .iter()
        .filter_map(|snapshot| snapshot.last.as_ref().map(|usage| usage.total_tokens))
        .sum();
    let (emitted, latest_non_estimate) = fold_incremental_snapshots(&snapshots);
    let emitted_sum: u64 = emitted
        .iter()
        .map(|selected| selected.usage.total_tokens)
        .sum();

    assert!(
        unfixed_sum >= latest_non_estimate.saturating_mul(2),
        "fixture must reproduce >=2x inflation on unfixed last-as-total emits: {unfixed_sum} vs {latest_non_estimate}"
    );
    assert_eq!(emitted.len(), snapshots.len());
    assert_eq!(emitted_sum, latest_non_estimate);
    assert!(
        !emitted
            .iter()
            .any(|selected| selected.used_cumulative_fallback)
    );
}

/// `last` 未变而 `total` 因额度/compact 快照变化时，不得把同一 last 再计为新调用。
#[test]
fn unchanged_last_with_moved_total_is_not_an_extra_call() {
    let first = TokenUsage::new(80, 20, Some(0), 20, 0, Some(100)).expect("first last is valid");
    let extras = [
        TokenUsage::new(160, 60, Some(0), 40, 0, Some(200)).expect("moved total 1 is valid"),
        TokenUsage::new(240, 100, Some(0), 60, 0, Some(300)).expect("moved total 2 is valid"),
        TokenUsage::new(320, 140, Some(0), 80, 0, Some(400)).expect("moved total 3 is valid"),
    ];
    let next_last = TokenUsage::new(20, 4, Some(0), 10, 0, Some(30)).expect("next last is valid");
    let next_total =
        TokenUsage::new(100, 24, Some(0), 30, 0, Some(130)).expect("next total is valid");
    let snapshots = [
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(first.clone()),
        },
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(extras[0].clone()),
        },
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(extras[1].clone()),
        },
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(extras[2].clone()),
        },
        TokenCountSnapshot {
            last: Some(next_last.clone()),
            cumulative: Some(next_total),
        },
    ];
    let unfixed_sum = first.total_tokens * 4 + next_last.total_tokens;
    let (emitted, latest_non_estimate) = fold_incremental_snapshots(&snapshots);
    let emitted_sum: u64 = emitted
        .iter()
        .map(|selected| selected.usage.total_tokens)
        .sum();

    assert!(
        unfixed_sum >= latest_non_estimate.saturating_mul(2),
        "fixture must reproduce >=2x inflation on unfixed same-last emits: {unfixed_sum} vs {latest_non_estimate}"
    );
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].usage, first);
    assert_eq!(emitted[1].usage, next_last);
    assert_eq!(emitted_sum, latest_non_estimate);
}

/// 累计完全重放仍忽略；total-only 估算之后的真实增量 `last` 仍只发出一次。
#[test]
fn replay_and_estimate_then_real_turn_keep_existing_v6_behavior() {
    let first = TokenUsage::new(80, 20, Some(0), 20, 0, Some(100)).expect("first is valid");
    let estimate = TokenUsage::new(0, 0, Some(0), 0, 0, Some(50_000)).expect("estimate is valid");
    let next_last = TokenUsage::new(10, 4, Some(0), 2, 0, Some(12)).expect("next last is valid");
    let next_total =
        TokenUsage::new(90, 24, Some(0), 22, 0, Some(112)).expect("next total is valid");
    let snapshots = [
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(first.clone()),
        },
        TokenCountSnapshot {
            last: Some(first.clone()),
            cumulative: Some(first.clone()),
        },
        TokenCountSnapshot {
            last: Some(estimate.clone()),
            cumulative: Some(estimate),
        },
        TokenCountSnapshot {
            last: Some(next_last.clone()),
            cumulative: Some(next_total),
        },
    ];
    let (emitted, latest_non_estimate) = fold_incremental_snapshots(&snapshots);
    let emitted_sum: u64 = emitted
        .iter()
        .map(|selected| selected.usage.total_tokens)
        .sum();

    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].usage, first);
    assert_eq!(emitted[1].usage, next_last);
    assert_eq!(emitted_sum, latest_non_estimate);
    assert!(
        !emitted
            .iter()
            .any(|selected| selected.used_cumulative_fallback)
    );
}

/// last 复制 total 中间插入 same-last 与 compact 估算时，增量必须相对上次已计费累计。
// 若把 50000 估算写进 previous_cumulative，后续 last=total=112 会因 112<50000 被丢掉，
// 合计停在 100，对不上最新非估算 total 112。
#[test]
fn last_copying_total_after_estimate_and_same_last_matches_latest_non_estimate() {
    let first = last_copies_total_snapshot(80, 20, 20, 100);
    let moved_total =
        TokenUsage::new(160, 60, Some(0), 40, 0, Some(200)).expect("moved total is valid");
    let estimate = TokenUsage::new(0, 0, Some(0), 0, 0, Some(50_000)).expect("estimate is valid");
    let snapshots = [
        first.clone(),
        TokenCountSnapshot {
            last: first.last.clone(),
            cumulative: Some(moved_total),
        },
        TokenCountSnapshot {
            last: Some(estimate.clone()),
            cumulative: Some(estimate),
        },
        last_copies_total_snapshot(90, 22, 22, 112),
        last_copies_total_snapshot(150, 40, 30, 180),
    ];
    let (emitted, latest_non_estimate) = fold_incremental_snapshots(&snapshots);
    let emitted_sum: u64 = emitted
        .iter()
        .map(|selected| selected.usage.total_tokens)
        .sum();

    assert_eq!(emitted.len(), 3);
    assert_eq!(emitted[0].usage.total_tokens, 100);
    assert_eq!(emitted[1].usage.total_tokens, 12);
    assert_eq!(emitted[2].usage.total_tokens, 68);
    assert_eq!(emitted_sum, latest_non_estimate);
    assert_eq!(latest_non_estimate, 180);
}

/// last 复制累计但会话总量下降时不得用减法伪造新调用。
#[test]
fn ignores_decreasing_last_copy_of_session_total_after_compact() {
    let first = last_copies_total_snapshot(80, 20, 20, 100);
    let rolled_back = last_copies_total_snapshot(40, 10, 10, 50);
    let (emitted, _) = fold_incremental_snapshots(&[first.clone(), rolled_back]);

    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].usage.total_tokens, 100);
    assert_eq!(
        first.last.as_ref().map(|usage| usage.total_tokens),
        Some(100)
    );
}
