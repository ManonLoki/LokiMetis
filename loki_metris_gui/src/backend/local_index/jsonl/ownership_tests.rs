//! fork/subagent rollout 所有权边界与增量恢复回归测试。

use super::*;

/// 构造不包含主机路径的合成来源。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-owned-segment".to_owned(),
        root_id: "root-owned-segment".to_owned(),
        relative_label: "sessions/owned-segment.jsonl".to_owned(),
        archived: false,
    }
}

/// fork 首条元数据必须冻结为文件 owner，复制进来的父会话元数据不得改写。
fn fork_owner_line() -> &'static str {
    concat!(
        r#"{"timestamp":"2026-08-12T08:00:00Z","type":"session_meta","payload":{"id":"fork-owner","agent_path":["parent","child"]}}"#,
        "\n"
    )
}

/// 构造复制父前缀中的元数据、旧任务边界和首个累计基线。
fn copied_parent_prefix() -> &'static str {
    concat!(
        r#"{"timestamp":"2026-08-12T08:00:00.100Z","type":"session_meta","payload":{"id":"copied-parent","cwd":"/private/parent"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:00.200Z","type":"event_msg","payload":{"type":"task_started","started_at":"2026-08-12T07:00:00Z"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:00.300Z","type":"event_msg","payload":{"type":"token_count","call_id":"copied-parent-call","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#,
        "\n"
    )
}

/// 构造与 fork owner 创建时刻相符的自身边界及首个自身调用。
fn owned_continuation() -> &'static str {
    concat!(
        r#"{"timestamp":"2026-08-12T08:00:01Z","type":"event_msg","payload":{"type":"task_started","started_at":"2026-08-12T08:00:00.500Z"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:02Z","type":"event_msg","payload":{"type":"token_count","call_id":"owned-call","info":{"last_token_usage":{"input_tokens":12,"cached_input_tokens":4,"output_tokens":8,"reasoning_output_tokens":3,"total_tokens":20},"total_token_usage":{"input_tokens":112,"cached_input_tokens":44,"output_tokens":28,"reasoning_output_tokens":8,"total_tokens":140}}}}"#,
        "\n"
    )
}

/// 运行一个内存片段并返回报告与调用，避免测试绕过生产流式入口。
fn parse_segment(
    input: &str,
    context: &mut JsonlParseContext,
) -> (JsonlParseReport, Vec<UsageCall>) {
    let mut calls = Vec::new();
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("owned segment fixture parses");
    (report, calls)
}

/// fork 文件只发出自身开始边界后的用量，复制父前缀只建立差量基线。
#[test]
fn excludes_copied_parent_prefix_and_emits_owned_usage() {
    let input = format!(
        "{}{}{}",
        fork_owner_line(),
        copied_parent_prefix(),
        owned_continuation()
    );
    let mut context = JsonlParseContext::new("owned-segment.jsonl");
    let (report, calls) = parse_segment(&input, &mut context);

    assert_eq!(report.calls_emitted, 1);
    assert_eq!(report.warnings.unresolved_owned_boundaries, 0);
    assert_eq!(report.snapshots.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 20);
    assert_eq!(calls[0].thread_key, stable_id("thread", "fork-owner"));
    assert_ne!(calls[0].thread_key, stable_id("thread", "copied-parent"));
    assert_eq!(context.call_sequence, 1);
    assert_eq!(
        context
            .previous_cumulative
            .as_ref()
            .map(|usage| usage.total_tokens),
        Some(140)
    );
}

/// 无法证明自身起点的 fork 必须保守丢弃全部用量并形成覆盖警告。
#[test]
fn unresolved_fork_boundary_never_emits_copied_usage() {
    let input = format!("{}{}", fork_owner_line(), copied_parent_prefix());
    let mut context = JsonlParseContext::new("unresolved-fork.jsonl");
    let (report, calls) = parse_segment(&input, &mut context);

    assert!(calls.is_empty());
    assert_eq!(report.calls_emitted, 0);
    assert_eq!(report.snapshots.len(), 0);
    assert_eq!(report.warnings.unresolved_owned_boundaries, 1);
    assert_eq!(report.warnings.total(), 1);
    assert_eq!(
        context
            .previous_cumulative
            .as_ref()
            .map(|usage| usage.total_tokens),
        Some(120)
    );
}

/// 增量扫描必须持久化等待状态与复制前缀基线，续扫后只发出自身差量。
#[test]
fn checkpoint_restores_pending_boundary_and_prefix_baseline() {
    let prefix = format!("{}{}", fork_owner_line(), copied_parent_prefix());
    let mut context = JsonlParseContext::new("checkpointed-fork.jsonl");
    let (prefix_report, prefix_calls) = parse_segment(&prefix, &mut context);
    assert!(prefix_calls.is_empty());
    assert_eq!(prefix_report.warnings.unresolved_owned_boundaries, 1);

    let adapter_state = context.adapter_state().expect("state encodes");
    assert!(!adapter_state.contains("fork-owner"));
    assert!(!adapter_state.contains("copied-parent"));
    let checkpoint = SourceParseCheckpoint {
        thread_key: context.thread_key.clone(),
        project_key: context.project_key.clone(),
        project_label: context.project_label.clone(),
        thread_label: context.thread_label.clone(),
        model: context.model.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        call_sequence: context.call_sequence,
        adapter_state: Some(adapter_state),
    };
    let mut restored =
        JsonlParseContext::from_checkpoint(checkpoint).expect("current state restores");
    let (continuation_report, calls) = parse_segment(owned_continuation(), &mut restored);

    assert_eq!(continuation_report.calls_emitted, 1);
    assert_eq!(continuation_report.warnings.unresolved_owned_boundaries, 0);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 20);
    assert_eq!(calls[0].thread_key, stable_id("thread", "fork-owner"));
    assert_eq!(restored.call_sequence, 1);
}
