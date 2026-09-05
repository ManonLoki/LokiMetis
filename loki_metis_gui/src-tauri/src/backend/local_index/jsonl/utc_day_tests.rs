//! 用 shipped parser 解析 8/11–8/13 UTC 边界 rollout，再按 UTC/本地日过滤。

use super::*;
use loki_metis_core::{
    TimeStandard, TotalTokenAccounting, aggregate_canonical_usage, canonicalize_usage_calls,
    filter_canonical_usage_for_date,
};
use jiff::civil::Date;
use jiff::tz::{TimeZone, offset};

/// 被 ADR-104 禁止回退的整文件前缀合计。
const FORBIDDEN_WHOLE_FILE_PREFIX: u64 = 1_731_727_909;
/// 被 ADR-104 禁止回退的会话 latest 合计。
const FORBIDDEN_SESSION_LATEST: u64 = 578_438_961;

/// 构造不包含主机路径的合成来源。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-utc-day".to_owned(),
        root_id: "root-utc-day".to_owned(),
        relative_label: "sessions/utc-day.jsonl".to_owned(),
        archived: false,
    }
}

/// 生成一条带累计的 token_count，供增量选择器识别重放。
#[allow(clippy::too_many_arguments)]
fn token_line(
    timestamp: &str,
    call_id: &str,
    last_input: u64,
    last_cached: u64,
    last_output: u64,
    last_total: u64,
    cum_input: u64,
    cum_cached: u64,
    cum_output: u64,
    cum_total: u64,
) -> String {
    format!(
        "{{\"timestamp\":\"{timestamp}\",\"type\":\"event_msg\",\"payload\":{{\
         \"type\":\"token_count\",\"call_id\":\"{call_id}\",\"info\":{{\
         \"last_token_usage\":{{\"input_tokens\":{last_input},\
         \"cached_input_tokens\":{last_cached},\"output_tokens\":{last_output},\
         \"reasoning_output_tokens\":0,\"total_tokens\":{last_total}}},\
         \"total_token_usage\":{{\"input_tokens\":{cum_input},\
         \"cached_input_tokens\":{cum_cached},\"output_tokens\":{cum_output},\
         \"reasoning_output_tokens\":0,\"total_tokens\":{cum_total}}}}}}}}}\n"
    )
}

/// 运行一个内存片段并返回 shipped parser 发出的调用。
fn parse_owned(input: &str) -> Vec<UsageCall> {
    let mut context = JsonlParseContext::new("utc-day.jsonl");
    let mut calls = Vec::new();
    parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("utc-day fixture parses");
    calls
}

/// UTC 日 8/12 含 00:30Z 与 16:00Z，不含 8/11 16:00Z；same-cumulative 与复制前缀不入账。
#[test]
fn parser_utc_day_aug12_excludes_local_midnight_prefix_and_replay() {
    let mut input = String::from(
        "{\"timestamp\":\"2026-08-12T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"fork-owner\",\"agent_path\":[\"parent\",\"child\"]}}\n\
         {\"timestamp\":\"2026-08-12T08:00:00.100Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"copied-parent\"}}\n\
         {\"timestamp\":\"2026-08-12T08:00:00.200Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"started_at\":\"2026-08-12T07:00:00Z\"}}\n",
    );
    input.push_str(&token_line(
        "2026-08-12T08:00:00.300Z",
        "copied-prefix",
        400,
        300,
        100,
        500,
        400,
        300,
        100,
        500,
    ));
    input.push_str(
        "{\"timestamp\":\"2026-08-12T08:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"started_at\":\"2026-08-12T08:00:00.500Z\"}}\n",
    );
    input.push_str(&token_line(
        "2026-08-11T16:00:00Z",
        "local-midnight",
        80,
        20,
        20,
        100,
        480,
        320,
        120,
        600,
    ));
    input.push_str(&token_line(
        "2026-08-12T00:30:00Z",
        "utc-morning",
        160,
        40,
        40,
        200,
        640,
        360,
        160,
        800,
    ));
    input.push_str(&token_line(
        "2026-08-12T00:30:01Z",
        "same-cumulative-replay",
        160,
        40,
        40,
        200,
        640,
        360,
        160,
        800,
    ));
    input.push_str(&token_line(
        "2026-08-12T16:00:00Z",
        "utc-evening",
        240,
        60,
        60,
        300,
        880,
        420,
        220,
        1_100,
    ));

    let calls = parse_owned(&input);
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.usage.total_tokens != 500));

    let day = Date::new(2026, 8, 12).expect("lock 2026-08-12");
    let tz8 = TimeZone::fixed(offset(8));
    let canonical =
        canonicalize_usage_calls(calls).with_total_token_accounting(TotalTokenAccounting::Observed);
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
    assert_ne!(utc_total, FORBIDDEN_WHOLE_FILE_PREFIX);
    assert_ne!(utc_total, FORBIDDEN_SESSION_LATEST);
    assert_ne!(utc_total, local_total);
}
