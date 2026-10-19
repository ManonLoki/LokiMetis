//! Claude 子 Agent 失败经原生信封、状态机和通知分类后的父会话隔离回归。

use serde_json::json;

use super::{AiTool, HookBehavior, HookNotification, HookStateMachine};
use super::tests_protocol_audit::deliver;

/// 子 Agent 的工具失败和正常收尾不宣称父任务失败或完成，父任务仍可正常结束。
#[test]
fn claude_child_tool_failures_do_not_notify_parent_error() {
    for event in ["PostToolUseFailure", "PermissionDenied"] {
        let mut machine = HookStateMachine::default();
        deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
        deliver(
            &mut machine,
            AiTool::ClaudeCode,
            "SubagentStart",
            json!({"agent_id":"reviewer"}),
        );
        let call = json!({"agent_id":"reviewer", "tool_name":"Bash", "tool_use_id":"check"});
        deliver(&mut machine, AiTool::ClaudeCode, "PreToolUse", call.clone());
        assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, event, call), None, "{event}");
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
        assert_eq!(
            deliver(&mut machine, AiTool::ClaudeCode, "SubagentStop", json!({"agent_id":"reviewer"})),
            None,
        );
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
        assert_eq!(
            deliver(&mut machine, AiTool::ClaudeCode, "Stop", json!({})),
            Some(HookNotification::Ended),
        );
        assert_eq!(
            deliver(&mut machine, AiTool::ClaudeCode, event, json!({"agent_id":"reviewer"})),
            None,
        );
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Idle));
    }
}

/// 子 Agent 的 API 失败不能终止父轮次；真正主 Agent 失败仍通知且封闭该轮。
#[test]
fn claude_child_stop_failure_does_not_retire_parent_turn() {
    let mut machine = HookStateMachine::default();
    deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    assert_eq!(
        deliver(&mut machine, AiTool::ClaudeCode, "StopFailure", json!({"agent_id":"reviewer"})),
        None,
    );
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
    deliver(&mut machine, AiTool::ClaudeCode, "PreToolUse", json!({"tool_name":"Read"}));
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
    assert_eq!(
        deliver(&mut machine, AiTool::ClaudeCode, "StopFailure", json!({})),
        Some(HookNotification::Error),
    );
    deliver(&mut machine, AiTool::ClaudeCode, "PreToolUse", json!({}));
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Error));
}

/// 子 Agent 失败只解除自己的交互；父询问及其它并行询问保留，且不补发错误通知。
#[test]
fn claude_child_failure_preserves_other_questions_and_resolves_its_own() {
    let mut machine = HookStateMachine::default();
    deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    let parent = json!({"tool_name":"AskUserQuestion", "tool_use_id":"question"});
    let child = json!({"agent_id":"reviewer", "tool_name":"AskUserQuestion", "tool_use_id":"question"});
    assert_eq!(
        deliver(&mut machine, AiTool::ClaudeCode, "PreToolUse", parent.clone()),
        Some(HookNotification::Asking),
    );
    deliver(&mut machine, AiTool::ClaudeCode, "PreToolUse", child.clone());
    assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, "PostToolUseFailure", child), None);
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Asking));
    assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, "PostToolUse", parent), None);
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
}

/// 主会话可配置 agent_type；缺少子 Agent 身份时仍保留原有工具失败通知。
#[test]
fn claude_main_agent_failures_remain_errors_with_custom_agent_type() {
    for event in ["PostToolUseFailure", "PermissionDenied", "StopFailure"] {
        let mut machine = HookStateMachine::default();
        deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
        assert_eq!(
            deliver(&mut machine, AiTool::ClaudeCode, event, json!({"agent_type":"reviewer"})),
            Some(HookNotification::Error),
            "{event}",
        );
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Error));
    }
}

/// 子 Agent 迟到失败既不能创建幽灵会话，也不能复活已结束的父轮次或覆盖父错误。
#[test]
fn claude_child_failures_preserve_absent_stopped_and_failed_parents() {
    for event in ["PostToolUseFailure", "PermissionDenied", "StopFailure"] {
        let mut machine = HookStateMachine::default();
        let child = json!({"agent_id":"reviewer"});
        assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, event, child.clone()), None);
        assert_eq!(machine.notification_behavior(), None);
        deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
        deliver(&mut machine, AiTool::ClaudeCode, "PostToolUseFailure", json!({}));
        assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, event, child.clone()), None);
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Error));
        deliver(&mut machine, AiTool::ClaudeCode, "Stop", json!({}));
        assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, event, child.clone()), None);
        assert_eq!(machine.notification_behavior(), Some(HookBehavior::Idle));
        deliver(&mut machine, AiTool::ClaudeCode, "SessionEnd", json!({}));
        assert_eq!(deliver(&mut machine, AiTool::ClaudeCode, event, child), None);
        assert_eq!(machine.notification_behavior(), None);
    }
}

/// 子 Agent API 失败只释放自身全部等待；其它子 Agent 的问题仍需答复。
#[test]
fn claude_child_stop_failure_releases_only_its_own_questions() {
    let mut machine = HookStateMachine::default();
    deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({}));
    for agent in ["reviewer", "other-reviewer"] {
        deliver(
            &mut machine,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"agent_id":agent, "tool_name":"AskUserQuestion", "tool_use_id":"question"}),
        );
    }
    assert_eq!(
        deliver(&mut machine, AiTool::ClaudeCode, "StopFailure", json!({"agent_id":"reviewer"})),
        None,
    );
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Asking));
    deliver(
        &mut machine,
        AiTool::ClaudeCode,
        "PostToolUse",
        json!({"agent_id":"other-reviewer", "tool_use_id":"question"}),
    );
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));

    deliver(
        &mut machine,
        AiTool::ClaudeCode,
        "PreToolUse",
        json!({"agent_id":"reviewer", "tool_name":"AskUserQuestion", "tool_use_id":"last-question"}),
    );
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Asking));
    assert_eq!(
        deliver(&mut machine, AiTool::ClaudeCode, "StopFailure", json!({"agent_id":"reviewer"})),
        None,
    );
    assert_eq!(machine.notification_behavior(), Some(HookBehavior::Running));
}

/// 旧轮次与乱序未知轮次的子失败不能解除当前轮次同名交互的等待。
#[test]
fn claude_stale_child_failures_do_not_resolve_current_turn_questions() {
    for event in ["PostToolUseFailure", "PermissionDenied", "StopFailure"] {
        let mut machine = HookStateMachine::default();
        deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({"turn_id":"old"}));
        deliver(&mut machine, AiTool::ClaudeCode, "UserPromptSubmit", json!({"turn_id":"current"}));
        deliver(
            &mut machine,
            AiTool::ClaudeCode,
            "PreToolUse",
            json!({"turn_id":"current", "agent_id":"reviewer", "tool_use_id":"q", "tool_name":"AskUserQuestion"}),
        );
        for turn in ["old", "unknown"] {
            assert_eq!(
                deliver(
                    &mut machine,
                    AiTool::ClaudeCode,
                    event,
                    json!({"turn_id":turn, "agent_id":"reviewer", "tool_use_id":"q"}),
                ),
                None,
            );
            assert_eq!(machine.notification_behavior(), Some(HookBehavior::Asking));
        }
    }
}
