//! 官方协议样本经最小化、生命周期归约和通知分类的回归。

use std::{path::Path, time::Duration};

use serde_json::{Value, json};

use super::{
    AiTool, HookBehavior, HookEventDecision, HookEventScope, HookNotification, HookStateMachine,
    PreparedNativeHook, generate_hook_config, hook_notification, prepare_native_hook,
};

/// 使用真实原生字段经过完整 core 管线，避免仅测试映射表自身。
pub(super) fn deliver(
    machine: &mut HookStateMachine,
    tool: AiTool,
    event: &str,
    fields: Value,
) -> Option<HookNotification> {
    let mut raw = json!({"session_id": "s"});
    raw.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    let PreparedNativeHook::Deliver(payload) =
        prepare_native_hook(tool, &serde_json::to_vec(&raw).unwrap(), event).unwrap()
    else {
        panic!("本机原生事件应被接纳");
    };
    let previous = machine.notification_behavior();
    let decision = machine.apply_event_with_scope_at(
        tool,
        &payload.hook_event_name,
        payload.session_id.as_deref(),
        payload.turn_id.as_deref(),
        payload.status.as_deref(),
        HookEventScope {
            agent_id: payload.agent_id.as_deref(),
            interaction_id: payload.interaction_id.as_deref(),
            tool_name: payload.tool_name.as_deref(),
        },
        Duration::ZERO,
    );
    assert_ne!(decision, HookEventDecision::Unsupported, "{tool:?}/{event}");
    hook_notification(tool, event, payload.status.as_deref(), previous, decision)
}

/// 验证 Codex 中断释放等待，不推送成功，且旧轮次不能再次启动。
#[test]
fn codex_interrupt_is_silent_and_retires_the_turn() {
    let mut m = HookStateMachine::default();
    let id = json!({"turn_id": "t"});
    deliver(&mut m, AiTool::Codex, "UserPromptSubmit", id.clone());
    assert_eq!(
        deliver(&mut m, AiTool::Codex, "PermissionRequest", id.clone()),
        Some(HookNotification::Asking)
    );
    assert_eq!(
        deliver(&mut m, AiTool::Codex, "Interrupt", id.clone()),
        None
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
    assert_eq!(deliver(&mut m, AiTool::Codex, "PreToolUse", id), None);
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
    let preview = generate_hook_config(AiTool::Codex, Path::new("/opt/loki")).unwrap();
    let config: Value = serde_json::from_str(&preview.content).unwrap();
    assert_eq!(config["hooks"]["Interrupt"][0]["hooks"][0]["timeout"], 3);
    assert!(config["hooks"]["PostCompact"].is_array());
}

/// 普通工具执行前不是询问；中断工具和整个任务取消都不伪造失败或完成。
#[test]
fn cursor_execution_and_cancellation_are_not_user_decisions_or_success() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::Cursor,
        "beforeSubmitPrompt",
        json!({"generation_id":"g"}),
    );
    for event in ["beforeShellExecution", "beforeMCPExecution"] {
        assert_eq!(
            deliver(&mut m, AiTool::Cursor, event, json!({"generation_id":"g"})),
            None
        );
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    }
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Cursor,
            "postToolUseFailure",
            json!({"generation_id":"g", "is_interrupt":true})
        ),
        None
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Cursor,
            "stop",
            json!({"generation_id":"g", "status":"aborted"})
        ),
        None
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
}

/// Cursor 明确区分成功、失败与仅关闭窗口，消息结束也不能代表工作完成。
#[test]
fn cursor_final_outcomes_and_session_close_use_native_result() {
    for (event, fields, expected) in [
        (
            "stop",
            json!({"status":"completed"}),
            Some(HookNotification::Ended),
        ),
        (
            "stop",
            json!({"status":"error"}),
            Some(HookNotification::Error),
        ),
        (
            "sessionEnd",
            json!({"reason":"error"}),
            Some(HookNotification::Error),
        ),
        ("sessionEnd", json!({"reason":"window_close"}), None),
        ("sessionEnd", json!({"reason":"aborted"}), None),
        (
            "sessionEnd",
            json!({"reason":"window_close", "final_status":"completed"}),
            Some(HookNotification::Ended),
        ),
    ] {
        let mut m = HookStateMachine::default();
        deliver(&mut m, AiTool::Cursor, "beforeSubmitPrompt", json!({}));
        assert_eq!(
            deliver(&mut m, AiTool::Cursor, "afterAgentResponse", json!({})),
            None
        );
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
        assert_eq!(
            deliver(&mut m, AiTool::Cursor, event, fields.clone()),
            expected,
            "{event}/{fields}"
        );
    }
}

/// 工具调用 ID 仅用于等待归属，子 Agent 和无关并行工具不能清除主询问。
#[test]
fn claude_question_wait_survives_unrelated_and_child_completions() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"tool_name":"AskUserQuestion", "tool_use_id":"q"})
        ),
        Some(HookNotification::Asking)
    );
    for (event, fields) in [
        ("PostToolUse", json!({"tool_use_id":"other"})),
        ("PreCompact", json!({})),
        (
            "PostToolUse",
            json!({"agent_id":"child", "tool_use_id":"q"}),
        ),
        ("SubagentStop", json!({"agent_id":"child"})),
    ] {
        assert_eq!(deliver(&mut m, AiTool::ClaudeCode, event, fields), None);
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    }
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_use_id":"q"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    let PreparedNativeHook::Deliver(p) = prepare_native_hook(
        AiTool::ClaudeCode,
        br#"{"tool_use_id":"call-only"}"#,
        "PreToolUse",
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(p.turn_id, None);
    assert_eq!(p.interaction_id.as_deref(), Some("call-only"));
}

/// 并行的两个询问必须逐个解除，通知只在聚合状态首次进入询问时产生。
#[test]
fn parallel_questions_and_elicitation_notifications_resolve_independently() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    for (id, expected) in [("q1", Some(HookNotification::Asking)), ("q2", None)] {
        assert_eq!(
            deliver(
                &mut m,
                AiTool::ClaudeCode,
                "PreToolUse",
                json!({"tool_name":"ExitPlanMode", "tool_use_id":id})
            ),
            expected
        );
    }
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_use_id":"q1"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    deliver(
        &mut m,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"tool_use_id":"q2"}),
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    for kind in ["elicitation_dialog", "elicitation_url_dialog"] {
        assert_eq!(
            deliver(
                &mut m,
                AiTool::ClaudeCode,
                "Notification",
                json!({"notification_type":kind})
            ),
            Some(HookNotification::Asking)
        );
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "Notification",
            json!({"notification_type":"elicitation_response"}),
        );
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    }
}

/// 有界交互存储溢出时保守等待，不因丢失第257个关联而伪造已解除询问。
#[test]
fn interaction_overflow_remains_asking_until_terminal() {
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    for id in 0..257 {
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"tool_name":"AskUserQuestion", "tool_use_id":id.to_string()}),
        );
    }
    for id in 0..257 {
        deliver(
            &mut m,
            AiTool::ClaudeCode,
            "PostToolUse",
            json!({"tool_use_id":id.to_string()}),
        );
    }
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Asking));
    assert_eq!(
        deliver(&mut m, AiTool::ClaudeCode, "Stop", json!({})),
        Some(HookNotification::Ended)
    );
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
}

/// Grok 工具失败可恢复，API最终失败才退休轮次，取消保持静默。
#[test]
fn grok_errors_are_classified_by_scope_and_camel_case_notification() {
    let mut m = HookStateMachine::default();
    let id = json!({"promptId":"p"});
    deliver(&mut m, AiTool::Grok, "UserPromptSubmit", id.clone());
    assert_eq!(
        deliver(&mut m, AiTool::Grok, "PostToolUseFailure", id.clone()),
        Some(HookNotification::Error)
    );
    deliver(&mut m, AiTool::Grok, "PreToolUse", id.clone());
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Grok,
            "Notification",
            json!({"promptId":"p", "notificationType":"permission_prompt"})
        ),
        Some(HookNotification::Asking)
    );
    deliver(&mut m, AiTool::Grok, "PostToolUse", id.clone());
    assert_eq!(deliver(&mut m, AiTool::Grok, "Stop", id.clone()), None);
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::Grok, "UserPromptSubmit", id.clone());
    assert_eq!(
        deliver(&mut m, AiTool::Grok, "StopFailure", id.clone()),
        Some(HookNotification::Error)
    );
    deliver(&mut m, AiTool::Grok, "PreToolUse", id.clone());
    assert_eq!(m.notification_behavior(), Some(HookBehavior::Error));
    let mut m = HookStateMachine::default();
    deliver(&mut m, AiTool::Grok, "UserPromptSubmit", id.clone());
    assert_eq!(deliver(&mut m, AiTool::Grok, "StopCancelled", id), None);
}

/// Grok 第一方交互工具在执行前进入询问，而普通工具保持运行。
#[test]
fn grok_native_question_tool_enters_asking() {
    let mut m = HookStateMachine::default();
    deliver(
        &mut m,
        AiTool::Grok,
        "UserPromptSubmit",
        json!({"promptId":"p"}),
    );
    assert_eq!(
        deliver(
            &mut m,
            AiTool::Grok,
            "PreToolUse",
            json!({"promptId":"p", "toolName":"ask_user_question"})
        ),
        Some(HookNotification::Asking)
    );
}

/// Codex/Grok Stop仍可被阻止并同ID续跑；不永久封死轮次或宣称已确定成功。
#[test]
fn continuable_stop_allows_same_turn_activity_without_false_success() {
    for tool in [AiTool::Codex, AiTool::Grok] {
        let mut m = HookStateMachine::default();
        let id = json!({"turn_id":"t"});
        deliver(&mut m, tool, "UserPromptSubmit", id.clone());
        assert_eq!(deliver(&mut m, tool, "Stop", id.clone()), None);
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
        deliver(&mut m, tool, "PostToolUse", id.clone());
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
        deliver(&mut m, tool, "PreToolUse", id.clone());
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Running));
        deliver(&mut m, tool, "Stop", id.clone());
        let cancel = if tool == AiTool::Codex {
            "Interrupt"
        } else {
            "StopCancelled"
        };
        assert_eq!(deliver(&mut m, tool, cancel, id.clone()), None);
        deliver(&mut m, tool, "PreToolUse", id);
        assert_eq!(m.notification_behavior(), Some(HookBehavior::Idle));
    }
}
