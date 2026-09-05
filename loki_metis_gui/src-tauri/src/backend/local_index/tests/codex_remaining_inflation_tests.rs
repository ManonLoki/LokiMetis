//! 驱动 shipped checkpoint restore 后再吃一条 last-copies / same-last `token_count`，
//! 确认已计费 Token 不会在增量续读时重发。不读真实 `~/.codex`。

use loki_metis_core::{
    IncrementalTokenUsageDecision, SourceProvenance, TokenUsage, select_incremental_call_usage,
};

use super::support::token_snapshot_line;
use crate::backend::local_index::CancellationToken;
use crate::backend::local_index::jsonl::{
    DEFAULT_MAX_JSONL_LINE_BYTES, JsonlParseContext, parse_jsonl_stream,
    restore_incremental_checkpoint,
};

/// 构造不含主机路径的合成 provenance。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-remaining-resume".to_owned(),
        root_id: "root-remaining-resume".to_owned(),
        relative_label: "sessions/resume.jsonl".to_owned(),
        archived: false,
    }
}

/// last 复制 running total 的前缀，供写入检查点。
fn last_copies_prefix_lines() -> Vec<String> {
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

/// 与最后一条完全相同的 last-copies 重放行。
fn last_copies_replay_line() -> String {
    token_snapshot_line(
        "2026-08-17T01:05:00Z",
        600,
        200,
        100,
        700,
        600,
        200,
        100,
        700,
    )
}

/// same-last 但 total 继续移动的额度快照行。
fn same_last_moved_total_line() -> String {
    token_snapshot_line(
        "2026-08-17T01:05:01Z",
        600,
        200,
        100,
        700,
        680,
        240,
        120,
        800,
    )
}

/// 解析前缀并返回已发出合计与最后一次 shipped 检查点。
fn parse_prefix() -> (u64, u64, String) {
    let mut input = String::from(
        "{\"timestamp\":\"2026-08-17T00:59:00Z\",\"type\":\"session_meta\",\
         \"payload\":{\"id\":\"session-remaining-resume\"}}\n",
    );
    for line in last_copies_prefix_lines() {
        input.push_str(&line);
    }
    let mut context = JsonlParseContext::new("remaining-resume.jsonl");
    let mut billed = 0_u64;
    let mut last_key = None;
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            billed = billed.saturating_add(call.usage.total_tokens);
            last_key = call.adapter_consistency_key.clone();
            Ok(())
        },
    )
    .expect("prefix last-copies file is parsed");
    let key = last_key.expect("emitted call must carry adapter_consistency_key");
    (report.calls_emitted, billed, key)
}

/// 从 shipped 检查点恢复 previous_* 后再解析一行，返回续读发出次数与合计。
fn resume_parse_one_line(checkpoint: &str, line: &str) -> (u64, u64) {
    let restored =
        restore_incremental_checkpoint(Some(checkpoint)).expect("shipped checkpoint restores");
    let mut context = JsonlParseContext::new("remaining-resume-tail.jsonl");
    context.previous_cumulative = Some(restored.cumulative.clone());
    context.previous_last = restored.last.clone();
    let mut billed = 0_u64;
    let report = parse_jsonl_stream(
        line.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            billed = billed.saturating_add(call.usage.total_tokens);
            Ok(())
        },
    )
    .expect("resume tail line is parsed");
    (report.calls_emitted, billed)
}

/// 验证 restore 后的 previous_* 交给 shipped 选择器时，last-copies 重放被忽略。
fn selector_after_restore(checkpoint: &str) -> IncrementalTokenUsageDecision {
    let restored =
        restore_incremental_checkpoint(Some(checkpoint)).expect("shipped checkpoint restores");
    let replay = TokenUsage::new(600, 200, Some(0), 100, 0, Some(700))
        .expect("replay last-copies usage is valid");
    select_incremental_call_usage(
        Some(replay.clone()),
        Some(replay),
        Some(&restored.cumulative),
        restored.last.as_ref(),
    )
    .expect("replay snapshot is valid")
}

/// 验证增量续读恢复检查点后再遇到 last-copies / same-last 行不会重计已账单。
#[test]
fn resume_after_checkpoint_does_not_rebill_last_copies_or_same_last() {
    let (prefix_calls, prefix_total, key) = parse_prefix();
    let restored = restore_incremental_checkpoint(Some(&key)).expect("prefix checkpoint restores");
    let (replay_calls, replay_total) = resume_parse_one_line(&key, &last_copies_replay_line());
    let (moved_calls, moved_total) = resume_parse_one_line(&key, &same_last_moved_total_line());
    let selector = selector_after_restore(&key);

    eprintln!(
        "resume prefix_calls={prefix_calls} prefix_total={prefix_total} restored_cumulative={} restored_last={} replay_calls={replay_calls} replay_total={replay_total} same_last_moved_calls={moved_calls} same_last_moved_total={moved_total} selector_rebills={}",
        restored.cumulative.total_tokens,
        restored
            .last
            .as_ref()
            .map(|usage| usage.total_tokens)
            .unwrap_or(0),
        !matches!(
            selector,
            IncrementalTokenUsageDecision::IgnoreNonIncremental
        )
    );

    assert_eq!(prefix_total, restored.cumulative.total_tokens);
    assert_eq!(replay_calls, 0);
    assert_eq!(replay_total, 0);
    assert_eq!(moved_calls, 0);
    assert_eq!(moved_total, 0);
    assert_eq!(
        selector,
        IncrementalTokenUsageDecision::IgnoreNonIncremental,
        "already-billed last-copies tokens must not be re-emitted after restore"
    );
}
