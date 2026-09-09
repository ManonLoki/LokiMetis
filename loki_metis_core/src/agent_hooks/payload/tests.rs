//! 原生 Hook 信封解析和跨宿主抑制回归。

use super::{MinimalHookPayload, PreparedNativeHook, prepare_native_hook};
use crate::agent_hooks::AiTool;

/// 提取必须投递的测试信封。
fn delivered(native_json: &[u8], event: &str) -> MinimalHookPayload {
    match prepare_native_hook(AiTool::Codex, native_json, event).unwrap() {
        PreparedNativeHook::Deliver(payload) => payload,
        PreparedNativeHook::SuppressForeignHost => {
            panic!("extraction fixture must not look like a Cursor host envelope")
        }
    }
}

/// 构造 Cursor 官方 SessionStart 信封。
fn official_cursor_session_start() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "conversation_id": "conv-cursor-1",
        "generation_id": "gen-1",
        "hook_event_name": "sessionStart",
        "cursor_version": "2.0.0",
        "workspace_roots": ["/Users/dev/project"],
        "transcript_path": null
    }))
    .unwrap()
}

/// 构造 Claude Code 官方 SessionStart 信封。
fn official_claude_session_start() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "session_id": "abc123",
        "transcript_path": "/Users/dev/.claude/projects/repo/abc123.jsonl",
        "cwd": "/Users/dev/project",
        "permission_mode": "default",
        "hook_event_name": "SessionStart",
        "source": "startup"
    }))
    .unwrap()
}

#[test]
/// 验证原生正文只保留状态机所需的最小字段。
fn native_hook_payload_is_reduced_to_the_state_machine_envelope() {
    let prompt = "包含中文、\\Windows\\路径和 \"引号\" 的长提示".repeat(100);
    let native = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "conversation_id": "session-1",
        "generation_id": "turn-9",
        "status": 500,
        "prompt": prompt,
        "tool_input": { "command": "echo large" },
        "tool_output": "x".repeat(8_000),
        "transcript_path": "C:\\Users\\tester\\session.jsonl"
    });
    let native_bytes = serde_json::to_vec(&native).unwrap();
    let payload = delivered(&native_bytes, "PostToolUse");
    let minimized = serde_json::to_vec(&payload).unwrap();

    assert_eq!(payload.hook_event_name, "PostToolUse");
    assert_eq!(payload.session_id.as_deref(), Some("session-1"));
    assert_eq!(payload.turn_id.as_deref(), Some("turn-9"));
    assert_eq!(payload.status.as_deref(), Some("500"));
    assert!(native_bytes.len() > 10_000);
    assert!(minimized.len() < 150);
    assert!(!String::from_utf8(minimized).unwrap().contains("prompt"));
}

#[test]
/// 验证 WorkBuddy 驼峰式上下文字段被归一化到统一信封。
fn workbuddy_camel_case_context_is_normalized() {
    let native = serde_json::json!({
        "hook_event_name": "Stop",
        "sessionId": "workbuddy-session-1",
        "turnId": "workbuddy-turn-2",
        "prompt": "must not leave the relay boundary"
    });
    let payload = delivered(&serde_json::to_vec(&native).unwrap(), "Stop");
    assert_eq!(payload.session_id.as_deref(), Some("workbuddy-session-1"));
    assert_eq!(payload.turn_id.as_deref(), Some("workbuddy-turn-2"));
}

#[test]
/// 验证空白主字段会回退到裁剪后的 Cursor 别名字段。
fn blank_primary_context_fields_fall_back_to_trimmed_cursor_aliases() {
    let native = serde_json::json!({
        "hook_event_name": "stop",
        "session_id": " \t ",
        "conversation_id": " conversation-1 ",
        "turn_id": "\n",
        "generation_id": " generation-2 ",
    });
    let payload = delivered(&serde_json::to_vec(&native).unwrap(), "stop");
    assert_eq!(payload.session_id.as_deref(), Some("conversation-1"));
    assert_eq!(payload.turn_id.as_deref(), Some("generation-2"));
}

#[test]
/// 验证带 BOM 的 UTF-8、UTF-16LE 与 UTF-16BE 正文均可解析。
fn native_hook_payload_accepts_utf8_and_utf16_boms() {
    let json = r#"{"hook_event_name":"stop","conversation_id":"会话-1"}"#;
    let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
    utf8_bom.extend_from_slice(json.as_bytes());
    let utf16 = json.encode_utf16().collect::<Vec<_>>();
    let mut utf16_le = vec![0xFF, 0xFE];
    utf16_le.extend(utf16.iter().flat_map(|unit| unit.to_le_bytes()));
    let mut utf16_be = vec![0xFE, 0xFF];
    utf16_be.extend(utf16.iter().flat_map(|unit| unit.to_be_bytes()));

    for native in [utf8_bom, utf16_le, utf16_be] {
        let payload = delivered(&native, "stop");
        assert_eq!(payload.session_id.as_deref(), Some("会话-1"));
    }
}

#[test]
/// 验证损坏的 UTF-16 BOM 输入被稳定拒绝且不会 panic。
fn malformed_utf16_bom_input_is_rejected_without_panicking() {
    let error = prepare_native_hook(AiTool::Codex, &[0xFF, 0xFE, b'{'], "stop").unwrap_err();
    assert_eq!(error.code, "error.payload.invalidUtf16");
    let error = prepare_native_hook(AiTool::Codex, &[0xFF, 0xFE, 0x00, 0xD8], "stop").unwrap_err();
    assert_eq!(error.code, "error.payload.invalidUtf16");
}

#[test]
/// 验证 Grok 官方 stdin 被裁剪为最小 listener 信封。
fn official_grok_stdin_is_reduced_to_the_minimal_listener_envelope() {
    let native = serde_json::json!({
        "hookEventName": "pre_tool_use",
        "sessionId": "abc-123",
        "promptId": "turn-7",
        "cwd": "/Users/you/project",
        "workspaceRoot": "/Users/you/project",
        "permissionMode": "default",
        "toolName": "run_terminal_command",
        "toolInput": { "command": "npm test" },
        "prompt": "must not leave the relay boundary",
        "timestamp": "2026-04-14T12:00:00Z"
    });
    let payload = delivered(&serde_json::to_vec(&native).unwrap(), "PreToolUse");
    let minimized_text = String::from_utf8(serde_json::to_vec(&payload).unwrap()).unwrap();
    assert_eq!(payload.hook_event_name, "PreToolUse");
    assert_eq!(payload.session_id.as_deref(), Some("abc-123"));
    assert_eq!(payload.turn_id.as_deref(), Some("turn-7"));
    for private_field in ["toolInput", "cwd", "workspaceRoot", "prompt"] {
        assert!(!minimized_text.contains(private_field));
    }
}

#[test]
/// 验证 Grok 正文事件名与已配置事件不一致时被拒绝。
fn mismatched_grok_hook_event_name_is_rejected() {
    let native = serde_json::json!({
        "hookEventName": "session_start",
        "sessionId": "abc-123",
    });
    let error = prepare_native_hook(
        AiTool::Grok,
        &serde_json::to_vec(&native).unwrap(),
        "PreToolUse",
    )
    .unwrap_err();
    assert_eq!(error.code, "error.payload.eventMismatch");
}

#[test]
/// 验证无 BOM 的非 UTF-8 字节不会被猜测为其他编码。
fn native_hook_payload_does_not_guess_a_non_utf8_encoding_without_a_bom() {
    let error = prepare_native_hook(AiTool::Codex, &[0xFF, b'{', b'}'], "stop").unwrap_err();
    assert_eq!(error.code, "error.payload.invalidUtf8");
}

#[test]
/// 验证 Cursor 宿主发出的 Claude 会话事件会被跨宿主抑制。
fn cursor_hosted_claude_session_start_is_suppressed() {
    assert_eq!(
        prepare_native_hook(
            AiTool::ClaudeCode,
            &official_cursor_session_start(),
            "SessionStart"
        )
        .unwrap(),
        PreparedNativeHook::SuppressForeignHost
    );
}

#[test]
/// 验证原生 Cursor 会话开始事件仍会正常投递。
fn native_cursor_session_start_is_delivered() {
    let PreparedNativeHook::Deliver(payload) = prepare_native_hook(
        AiTool::Cursor,
        &official_cursor_session_start(),
        "sessionStart",
    )
    .unwrap() else {
        panic!("native Cursor stdin must be delivered");
    };
    assert_eq!(payload.session_id.as_deref(), Some("conv-cursor-1"));
}

#[test]
/// 验证官方 Claude Code 会话开始信封可正常投递。
fn official_claude_session_start_is_delivered() {
    let PreparedNativeHook::Deliver(payload) = prepare_native_hook(
        AiTool::ClaudeCode,
        &official_claude_session_start(),
        "SessionStart",
    )
    .unwrap() else {
        panic!("real Claude stdin must be delivered");
    };
    assert_eq!(payload.session_id.as_deref(), Some("abc123"));
}

#[test]
/// 验证仅有工作区根字段不会被误判为 Cursor 宿主。
fn workspace_roots_without_cursor_version_is_not_a_cursor_host() {
    let native = serde_json::to_vec(&serde_json::json!({
        "hook_event_name": "SessionStart",
        "conversation_id": "conv-2",
        "workspace_roots": ["/tmp/repo"]
    }))
    .unwrap();
    let PreparedNativeHook::Deliver(payload) =
        prepare_native_hook(AiTool::ClaudeCode, &native, "SessionStart").unwrap()
    else {
        panic!("workspace_roots alone must not suppress");
    };
    assert_eq!(payload.session_id.as_deref(), Some("conv-2"));
}

#[test]
/// 验证跨宿主抑制先于事件名不匹配校验执行。
fn cursor_hosted_claude_hook_is_suppressed_before_event_name_mismatch() {
    let native = serde_json::to_vec(&serde_json::json!({
        "hook_event_name": "beforeSubmitPrompt",
        "conversation_id": "conv-cursor-1",
        "cursor_version": "2.0.0"
    }))
    .unwrap();
    assert_eq!(
        prepare_native_hook(AiTool::ClaudeCode, &native, "UserPromptSubmit").unwrap(),
        PreparedNativeHook::SuppressForeignHost
    );
}

#[test]
/// 验证空白或 null 的 Cursor 版本不构成跨宿主标识。
fn blank_or_null_cursor_version_does_not_mark_a_cursor_host() {
    for cursor_version in [
        serde_json::Value::String(String::new()),
        serde_json::Value::String("  \t".to_owned()),
        serde_json::Value::Null,
    ] {
        let native = serde_json::to_vec(&serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": "abc123",
            "cursor_version": cursor_version
        }))
        .unwrap();
        let PreparedNativeHook::Deliver(payload) =
            prepare_native_hook(AiTool::ClaudeCode, &native, "SessionStart").unwrap()
        else {
            panic!("empty cursor_version must not suppress");
        };
        assert_eq!(payload.session_id.as_deref(), Some("abc123"));
    }
}
