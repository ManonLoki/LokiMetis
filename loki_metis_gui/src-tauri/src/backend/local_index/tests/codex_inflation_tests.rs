//! 验证 Codex `token_count` 虚高模式在 JSONL 流式路径和隔离索引上的修复。
//! 夹具复现 `last` 复制 running total，以及 `last` 未变而 `total` 移动；
//! 不得读取真实 `~/.codex` 或官方账户。

use loki_metis_core::{SourceClientKind, SourceProvenance};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    append_rollout, codex_aggregate, create_root, discover_registered, scan, token_snapshot_line,
    write_rollout,
};
use crate::backend::local_index::jsonl::{
    DEFAULT_MAX_JSONL_LINE_BYTES, JsonlParseContext, parse_jsonl_stream,
};
use crate::backend::local_index::{CancellationToken, LocalIndex, PARSER_VERSION};

/// 在隔离 app-data 内按生产读取入口的 Codex parser generation 打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(
        app_data_dir,
        SourceClientKind::Codex.parser_version(),
    ))
    .expect("index opens")
}

/// 构造不包含主机路径的合成 provenance。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-inflation".to_owned(),
        root_id: "root-inflation".to_owned(),
        relative_label: "sessions/fixture.jsonl".to_owned(),
        archived: false,
    }
}

/// 根因夹具：五回合 `last_token_usage` 逐字复制增长中的 `total_token_usage`。
// 未修复的选择器会把 100+220+360+520+700 都当成新调用（三角数 1900），
// 相对会话最新非估算 total 700 虚高 2.71 倍。
fn last_copies_running_total_lines() -> Vec<String> {
    vec![
        token_snapshot_line("2026-08-17T01:00:00Z", 80, 20, 20, 100, 80, 20, 20, 100),
        token_snapshot_line("2026-08-17T01:01:00Z", 180, 50, 40, 220, 180, 50, 40, 220),
        token_snapshot_line("2026-08-17T01:02:00Z", 300, 90, 60, 360, 300, 90, 60, 360),
        token_snapshot_line("2026-08-17T01:03:00Z", 440, 140, 80, 520, 440, 140, 80, 520),
        token_snapshot_line(
            "2026-08-17T01:04:00Z",
            600,
            200,
            100,
            700,
            600,
            200,
            100,
            700,
        ),
    ]
}

/// 从同一 fixture 字段计算最新非估算 `total_token_usage`。
fn latest_non_estimate_total(lines: &[String]) -> u64 {
    lines
        .iter()
        .rev()
        .find_map(|line| {
            let total = parse_total_token_usage(line)?;
            let last = parse_last_token_usage(line)?;
            let estimate = last.0 == 0 && last.2 == 0 && last.3 > 0 && total.0 == 0 && total.2 == 0;
            (!estimate).then_some(total.3)
        })
        .unwrap_or(0)
}

/// 解析 fixture 行里的 `last_token_usage` 输入/缓存/输出/总量。
fn parse_last_token_usage(line: &str) -> Option<(u64, u64, u64, u64)> {
    parse_named_usage(line, "last_token_usage")
}

/// 解析 fixture 行里的 `total_token_usage` 输入/缓存/输出/总量。
fn parse_total_token_usage(line: &str) -> Option<(u64, u64, u64, u64)> {
    parse_named_usage(line, "total_token_usage")
}

/// 从合成 JSONL 行提取具名用量对象的四个整数，供对账而不是复述选择器。
fn parse_named_usage(line: &str, name: &str) -> Option<(u64, u64, u64, u64)> {
    let marker = format!("\"{name}\":{{");
    let start = line.find(&marker)? + marker.len();
    let slice = &line[start..];
    let input = extract_u64(slice, "input_tokens")?;
    let cached = extract_u64(slice, "cached_input_tokens")?;
    let output = extract_u64(slice, "output_tokens")?;
    let total = extract_u64(slice, "total_tokens")?;
    Some((input, cached, output, total))
}

/// 提取 JSON 对象片段中第一个具名无符号整数。
fn extract_u64(slice: &str, field: &str) -> Option<u64> {
    let marker = format!("\"{field}\":");
    let start = slice.find(&marker)? + marker.len();
    slice[start..]
        .split(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok())
}

/// 驱动已发布 JSONL 流式路径，返回发出次数与 `total_tokens` 合计。
fn emit_stream(lines: &[String]) -> (u64, u64) {
    let mut input = String::from(
        "{\"timestamp\":\"2026-08-17T00:59:00Z\",\"type\":\"session_meta\",\
         \"payload\":{\"id\":\"session-inflation\"}}\n",
    );
    for line in lines {
        input.push_str(line);
    }
    let mut context = JsonlParseContext::new("inflation.jsonl");
    let mut emitted_sum = 0_u64;
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            emitted_sum = emitted_sum.saturating_add(call.usage.total_tokens);
            Ok(())
        },
    )
    .expect("inflation fixture is parsed");
    (report.calls_emitted, emitted_sum)
}

/// 验证 last 复制 running total 的流式发出合计等于最新非估算累计，不是公式 D。
#[test]
fn jsonl_stream_last_copying_total_matches_latest_non_estimate_cumulative() {
    let lines = last_copies_running_total_lines();
    let latest = latest_non_estimate_total(&lines);
    let unfixed_sum: u64 = lines
        .iter()
        .filter_map(|line| parse_last_token_usage(line).map(|usage| usage.3))
        .sum();
    let (calls, emitted_sum) = emit_stream(&lines);

    assert!(
        unfixed_sum >= latest.saturating_mul(2),
        "stream fixture must reproduce >=2x unfixed inflation: {unfixed_sum} vs {latest}"
    );
    assert_eq!(calls, lines.len() as u64);
    assert_eq!(emitted_sum, latest);
}

/// 验证 same last + 变化 total 的额度快照不会额外入库，随后真实增量仍计一次。
#[test]
fn jsonl_stream_same_last_changed_total_then_real_turn() {
    let lines = vec![
        token_snapshot_line("2026-08-17T02:00:00Z", 80, 20, 20, 100, 80, 20, 20, 100),
        token_snapshot_line("2026-08-17T02:00:01Z", 80, 20, 20, 100, 160, 60, 40, 200),
        token_snapshot_line("2026-08-17T02:00:02Z", 80, 20, 20, 100, 240, 100, 60, 300),
        token_snapshot_line("2026-08-17T02:00:03Z", 80, 20, 20, 100, 320, 140, 80, 400),
        token_snapshot_line("2026-08-17T02:00:04Z", 20, 4, 10, 30, 100, 24, 30, 130),
    ];
    let latest = latest_non_estimate_total(&lines);
    let unfixed_sum = 100 * 4 + 30;
    let (calls, emitted_sum) = emit_stream(&lines);

    assert!(
        unfixed_sum >= latest.saturating_mul(2),
        "same-last fixture must reproduce >=2x unfixed inflation: {unfixed_sum} vs {latest}"
    );
    assert_eq!(calls, 2);
    assert_eq!(emitted_sum, latest);
}

/// 验证 last 复制 total 中间插入 same-last 与 compact 估算后，合计仍等于最新非估算 total。
// 估算 50000 若被写成增量基线，后续 last=total=112 会因小于 50000 被丢掉。
#[test]
fn jsonl_stream_last_copying_total_after_estimate_keeps_billed_baseline() {
    let lines = vec![
        token_snapshot_line("2026-08-17T03:00:00Z", 80, 20, 20, 100, 80, 20, 20, 100),
        token_snapshot_line("2026-08-17T03:00:01Z", 80, 20, 20, 100, 160, 60, 40, 200),
        token_snapshot_line("2026-08-17T03:00:02Z", 0, 0, 0, 50_000, 0, 0, 0, 50_000),
        token_snapshot_line("2026-08-17T03:00:03Z", 90, 22, 22, 112, 90, 22, 22, 112),
        token_snapshot_line("2026-08-17T03:00:04Z", 150, 40, 30, 180, 150, 40, 30, 180),
    ];
    let latest = latest_non_estimate_total(&lines);
    let mut input = String::from(
        "{\"timestamp\":\"2026-08-17T02:59:00Z\",\"type\":\"session_meta\",\
         \"payload\":{\"id\":\"session-estimate-baseline\"}}\n",
    );
    for line in &lines {
        input.push_str(line);
    }
    let mut context = JsonlParseContext::new("estimate-baseline.jsonl");
    let mut emitted_sum = 0_u64;
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            emitted_sum = emitted_sum.saturating_add(call.usage.total_tokens);
            Ok(())
        },
    )
    .expect("interleaved fixture is parsed");

    assert_eq!(report.calls_emitted, 3);
    assert_eq!(emitted_sum, latest);
    assert_eq!(latest, 180);
    assert_eq!(
        context
            .previous_cumulative
            .as_ref()
            .map(|usage| usage.total_tokens),
        Some(180),
        "ignored estimate must not remain the increment baseline"
    );
}

/// 验证隔离索引对 last-copies-total 夹具及追加重放+真实回合的合计正确，且旧 generation 不可读。
#[test]
fn index_last_copying_total_reopen_matches_latest_and_excludes_old_generation() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-inflation.jsonl");
    let initial = last_copies_running_total_lines();
    write_rollout(&rollout, "session-inflation", &initial);
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    assert_eq!(
        SourceClientKind::Codex.parser_version(),
        PARSER_VERSION,
        "production reads must use the same generation the JSONL writer stores"
    );
    let first = scan(&mut index, &roots, &CancellationToken::new());
    let first_aggregate = codex_aggregate(&mut index);
    let latest = latest_non_estimate_total(&initial);
    assert_eq!(first.call_count, initial.len() as u64);
    assert_eq!(first_aggregate.tokens.total_tokens, latest);

    drop(index);
    append_rollout(
        &rollout,
        &token_snapshot_line(
            "2026-08-17T01:05:00Z",
            600,
            200,
            100,
            700,
            600,
            200,
            100,
            700,
        ),
    );
    append_rollout(
        &rollout,
        &token_snapshot_line("2026-08-17T01:06:00Z", 30, 6, 8, 38, 630, 206, 108, 738),
    );
    let mut reopened = open_index(app_temp.path());
    let appended = scan(&mut reopened, &roots, &CancellationToken::new());
    let appended_aggregate = codex_aggregate(&mut reopened);
    let appended_latest = latest_non_estimate_total(&[token_snapshot_line(
        "2026-08-17T01:06:00Z",
        30,
        6,
        8,
        38,
        630,
        206,
        108,
        738,
    )]);

    assert_eq!(appended.calls_added, 1);
    assert_eq!(
        appended.call_count,
        (initial.len() as u64).saturating_add(1)
    );
    assert_eq!(appended_aggregate.tokens.total_tokens, appended_latest);

    let database_path = reopened.database_path().to_path_buf();
    {
        let connection = super::support::reopen_for_fixture(&database_path);
        super::support::execute_fixture_sql(
            &connection,
            &format!(
                "UPDATE source_files SET parser_version = {}",
                PARSER_VERSION.saturating_sub(1)
            ),
        );
    }
    let stale = block_on(reopened.canonical_calls()).expect("stale generation is readable");
    assert!(
        stale.calls.is_empty(),
        "previous parser generation must stay excluded from current reads"
    );
}
