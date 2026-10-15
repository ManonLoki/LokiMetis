//! 原生Hooks最终结果、次要协议与交互恢复回归。

use super::tests_protocol_audit::deliver;
use super::{
    AiTool, HookBehavior, HookNotification, HookStateMachine, PreparedNativeHook,
    prepare_native_hook,
};
use serde_json::json;

/// WorkBuddy Stop可能继续；FinalStop的结果才决定通知，迟到事件不可复活终态。
#[test]
fn workbuddy_final_stop_is_authoritative_and_idless_late_events_are_ignored() {
    for (reason, expected, behavior) in [
        (
            "completed",
            Some(HookNotification::Ended),
            HookBehavior::Idle,
        ),
        ("failed", Some(HookNotification::Error), HookBehavior::Error),
        ("cancelled", None, HookBehavior::Idle),
        ("interrupted", None, HookBehavior::Idle),
    ] {
        let mut m = HookStateMachine::default();
        deliver(&mut m, AiTool::WorkBuddy, "UserPromptSubmit", json!({}));
        assert_eq!(deliver(&mut m, AiTool::WorkBuddy, "Stop", json!({})), None);
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
        assert_eq!(
            deliver(
                &mut m,
                AiTool::WorkBuddy,
                "FinalStop",
                json!({"final_stop_reason":reason})
            ),
            expected
        );
        assert_eq!(m.notification_behavior(), Some(behavior));
        deliver(&mut m, AiTool::WorkBuddy, "PreToolUse", json!({}));
        assert_eq!(m.notification_behavior(), Some(behavior));
        deliver(
            &mut m,
            AiTool::WorkBuddy,
            "SubagentStart",
            json!({"agent_id":"late-child"}),
        );
        assert_eq!(m.notification_behavior(), Some(behavior));
        deliver(&mut m, AiTool::WorkBuddy, "UserPromptSubmit", json!({}));
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    }
}

/// WorkBuddy 原生三类阻塞交互都能提示决策，并在对应调用结束后解除。
#[test]
fn workbuddy_question_tools_are_observed_without_relaying_the_question() {
    for name in [
        "AskUserQuestion",
        "ExitPlanMode",
        "AskUserForStructuredInput",
    ] {
        let mut m = HookStateMachine::default();
        deliver(&mut m, AiTool::WorkBuddy, "UserPromptSubmit", json!({}));
        assert_eq!(
            deliver(
                &mut m,
                AiTool::WorkBuddy,
                "PreToolUse",
                json!({"tool_name":name,"tool_use_id":"q","tool_input":{"question":"private"}})
            ),
            Some(HookNotification::Asking)
        );
        deliver(
            &mut m,
            AiTool::WorkBuddy,
            "PostToolUse",
            json!({"tool_use_id":"q"}),
        );
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    }
}

/// Copilot permissionRequest先于自动批准，只有实际UI通知才可证明需要人类决策。
#[test]
fn copilot_permission_evaluation_does_not_imply_human_wait() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::GitHubCopilot,
        "userPromptSubmitted",
        json!({}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::GitHubCopilot,
            "permissionRequest",
            json!({})
        ),
        None
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::GitHubCopilot,
            "notification",
            json!({"notification_type":"permission_prompt"})
        ),
        Some(HookNotification::Asking)
    );
    deliver(
        &mut m,
        AiTool::GitHubCopilot,
        "subagentStop",
        json!({"agent_id":"child"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
}

/// Gemini结构化工具错误需保留失败语义，错误内容本身不能越过relay边界。
#[test]
fn gemini_structured_error_is_recoverable_and_private() {
    let fields = json!({"tool_response":{"error":{"message":"private"}}});
    let PreparedNativeHook::Deliver(payload) = prepare_native_hook(
        AiTool::GeminiCli,
        &serde_json::to_vec(&fields).unwrap(),
        "AfterTool",
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(payload.status.as_deref(), Some("tool_error"));
    assert!(!serde_json::to_string(&payload).unwrap().contains("private"));
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::GeminiCli, "BeforeAgent", json!({}));
    assert_eq!(
        deliver(&mut m, AiTool::GeminiCli, "AfterTool", fields),
        Some(HookNotification::Error)
    );
    deliver(&mut m, AiTool::GeminiCli, "BeforeTool", json!({}));
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// Qwen工具错误和权限拒绝不退休父轮次，真实权限提示仍应进入询问。
#[test]
fn qwen_permission_prompt_and_recoverable_errors_preserve_the_turn() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::QwenCode,
        "UserPromptSubmit",
        json!({"turn_id":"t"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::QwenCode,
            "Notification",
            json!({"turn_id":"t","notification_type":"permission_prompt"})
        ),
        Some(HookNotification::Asking)
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::QwenCode,
            "PermissionDenied",
            json!({"turn_id":"t"})
        ),
        Some(HookNotification::Error)
    );
    deliver(
        &mut m,
        AiTool::QwenCode,
        "PreToolUse",
        json!({"turn_id":"t"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// Hermes 自动smart审批不属于人类等待，最终轮次按插件归约结果分类。
#[test]
fn hermes_smart_approval_and_end_outcomes_are_distinct() {
    for (status, expected) in [
        ("completed", Some(HookNotification::Ended)),
        ("failed", Some(HookNotification::Error)),
        ("cancelled", None),
    ] {
        let mut m = HookStateMachine::default();
        deliver(&mut m, AiTool::Hermes, "pre_llm_call", json!({}));
        assert_eq!(
            deliver(
                &mut m,
                AiTool::Hermes,
                "pre_approval_request",
                json!({"status":"smart"})
            ),
            None
        );
        assert_eq!(
            deliver(
                &mut m,
                AiTool::Hermes,
                "on_session_end",
                json!({"status":status})
            ),
            expected
        );
    }
}

/// 缺少调用ID的权限请求仅在唯一同名在途工具上关联，不被无关完成解除。
#[test]
fn claude_permission_request_uses_the_unique_active_tool() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PreToolUse",
        json!({"tool_name":"Bash", "tool_use_id":"a"}),
    );
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PreToolUse",
        json!({"tool_name":"Read", "tool_use_id":"b"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PermissionRequest",
            json!({"tool_name":"Bash"})
        ),
        Some(HookNotification::Asking)
    );
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_name":"Read", "tool_use_id":"b"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_name":"Bash", "tool_use_id":"a"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// 无法区分的并行同名调用采取保守等待，不能错误决定权限请求所属。
#[test]
fn ambiguous_permission_call_cannot_be_cleared_by_an_arbitrary_tool() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    for id in ["a", "b"] {
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"tool_name":"Bash", "tool_use_id":id}),
        );
    }
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PermissionRequest",
        json!({"tool_name":"Bash"}),
    );
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_name":"Bash", "tool_use_id":"b"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(&mut m, AiTool::ClaudeCode, "PreCompact", json!({}));
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
}

/// 非最终WorkBuddy Stop不会充当询问回复。
#[test]
fn workbuddy_nonfinal_stop_keeps_the_pending_question() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::WorkBuddy, "UserPromptSubmit", json!({}));
    deliver(
        &mut m,
        AiTool::WorkBuddy,
        "PreToolUse",
        json!({"tool_name":"AskUserQuestion", "tool_use_id":"q"}),
    );
    assert_eq!(deliver(&mut m, AiTool::WorkBuddy, "Stop", json!({})), None);
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
}

/// OpenCode回复按请求ID解除各自询问，工具callID不能替代requestID。
#[test]
fn opencode_replies_resolve_the_matching_request_only() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::OpenCode, "session.busy", json!({}));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::OpenCode,
            "question.asked",
            json!({"interaction_id":"q"})
        ),
        Some(HookNotification::Asking)
    );
    deliver(
        &mut m,
        AiTool::OpenCode,
        "permission.asked",
        json!({"interaction_id":"p"}),
    );
    deliver(
        &mut m,
        AiTool::OpenCode,
        "question.replied",
        json!({"interaction_id":"q"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(
        &mut m,
        AiTool::OpenCode,
        "permission.replied",
        json!({"interaction_id":"p"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// Grok权限通知缺少工具信息，可关联唯一在途调用，回复后不滞留询问。
#[test]
fn grok_notification_uses_the_unique_active_call() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::Grok,
        "UserPromptSubmit",
        json!({"promptId":"p"}),
    );
    deliver(
        &mut m,
        AiTool::Grok,
        "PreToolUse",
        json!({"promptId":"p", "toolName":"run_terminal_command", "toolCallId":"call"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Grok,
            "Notification",
            json!({"promptId":"p", "notificationType":"permission_prompt"})
        ),
        Some(HookNotification::Asking)
    );
    deliver(
        &mut m,
        AiTool::Grok,
        "PostToolUse",
        json!({"promptId":"p", "toolName":"run_terminal_command", "toolCallId":"call"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// Hermes最终输出包含失败解释也不能报成功，须等on_session_end的权威结果。
#[test]
fn hermes_final_response_is_not_proof_of_success() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::Hermes,
        "pre_llm_call",
        json!({"turn_id":"t"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Hermes,
            "api_request_error",
            json!({"turn_id":"t"})
        ),
        Some(HookNotification::Error)
    );
    deliver(
        &mut m,
        AiTool::Hermes,
        "pre_api_request",
        json!({"turn_id":"t"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Hermes,
            "post_llm_call",
            json!({"turn_id":"t"})
        ),
        None
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Hermes,
            "on_session_end",
            json!({"turn_id":"t", "status":"failed"})
        ),
        Some(HookNotification::Error)
    );
}

/// WorkBuddy停止Hook失败不会抢先退休工作，FinalStop仍可给出最终结果。
#[test]
fn workbuddy_stop_hook_error_can_be_followed_by_final_outcome() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::WorkBuddy,
        "UserPromptSubmit",
        json!({"turn_id":"t"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::WorkBuddy,
            "StopFailure",
            json!({"turn_id":"t"})
        ),
        Some(HookNotification::Error)
    );
    deliver(
        &mut m,
        AiTool::WorkBuddy,
        "PreToolUse",
        json!({"turn_id":"t"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::WorkBuddy,
            "FinalStop",
            json!({"turn_id":"t", "final_stop_reason":"completed"})
        ),
        Some(HookNotification::Ended)
    );
}

/// MCP表单回执常无调用ID；按服务器恢复，并保护另一服务器的并行等待。
#[test]
fn claude_form_elicitation_resolves_without_a_call_id() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PreToolUse",
        json!({"tool_name":"mcp__a__tool", "tool_use_id":"call"}),
    );
    for server in ["a", "b"] {
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "Elicitation",
            json!({"mcp_server_name":server}),
        );
    }
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "ElicitationResult",
        json!({"mcp_server_name":"b"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "ElicitationResult",
        json!({"mcp_server_name":"a"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
}

/// 同服务器的并行表单缺少调用ID时不能把任一回复当全部询问已解除。
#[test]
fn ambiguous_parallel_elicitations_remain_conservative() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    for _ in 0..2 {
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "Elicitation",
            json!({"mcp_server_name":"same"}),
        );
    }
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "ElicitationResult",
        json!({"mcp_server_name":"same"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(&mut m, AiTool::ClaudeCode, "Stop", json!({}));
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
}

/// 延迟UI澄清通知不能重复建立等待；通知早于原生请求时也须由真实归属替换。
#[test]
fn elicitation_ui_notification_does_not_duplicate_the_native_wait() {
    for notification_first in [false, true] {
        let mut m = HookStateMachine::default();
        deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"tool_name":"mcp__s__tool", "tool_use_id":"call"}),
        );
        if notification_first {
            deliver(
                &mut m,
                AiTool::ClaudeCode,
                "Notification",
                json!({"notification_type":"elicitation_dialog"}),
            );
        }
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "Elicitation",
            json!({"mcp_server_name":"s"}),
        );
        if !notification_first {
            deliver(
                &mut m,
                AiTool::ClaudeCode,
                "Notification",
                json!({"notification_type":"elicitation_dialog"}),
            );
        }
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "ElicitationResult",
            json!({"mcp_server_name":"s"}),
        );
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    }
}
