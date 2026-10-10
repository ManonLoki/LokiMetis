//! 使用合成响应验证新旧 Codex 用量共存，不读取真实会话。

use serde_json::json;

use super::*;

/// 构造已提供全部分项的单响应事实。
fn usage(input: u64, output: u64) -> Value {
    json!({"input_tokens": input, "cached_input_tokens": 0,
        "cache_write_input_tokens": 0, "output_tokens": output,
        "reasoning_output_tokens": 0, "total_tokens": input + output})
}

/// 构造当前线程与根会话身份可不同的元数据。
fn meta(owner: &str, root: &str) -> Value {
    json!({"timestamp":"2026-10-11T00:00:00Z", "type":"session_meta",
        "payload":{"id":owner, "session_id":root}})
}

/// 构造有稳定响应身份、单次与累计相互独立的新记录。
fn response(owner: &str, id: &str, input: u64, output: u64) -> Value {
    json!({"timestamp":"2026-10-11T00:00:01Z", "type":"token_usage_record",
        "payload":{"thread_id":owner, "session_id":"fixture-root",
            "response_id":id, "usage":usage(input, output),
            "turn_token_usage":usage(input * 10, output * 10),
            "thread_token_usage":usage(input * 20, output * 20)}})
}

/// 构造兼容 token_count；它可延迟到工具完成后才发出。
fn legacy(input: u64, output: u64) -> Value {
    json!({"timestamp":"2026-10-11T00:10:00Z", "type":"event_msg",
        "payload":{"type":"token_count", "info":{
            "last_token_usage":usage(input, output),
            "total_token_usage":usage(input, output)}}})
}

/// 在一个上下文内解析合成记录，并返回调用与质量事实。
fn parse(values: &[Value], context: &mut JsonlParseContext) -> (Vec<UsageCall>, JsonlParseReport) {
    let input = values.iter().map(|v| format!("{v}\n")).collect::<String>();
    let provenance = SourceProvenance {
        source_id: "fixture-source".into(),
        root_id: "fixture-root".into(),
        relative_label: "sessions/fixture.jsonl".into(),
        archived: false,
    };
    let mut calls = Vec::new();
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        context,
        &provenance,
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("synthetic response stream parses");
    (calls, report)
}

/// 正常响应与压缩响应各计一次，延迟旧影子与不同累计值均不重复计费。
#[test]
fn modern_records_include_compaction_without_legacy_shadow_double_counting() {
    let (calls, report) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            response("fixture-owner", "normal-response", 100, 20),
            legacy(100, 20),
            response("fixture-owner", "compact-response", 30, 5),
            json!({"type":"compacted", "payload":{"compaction_response_id":"compact-response"}}),
            legacy(100, 20),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls.iter().map(|c| c.usage.total_tokens).sum::<u64>(), 155);
    assert!(
        calls
            .iter()
            .all(|c| c.confidence == loki_metris_core::Confidence::Exact)
    );
    assert!(
        calls
            .iter()
            .all(|c| c.thread_key == stable_id("thread", "fixture-owner"))
    );
    assert!(!format!("{calls:?}").contains("normal-response"));
}

/// 首条新记录前保留真实旧调用，现代响应总量不能覆盖或重新累计旧前缀。
#[test]
fn modern_transition_preserves_legacy_prefix() {
    let (calls, report) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            legacy(10, 2),
            response("fixture-owner", "modern-response", 20, 4),
            legacy(30, 6),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls.iter().map(|c| c.usage.total_tokens).sum::<u64>(), 36);
}

/// 新身份由响应 ID 决定，不能把不同用量拼入 ID 隐藏冲突。
#[test]
fn response_identity_is_independent_of_usage_and_sequence() {
    let (calls, _) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            response("fixture-owner", "same-response", 10, 2),
            response("fixture-owner", "different-response", 10, 2),
            response("fixture-owner", "same-response", 20, 4),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].logical_call_id, calls[2].logical_call_id);
    assert_ne!(calls[0].logical_call_id, calls[1].logical_call_id);
}

/// 纯额度更新不缺用量；没有额度证据的缺失事实仍须警告。
#[test]
fn quota_only_update_is_ignored_but_missing_usage_is_not() {
    let (_, report) = parse(
        &[
            json!({"type":"event_msg","payload":{"type":"token_count","info":null,
            "rate_limits":{"primary":null,"secondary":null}}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":"unknown"}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"message"}}}),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(report.warnings.missing_usage, 2);
    assert_eq!(report.warnings.malformed_lines, 1);
    assert_eq!(report.warnings.unknown_events, 0);
}

/// 缺少现代协议必填字段与违反 core 数值规则时均不产生账单。
#[test]
fn invalid_response_records_do_not_change_format_authority() {
    let mut missing = response("fixture-owner", "bad-response", 10, 2);
    missing["payload"]["response_id"] = json!("");
    let mut invalid = response("fixture-owner", "bad-usage", 10, 2);
    invalid["payload"]["usage"]["cached_input_tokens"] = json!(11);
    let mut no_time = response("fixture-owner", "bad-time", 10, 2);
    no_time["timestamp"] = Value::Null;
    let foreign = response("foreign-owner", "foreign-response", 10, 2);
    let mut incomplete = response("fixture-owner", "bad-components", 10, 2);
    incomplete["payload"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("input_tokens");
    let (calls, report) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            missing,
            invalid,
            no_time,
            foreign,
            incomplete,
            legacy(10, 2),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(report.warnings.total(), 5);
    assert_eq!(report.warnings.invalid_timestamps, 1);
}

/// 父线程复制前缀不能切换 child 格式，当前线程记录只认 meta.id。
#[test]
fn copied_modern_prefix_does_not_suppress_child_legacy_usage() {
    let mut owner = meta("child-owner", "fixture-root");
    owner["payload"]["forked_from_id"] = json!("parent-owner");
    let (calls, report) = parse(
        &[
            owner,
            response("parent-owner", "parent-response", 500, 50),
            json!({"type":"event_msg","payload":{"type":"task_started",
            "started_at":"2026-10-11T00:00:00Z"}}),
            legacy(10, 2),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 12);
    assert_eq!(calls[0].thread_key, stable_id("thread", "child-owner"));
}

/// fork 的实际开始可晚于创建；有效响应的本线程身份独立证明自身用量。
#[test]
fn modern_owner_identity_opens_fork_without_matching_start_time() {
    let mut owner = meta("child-owner", "child-owner");
    owner["payload"]["forked_from_id"] = json!("parent-owner");
    let mut context = JsonlParseContext::new("fixture");
    let (calls, report) = parse(
        &[
            owner,
            response("parent-owner", "copied-response", 500, 50),
            json!({"type":"event_msg","payload":{"type":"task_started",
                "started_at":"2026-10-11T00:00:14Z"}}),
            response("child-owner", "own-response", 10, 2),
            legacy(999, 99),
        ],
        &mut context,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 12);
    assert_eq!(calls[0].thread_key, stable_id("thread", "child-owner"));
    assert_eq!(context.call_sequence, 1);
    assert!(!context.awaits_owned_start());
    assert_eq!(report.warnings.total(), 0);
}

/// 缺失或无效本线程事实不能开启 fork；父前缀的身份也不能被借用。
#[test]
fn invalid_response_does_not_prove_fork_ownership() {
    for bad_field in ["thread_id", "response_id", "usage", "timestamp"] {
        let mut owner = meta("child-owner", "child-owner");
        owner["payload"]["forked_from_id"] = json!("parent-owner");
        let mut bad = response("child-owner", "invalid-response", 10, 2);
        if bad_field == "timestamp" {
            bad[bad_field] = json!("invalid");
        } else {
            bad["payload"][bad_field] = Value::Null;
        }
        let mut context = JsonlParseContext::new("fixture");
        let (calls, report) = parse(&[owner, bad], &mut context);
        assert!(calls.is_empty());
        assert!(context.awaits_owned_start());
        assert!(!context.usage_authority.ignores_legacy_shadow());
        assert!(report.warnings.total() > 0);
    }
    for id in ["".to_owned(), " ".to_owned(), "x".repeat(513)] {
        let mut owner = meta("child-owner", "child-owner");
        owner["payload"]["forked_from_id"] = json!("parent-owner");
        let mut bad = response("child-owner", "invalid-response", 10, 2);
        bad["payload"]["thread_id"] = json!(id);
        let mut context = JsonlParseContext::new("fixture");
        let (calls, report) = parse(&[owner, bad], &mut context);
        assert!(calls.is_empty());
        assert!(context.awaits_owned_start());
        assert_eq!(report.warnings.missing_usage, 1);
    }
}

/// 尚未执行的空 fork 和可明确归属父线程的现代前缀都是已知零贡献。
#[test]
fn empty_fork_and_explicit_foreign_prefix_do_not_degrade_quality() {
    let mut owner = meta("child-owner", "child-owner");
    owner["payload"]["forked_from_id"] = json!("parent-owner");
    let mut context = JsonlParseContext::new("fixture");
    let (calls, report) = parse(
        &[
            owner,
            json!({"type":"event_msg","payload":{"type":"thread_settings_applied"}}),
            response("parent-owner", "copied-response", 500, 50),
        ],
        &mut context,
    );
    assert!(calls.is_empty());
    assert!(context.awaits_owned_start());
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(persisted_source_quality(Some(&context.adapter_state().unwrap())),
        loki_metris_core::PersistedSourceQuality::Known { warning_count: 0 });
    let (_, report) = parse(&[legacy(10, 2)], &mut context);
    assert_eq!(report.warnings.unresolved_owned_boundaries, 1);
}

/// 后来的明确响应不能反向证明早先无身份旧用量属于父线程；质量继续保留歧义。
#[test]
fn modern_fork_response_preserves_prior_legacy_ambiguity() {
    let mut owner = meta("child-owner", "child-owner");
    owner["payload"]["forked_from_id"] = json!("parent-owner");
    let mut context = JsonlParseContext::new("fixture");
    let (calls, report) = parse(&[
        owner, legacy(50, 5), response("child-owner", "own-response", 10, 2),
        legacy(999, 99),
    ], &mut context);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 12);
    assert_eq!(report.warnings.unresolved_owned_boundaries, 1);
    assert!(!context.awaits_owned_start());
    assert_eq!(persisted_source_quality(Some(&context.adapter_state().unwrap())),
        loki_metris_core::PersistedSourceQuality::Known { warning_count: 1 });
}

/// 旧状态或缺失歧义字段必须重建，不能把未知前缀默认为可信零贡献。
#[test]
fn missing_fork_ambiguity_state_requires_rebuild() {
    let context = JsonlParseContext::new("fixture");
    let mut persisted: Value = serde_json::from_str(&context.adapter_state().unwrap()).unwrap();
    persisted["schemaVersion"] = json!(3);
    assert!(PersistedJsonlParseState::decode(Some(&persisted.to_string())).is_none());
    persisted["schemaVersion"] = json!(4);
    persisted.as_object_mut().unwrap().remove("unresolvedLegacyPrefix");
    assert!(PersistedJsonlParseState::decode(Some(&persisted.to_string())).is_none());
}

/// 现代格式阶段随检查点恢复，下一批旧影子不会成为新调用。
#[test]
fn response_authority_survives_checkpoint_round_trip() {
    let mut context = JsonlParseContext::new("fixture");
    let (calls, _) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            response("fixture-owner", "first-response", 10, 2),
        ],
        &mut context,
    );
    assert_eq!(calls.len(), 1);
    let checkpoint = SourceParseCheckpoint {
        thread_key: context.thread_key.clone(),
        project_key: context.project_key.clone(),
        project_label: context.project_label.clone(),
        thread_label: context.thread_label.clone(),
        model: context.model.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        call_sequence: context.call_sequence,
        adapter_state: Some(context.adapter_state().unwrap()),
    };
    let mut resumed =
        JsonlParseContext::from_checkpoint(checkpoint).expect("current checkpoint restores");
    let (calls, report) = parse(
        &[
            legacy(10, 2),
            response("fixture-owner", "next-response", 20, 4),
        ],
        &mut resumed,
    );
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].usage.total_tokens, 24);
}

/// 可解码旧影子须在 core 值域校验与上下文更新前忽略；损坏 JSON 仍须警告。
#[test]
fn modern_shadow_cannot_change_context_or_revalidate_old_usage() {
    let mut shadow = legacy(10, 2);
    shadow["payload"]["model"] = json!("wrong-shadow-model");
    shadow["payload"]["info"]["last_token_usage"]["cached_input_tokens"] = json!(11);
    let mut context = JsonlParseContext::new("fixture");
    let (calls, report) = parse(
        &[
            meta("fixture-owner", "fixture-root"),
            json!({"type":"turn_context","payload":{"model":"gpt-5"}}),
            response("fixture-owner", "first", 10, 2),
            shadow,
            json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
            response("fixture-owner", "next", 20, 4),
        ],
        &mut context,
    );
    assert_eq!(report.warnings.total(), 0);
    assert_eq!(calls.len(), 2);
    assert_eq!(context.call_sequence, 2);
    assert!(
        calls
            .iter()
            .all(|call| call.model.as_deref() == Some("gpt-5"))
    );
    let (_, report) = parse(
        &[json!({"type":"event_msg","payload":{"type":"token_count",
        "info":{"last_token_usage":{"input_tokens":-1}}}})],
        &mut context,
    );
    assert_eq!(
        report.warnings.malformed_lines, 1,
        "unparseable envelopes remain visible quality loss"
    );
}

/// 当前 meta.id 无效时不能回退为另一个根 session_id，再误计根线程响应。
#[test]
fn invalid_present_owner_id_does_not_fall_back_to_root_session() {
    let (calls, report) = parse(
        &[
            meta("", "fixture-root"),
            response("fixture-root", "root-response", 10, 2),
        ],
        &mut JsonlParseContext::new("fixture"),
    );
    assert!(calls.is_empty());
    assert_eq!(report.warnings.malformed_lines, 1);
    assert_eq!(report.warnings.invalid_usage, 1);
}
