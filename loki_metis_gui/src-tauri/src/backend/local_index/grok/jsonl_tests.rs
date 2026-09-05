//! Grok `updates.jsonl` 解析器的夹具驱动单元测试。

use loki_metis_core::{SourceProvenance, TokenUsage};

use super::super::CancellationToken;
use super::jsonl::{
    GrokJsonlParseContext, PRODUCTION_GROK_SESSION_ENVELOPE_JSONL,
    PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL, SYNTHETIC_GROK_UPDATES_JSONL, parse_grok_jsonl_stream,
    sum_completed_usage_from_fixture,
};

/// 构造 Grok JSONL 测试使用的稳定来源证据。
fn provenance() -> SourceProvenance {
    SourceProvenance {
        source_id: "source-grok".to_owned(),
        root_id: "grok-root-test".to_owned(),
        relative_label: "sessions/cwd/session/updates.jsonl".to_owned(),
        archived: false,
    }
}

/// 进行中轮次与缺用量行必须省略；完成轮次总量等于夹具自身字段求和。
#[test]
fn omits_in_progress_and_sums_completed_usage_from_fixture() {
    let mut context = GrokJsonlParseContext::new("session-a", Some("%2Ftmp%2Fapp"));
    let mut calls = Vec::new();
    let report = parse_grok_jsonl_stream(
        SYNTHETIC_GROK_UPDATES_JSONL.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("fixture parses");
    let expected = sum_completed_usage_from_fixture(SYNTHETIC_GROK_UPDATES_JSONL);
    let actual = calls.iter().fold(TokenUsage::zero(), |acc, call| {
        acc.checked_add(&call.usage).expect("sum fits")
    });
    assert_eq!(report.calls_emitted, 3);
    assert_eq!(calls.len(), 3);
    assert_eq!(actual, expected);
    assert!(
        calls
            .iter()
            .all(|call| call.model.as_deref() != Some("grok-in-progress"))
    );
    assert_eq!(context.project_label.as_deref(), Some("app"));
    assert_eq!(context.call_sequence, 3);
}

/// 生产 `params.update` 信封必须解析出已完成轮次，总量等于该行 usage 字段。
#[test]
fn parses_production_params_update_envelope_completed_usage() {
    let mut context = GrokJsonlParseContext::new("session-prod", Some("%2Ftmp%2Fapp"));
    let mut calls = Vec::new();
    let report = parse_grok_jsonl_stream(
        PRODUCTION_GROK_SESSION_ENVELOPE_JSONL.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("production envelope parses");
    let expected = sum_completed_usage_from_fixture(PRODUCTION_GROK_SESSION_ENVELOPE_JSONL);
    let actual = calls.iter().fold(TokenUsage::zero(), |acc, call| {
        acc.checked_add(&call.usage).expect("sum fits")
    });
    assert_eq!(report.calls_emitted, 1);
    assert_eq!(calls.len(), 1);
    assert_eq!(actual, expected);
    assert_eq!(expected.total_tokens, 100);
    assert_eq!(calls[0].model, None);
    assert_eq!(calls[0].reasoning_effort, None);
}

/// `usage.modelUsage` 的模型键必须原样进入 shipped `UsageCall.model`，且不得把聚合 usage 再计一次。
#[test]
fn persists_usage_model_usage_slug_without_double_counting() {
    let mut context = GrokJsonlParseContext::new("session-prod", Some("%2Ftmp%2Fapp"));
    let mut calls = Vec::new();
    parse_grok_jsonl_stream(
        PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("usage.modelUsage envelope parses");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].model.as_deref(), Some("grok-4.5-build"));
    assert_eq!(calls[0].reasoning_effort, None);
    assert_eq!(calls[0].usage.total_tokens, 100);
    let expected = sum_completed_usage_from_fixture(PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL);
    assert_eq!(calls[0].usage, expected);
}

/// 路径型模型与凭据对象邮箱不得进入调用字段。
#[test]
fn rejects_path_like_model_and_does_not_copy_credential_email() {
    let input = concat!(
        r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z","model":"/Users/alice/secret","auth":{"email":"alice@example.com"},"usage":{"inputTokens":12,"outputTokens":8,"totalTokens":20}}"#,
        "\n",
    );
    let mut context = GrokJsonlParseContext::new("session-a", None);
    let mut calls = Vec::new();
    parse_grok_jsonl_stream(
        input.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("unsafe model still yields a usage call");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].model, None);
    assert_eq!(calls[0].reasoning_effort, None);
    let debug = format!("{calls:?}");
    assert!(!debug.contains("alice@example.com"));
    assert!(!debug.contains("/Users/alice"));
}

/// 续读必须沿用上下文序号，避免无 turnId 的新完成轮次复用首条 logical_call_id。
#[test]
fn append_without_turn_id_keeps_distinct_logical_call_ids() {
    let mut context = GrokJsonlParseContext::new("session-a", Some("%2Ftmp%2Fapp"));
    let mut first_ids = Vec::new();
    parse_grok_jsonl_stream(
        SYNTHETIC_GROK_UPDATES_JSONL.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            first_ids.push(call.logical_call_id);
            Ok(())
        },
    )
    .expect("fixture parses");
    let appended = concat!(
        r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z","model":"grok-4.5-build","usage":{"inputTokens":12,"outputTokens":8,"totalTokens":20}}"#,
        "\n",
    );
    let mut appended_ids = Vec::new();
    parse_grok_jsonl_stream(
        appended.as_bytes(),
        1024 * 1024,
        &mut context,
        &provenance(),
        &CancellationToken::new(),
        |call| {
            appended_ids.push(call.logical_call_id);
            Ok(())
        },
    )
    .expect("append parses");
    assert_eq!(appended_ids.len(), 1);
    assert!(
        !first_ids.contains(&appended_ids[0]),
        "appended turn must not reuse a prior logical_call_id"
    );
    assert_eq!(context.call_sequence, 4);
}
