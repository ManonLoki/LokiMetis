//! 本文件的测试覆盖：构造不包含主机路径的合成 provenance；验证根签名只接受有时间和会话标识的首条 session 元数据；验证解析器优先使用单次事实，并且正文诱饵不会进入规范化调用。

use super::*;

/// 构造不包含主机路径的合成 provenance。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-a".to_owned(),
        root_id: "root-a".to_owned(),
        relative_label: "sessions/fixture.jsonl".to_owned(),
        archived: false,
    }
}

/// 验证根签名只接受有时间和会话标识的首条 session 元数据。
#[test]
fn rollout_signature_rejects_generic_or_malformed_json() {
    assert!(is_rollout_signature_line(
        br#"{"timestamp":"2026-07-31T08:00:00Z","type":"session_meta","payload":{"id":"session-a"}}"#
    ));
    assert!(!is_rollout_signature_line(
        br#"{"type":"session_meta","payload":{"id":"session-a"}}"#
    ));
    assert!(!is_rollout_signature_line(
        br#"{"timestamp":"2026-07-31T08:00:00Z","type":"event_msg","payload":{"type":"token_count"}}"#
    ));
    assert!(!is_rollout_signature_line(b"{not-json}"));
}

/// 验证解析器优先使用单次事实，并且正文诱饵不会进入规范化调用。
#[test]
fn parses_last_usage_without_retaining_private_content() {
    let input = concat!(
        "{\"timestamp\":\"2026-07-30T12:00:00.123Z\",\"type\":\"session_meta\",",
        "\"payload\":{\"id\":\"session-a\",\"cwd\":\"/private/secret\",",
        "\"prompt\":\"API_KEY=privacy-bait\"}}\n",
        "{\"timestamp\":\"2026-07-30T12:01:00Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"call_id\":\"call-a\",",
        "\"model\":\"gpt-5\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"output_tokens\":20,\"reasoning_output_tokens\":5,\"total_tokens\":120},",
        "\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,",
        "\"output_tokens\":200,\"reasoning_output_tokens\":50,\"total_tokens\":1200}},",
        "\"answer\":\"privacy-bait@example.com\"}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("fixture is parsed");

    assert_eq!(report.calls_emitted, 1);
    assert_eq!(calls[0].usage.input_tokens, 100);
    assert_eq!(calls[0].usage.total_tokens, 120);
    assert_eq!(calls[0].project_label.as_deref(), Some("secret"));
    assert!(!format!("{calls:?}").contains("privacy-bait"));
    assert!(!format!("{calls:?}").contains("/private/secret"));
}

/// 验证遗留 thread_name_updated 写入短标题，且 goal 事件不进入标签。
#[test]
fn parses_thread_name_updated_without_reading_goal_objective() {
    let input = concat!(
        "{\"timestamp\":\"2026-07-30T12:00:00Z\",\"type\":\"session_meta\",",
        "\"payload\":{\"id\":\"session-named\",\"cwd\":\"/Users/x/my-app\"}}\n",
        "{\"timestamp\":\"2026-07-30T12:00:01Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"thread_name_updated\",\"thread_name\":\"迁移看板\"}}\n",
        "{\"timestamp\":\"2026-07-30T12:00:02Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"thread_goal_updated\",\"goal\":{\"objective\":",
        "\"do not index this long goal objective text\"}}}\n",
        "{\"timestamp\":\"2026-07-30T12:01:00Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"call_id\":\"call-named\",",
        "\"info\":{\"last_token_usage\":{\"input_tokens\":10,\"cached_input_tokens\":0,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":0,\"total_tokens\":12}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture-named.jsonl");
    let mut calls = Vec::new();
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("named fixture is parsed");
    assert_eq!(report.calls_emitted, 1);
    assert_eq!(calls[0].project_label.as_deref(), Some("my-app"));
    assert_eq!(calls[0].thread_label.as_deref(), Some("迁移看板"));
    assert!(!format!("{calls:?}").contains("do not index"));
    assert!(!format!("{calls:?}").contains("/Users/x/my-app"));
}

/// 验证真实 rollout 顺序可从 `turn_context.effort` 继承模型与推理强度，且正常非用量事件不制造覆盖警告。
#[test]
fn parses_real_context_shape_and_ignores_known_non_usage_events() {
    let input = concat!(
        "{\"timestamp\":\"2026-07-31T08:00:00Z\",\"type\":\"session_meta\",",
        "\"payload\":{\"id\":\"session-real-shape\",\"cwd\":\"/privacy-bait\"}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:01Z\",\"type\":\"turn_context\",",
        "\"payload\":{\"model\":\"gpt-5.4\",\"effort\":\"high\"}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:02Z\",\"type\":\"response_item\",",
        "\"payload\":{\"type\":\"message\",\"content\":\"privacy-bait-answer\"}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:03Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"agent_message\",\"message\":\"privacy-bait-answer\"}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:04Z\",\"type\":\"world_state\",",
        "\"payload\":{}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:05Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"cache_write_input_tokens\":0,\"output_tokens\":20,",
        "\"reasoning_output_tokens\":5,\"total_tokens\":120},",
        "\"total_token_usage\":{\"input_tokens\":1000,\"cached_input_tokens\":400,",
        "\"cache_write_input_tokens\":0,\"output_tokens\":200,",
        "\"reasoning_output_tokens\":50,\"total_tokens\":1200},",
        "\"model_context_window\":258400},\"rate_limits\":null}}\n",
        "{\"timestamp\":\"2026-07-31T08:00:06Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"future_usage_signal\"}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("real rollout shape is parsed without retaining non-usage content");

    assert_eq!(report.calls_emitted, 1);
    assert_eq!(report.warnings.unknown_events, 1);
    assert_eq!(report.warnings.total(), 1);
    assert_eq!(calls[0].model.as_deref(), Some("gpt-5.4"));
    assert_eq!(calls[0].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(calls[0].usage.total_tokens, 120);
    assert_eq!(calls[0].project_label.as_deref(), Some("privacy-bait"));
    assert!(!calls[0].logical_call_id.is_empty());
    assert!(!format!("{calls:?}").contains("/privacy-bait"));
    assert!(!format!("{calls:?}").contains("privacy-bait-answer"));
}

/// 验证显式类路径模型与推理标签会清除旧上下文，不能进入索引或 GUI DTO。
#[test]
fn rejects_path_like_model_and_reasoning_labels() {
    let input = concat!(
        "{\"timestamp\":\"2026-07-30T12:00:00Z\",\"type\":\"session_meta\",",
        "\"payload\":{\"id\":\"session-a\",\"model\":\"gpt-5\",",
        "\"reasoningEffort\":\"high\"}}\n",
        "{\"timestamp\":\"2026-07-30T12:01:00Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"call_id\":\"call-a\",",
        "\"model\":\"/Users/alice/secret\",",
        "\"reasoningEffort\":\"C:/Users/alice/secret\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":5,\"cached_input_tokens\":1,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("path-like labels are removed without exposing content");

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].model, None);
    assert_eq!(calls[0].reasoning_effort, None);
    assert!(!format!("{calls:?}").contains("/Users/"));
    assert!(!format!("{calls:?}").contains("C:/"));
}

/// 凭据对象中的邮箱不得复制到 shipped `UsageCall`。
#[test]
fn does_not_copy_credential_email_onto_the_call() {
    let input = concat!(
        "{\"timestamp\":\"2026-07-30T12:00:00Z\",\"type\":\"session_meta\",",
        "\"payload\":{\"id\":\"session-a\",\"model\":\"gpt-5\",\"effort\":\"high\"}}\n",
        "{\"timestamp\":\"2026-07-30T12:01:00Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"call_id\":\"call-a\",\"model\":\"gpt-5\",",
        "\"auth\":{\"email\":\"alice@example.com\"},\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":5,\"cached_input_tokens\":1,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();
    parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("credential object is ignored");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].model.as_deref(), Some("gpt-5"));
    assert_eq!(calls[0].reasoning_effort.as_deref(), Some("high"));
    assert!(!format!("{calls:?}").contains("alice@example.com"));
}

/// 验证末尾半行不被提交或回调，调用方可从完整行边界重读。
#[test]
fn preserves_trailing_partial_line_as_offset_checkpoint() {
    let input = b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\"";
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");

    let report = parse_jsonl_stream(
        input.as_slice(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |_| Ok(()),
    )
    .expect("partial fixture does not fail");

    assert_eq!(report.checkpoint.committed_bytes, 0);
    assert_eq!(report.checkpoint.trailing_bytes, input.len() as u64);
    assert_eq!(report.calls_emitted, 0);
}

/// 验证超大行与损坏行被有界丢弃并计数，后续合法行仍可解析。
#[test]
fn counts_oversized_and_malformed_lines_without_stopping() {
    let valid = concat!(
        "{\"timestamp\":1,\"type\":\"event_msg\",\"payload\":{",
        "\"type\":\"token_count\",\"info\":{\"last_token_usage\":{",
        "\"input_tokens\":2,\"cached_input_tokens\":1,\"output_tokens\":1,",
        "\"reasoning_output_tokens\":0,\"total_tokens\":3}}}}\n"
    );
    let input = format!("{}\nnot-json\n{valid}", "x".repeat(40));
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut count = 0_u64;

    let report = parse_jsonl_stream(
        input.as_bytes(),
        32,
        &mut context,
        &provenance(),
        &cancellation,
        |_| {
            count += 1;
            Ok(())
        },
    )
    .expect("bounded parser continues");

    assert_eq!(report.warnings.oversized_lines, 2);
    assert_eq!(report.warnings.malformed_lines, 1);
    assert_eq!(
        count, 0,
        "valid line is intentionally above the tiny test bound"
    );
}

/// 验证缺少单次事实时累计值只作为显式降置信度回退。
#[test]
fn marks_cumulative_fallback_as_derived() {
    let input = concat!(
        "{\"timestamp\":1,\"type\":\"event_msg\",\"payload\":{",
        "\"type\":\"token_count\",\"info\":{\"total_token_usage\":{",
        "\"input_tokens\":5,\"cached_input_tokens\":1,\"output_tokens\":2,",
        "\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("fallback fixture is parsed");

    assert_eq!(report.warnings.cumulative_fallbacks, 1);
    assert_eq!(calls[0].confidence, loki_metris_core::Confidence::Derived);
}

/// 验证真实新增、额度重放与 compact 上下文估算在同一流中只产生两次调用。
#[test]
fn ignores_replayed_and_total_only_snapshots_before_next_real_call() {
    let input = concat!(
        "{\"timestamp\":\"2026-08-14T01:00:00Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"output_tokens\":20,\"reasoning_output_tokens\":5,\"total_tokens\":120},",
        "\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"output_tokens\":20,\"reasoning_output_tokens\":5,\"total_tokens\":120}}}}\n",
        "{\"timestamp\":\"2026-08-14T01:00:01Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"output_tokens\":20,\"reasoning_output_tokens\":5,\"total_tokens\":120},",
        "\"total_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,",
        "\"output_tokens\":20,\"reasoning_output_tokens\":5,\"total_tokens\":120}}}}\n",
        "{\"timestamp\":\"2026-08-14T01:00:02Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":0,\"cached_input_tokens\":0,",
        "\"output_tokens\":0,\"reasoning_output_tokens\":0,\"total_tokens\":50000},",
        "\"total_token_usage\":{\"input_tokens\":0,\"cached_input_tokens\":0,",
        "\"output_tokens\":0,\"reasoning_output_tokens\":0,\"total_tokens\":50000}}}}\n",
        "{\"timestamp\":\"2026-08-14T01:00:03Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":10,\"cached_input_tokens\":4,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":12},",
        "\"total_token_usage\":{\"input_tokens\":110,\"cached_input_tokens\":44,",
        "\"output_tokens\":22,\"reasoning_output_tokens\":6,\"total_tokens\":132}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("mixed snapshots are parsed");

    assert_eq!(report.calls_emitted, 2);
    assert_eq!(
        calls
            .iter()
            .map(|call| call.usage.total_tokens)
            .collect::<Vec<_>>(),
        vec![120, 12]
    );
    assert_eq!(context.call_sequence, 4);
    assert_eq!(
        context
            .previous_cumulative
            .as_ref()
            .map(|usage| usage.total_tokens),
        Some(132)
    );
    assert_eq!(report.snapshots.len(), 4);
    assert_eq!(
        report
            .snapshots
            .iter()
            .map(|snapshot| snapshot.cumulative_total_tokens)
            .collect::<Vec<_>>(),
        vec![120, 120, 50000, 132]
    );
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - tests_more.rs：验证 IgnoreNonIncremental 仍写出 cumulative 快照，且不推进增量基线。；验证累计检查点仅恢复合法当前版本，不接受截断、旧版或矛盾总量。；验证缺失或畸形时间戳会被跳过并单独计数，不能落…
include!("tests_more.rs");
