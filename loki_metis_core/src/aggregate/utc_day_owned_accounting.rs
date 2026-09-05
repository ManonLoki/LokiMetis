//! UTC 日成员与自有 raw 合计：8/11–8/13 边界、same-cumulative Ignore 不得并入。

use crate::{
    Confidence, IncrementalTokenUsageDecision, SourceProvenance, TimeStandard, TokenUsage,
    TotalTokenAccounting, UsageCall, aggregate_canonical_usage, canonicalize_usage_calls,
    filter_canonical_usage_for_date, select_incremental_call_usage,
};
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};

/// 被 ADR-104 覆盖的整文件前缀合计，生产 UTC 日合计不得回退到该数。
const FORBIDDEN_WHOLE_FILE_PREFIX: u64 = 1_731_727_909;
/// 被 ADR-104 覆盖的会话 latest 合计，生产 UTC 日合计不得回退到该数。
const FORBIDDEN_SESSION_LATEST: u64 = 578_438_961;

/// 把 UTC 民用时刻转为 Unix 毫秒。
fn utc_civil_ms(year: i16, month: i8, day: i8, hour: i8, minute: i8) -> i64 {
    Date::new(year, month, day)
        .expect("date is valid")
        .at(hour, minute, 0, 0)
        .to_zoned(TimeZone::UTC)
        .expect("UTC instant is representable")
        .timestamp()
        .as_millisecond()
}

/// 构造一条已选定的 Codex 自有调用。
fn owned_call(id: &str, occurred_at_epoch_ms: i64, total_tokens: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(),
        occurred_at_epoch_ms,
        model: Some("gpt-5".to_owned()),
        reasoning_effort: Some("medium".to_owned()),
        project_key: Some("proj".to_owned()),
        project_label: Some("proj".to_owned()),
        thread_key: "thread-aug12".to_owned(),
        thread_label: None,
        usage: TokenUsage::new(10, 4, None, 2, 1, Some(total_tokens))
            .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{id}"),
            root_id: "root".to_owned(),
            relative_label: "sessions/rollout.jsonl".to_owned(),
            archived: false,
        }],
    }
}

/// 2026-08-12 UTC 日只含真实 UTC 瞬间，不受 UTC+8 本地日成员影响。
#[test]
fn utc_day_aug12_membership_is_independent_of_utc8_local_day() {
    let day = Date::new(2026, 8, 12).expect("lock 2026-08-12");
    let tz8 = TimeZone::fixed(offset(8));
    let local_midnight = utc_civil_ms(2026, 8, 11, 16, 0);
    let utc_morning = utc_civil_ms(2026, 8, 12, 0, 30);
    let utc_evening = utc_civil_ms(2026, 8, 12, 16, 0);
    let canonical = canonicalize_usage_calls(vec![
        owned_call("local-midnight", local_midnight, 100),
        owned_call("utc-morning", utc_morning, 200),
        owned_call("utc-evening", utc_evening, 300),
    ])
    .with_total_token_accounting(TotalTokenAccounting::Observed);

    let utc_day = filter_canonical_usage_for_date(&canonical, day, &TimeStandard::utc(), &tz8);
    let local_day = filter_canonical_usage_for_date(&canonical, day, &TimeStandard::Local, &tz8);
    let utc_total = aggregate_canonical_usage(&utc_day)
        .expect("utc aggregate")
        .tokens
        .total_tokens;
    let local_total = aggregate_canonical_usage(&local_day)
        .expect("local aggregate")
        .tokens
        .total_tokens;

    assert_eq!(utc_total, 500);
    assert_eq!(local_total, 300);
    assert_eq!(utc_day.calls.len(), 2);
    assert_eq!(local_day.calls.len(), 2);
    assert!(
        utc_day
            .calls
            .iter()
            .all(|call| call.logical_call_id != "local-midnight")
    );
    assert!(
        local_day
            .calls
            .iter()
            .all(|call| call.logical_call_id != "utc-evening")
    );
    assert_ne!(utc_total, FORBIDDEN_WHOLE_FILE_PREFIX);
    assert_ne!(utc_total, FORBIDDEN_SESSION_LATEST);
    assert_ne!(utc_total, local_total);
}

/// same-cumulative 重放的 last.total 不得并入 UTC 日自有合计。
#[test]
fn utc_day_owned_sum_excludes_same_cumulative_replay_last() {
    let day = Date::new(2026, 8, 12).expect("lock 2026-08-12");
    let tz8 = TimeZone::fixed(offset(8));
    let occurred_at = utc_civil_ms(2026, 8, 12, 3, 28);
    let first = TokenUsage::new(10, 4, None, 2, 1, Some(12)).expect("first last is valid");
    let replay = first.clone();
    let next_last = TokenUsage::new(6, 2, None, 2, 1, Some(8)).expect("next last is valid");
    let next_cumulative = TokenUsage::new(16, 6, None, 4, 2, Some(20)).expect("next cumulative");
    let mut previous_cumulative = None;
    let mut previous_last = None;
    let mut emitted = Vec::new();
    let snapshots = [
        (Some(first.clone()), Some(first.clone())),
        (Some(replay), Some(first.clone())),
        (Some(next_last.clone()), Some(next_cumulative.clone())),
    ];
    for (last, cumulative) in snapshots {
        let decision = select_incremental_call_usage(
            last.clone(),
            cumulative.clone(),
            previous_cumulative.as_ref(),
            previous_last.as_ref(),
        )
        .expect("fixture snapshots are valid");
        if let IncrementalTokenUsageDecision::Emit(selected) = decision {
            emitted.push(selected.usage);
            previous_cumulative = cumulative;
            previous_last = last;
        }
    }

    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].total_tokens, 12);
    assert_eq!(emitted[1].total_tokens, 8);
    let replay_last_total = first.total_tokens;
    let naive_last_sum = emitted
        .iter()
        .map(|usage| usage.total_tokens)
        .sum::<u64>()
        .saturating_add(replay_last_total);
    assert_eq!(naive_last_sum, 32);

    let calls = emitted
        .into_iter()
        .enumerate()
        .map(|(index, usage)| {
            let mut call = owned_call(&format!("owned-{index}"), occurred_at, 12);
            call.usage = usage;
            call
        })
        .collect();
    let canonical =
        canonicalize_usage_calls(calls).with_total_token_accounting(TotalTokenAccounting::Observed);
    let utc_day = filter_canonical_usage_for_date(&canonical, day, &TimeStandard::utc(), &tz8);
    let utc_total = aggregate_canonical_usage(&utc_day)
        .expect("utc aggregate")
        .tokens
        .total_tokens;

    assert_eq!(utc_total, 20);
    assert_ne!(utc_total, naive_last_sum);
    assert_ne!(utc_total, FORBIDDEN_WHOLE_FILE_PREFIX);
    assert_ne!(utc_total, FORBIDDEN_SESSION_LATEST);
}
