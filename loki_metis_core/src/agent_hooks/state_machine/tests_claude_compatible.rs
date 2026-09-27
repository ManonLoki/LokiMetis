//! Claude 兼容客户端的事件语义、同名通知与多会话回归。

use std::time::Duration;

use super::{HookEventDecision, HookStateMachine, HookTransition};
use crate::agent_hooks::{AiTool, HookBehavior};

/// 以固定时间推进指定客户端的一条 Hook 事件。
fn apply_tool(
    machine: &mut HookStateMachine,
    tool: AiTool,
    event: &str,
    session_id: Option<&str>,
    turn_id: Option<&str>,
    second: u64,
) -> HookEventDecision {
    machine.apply_event_with_status_at(
        tool,
        event,
        session_id,
        turn_id,
        None,
        Duration::from_secs(second),
    )
}

/// WorkBuddy 与 CodeBuddy 的重复进度由多会话状态机去重。
#[test]
fn workbuddy_and_codebuddy_suppress_repeated_progress() {
    for tool in [AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            machine.apply_event_with_status_at(
                tool,
                "PreToolUse",
                Some("session-1"),
                Some("turn-1"),
                None,
                Duration::from_secs(1),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            machine.apply_event_with_status_at(
                tool,
                "PreToolUse",
                Some("session-1"),
                Some("turn-1"),
                None,
                Duration::from_secs(2),
            ),
            HookEventDecision::Ignore,
            "{tool:?}"
        );
    }
}

/// 两客户端的并发会话独立聚合，已知旧轮次的迟到进度不能复活。
#[test]
fn workbuddy_and_codebuddy_aggregate_sessions_and_reject_late_progress() {
    for tool in [AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply_tool(&mut machine, tool, "SessionStart", Some("s1"), None, 1),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                tool,
                "UserPromptSubmit",
                Some("s1"),
                Some("t1"),
                2
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "UserPromptSubmit", Some("s2"), None, 3),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "Stop", Some("s1"), Some("t1"), 4),
            HookEventDecision::Ignore,
            "另一个会话仍在运行：{tool:?}"
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "PreToolUse", Some("s1"), Some("t1"), 5),
            HookEventDecision::Ignore,
            "停止后的迟到进度：{tool:?}"
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "Stop", Some("s2"), None, 6),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "SessionEnd", Some("s1"), None, 7),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "SessionEnd", Some("s2"), None, 8),
            HookEventDecision::Forward(HookTransition::Release)
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "PreToolUse", Some("s1"), None, 9),
            HookEventDecision::Ignore,
            "会话墓碑拦截迟到事件：{tool:?}"
        );
    }
}

/// 无轮次 ID 时 Stop Hook 可能继续执行：迟到完成无效，新的活动信号可恢复运行。
#[test]
fn idless_stop_can_resume_after_new_activity() {
    for tool in [AiTool::ClaudeCode, AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let mut machine = HookStateMachine::default();
        let _ = apply_tool(&mut machine, tool, "UserPromptSubmit", Some("s1"), None, 1);
        assert_eq!(
            apply_tool(&mut machine, tool, "Stop", Some("s1"), None, 2),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "PostToolUse", Some("s1"), None, 3),
            HookEventDecision::Ignore,
            "迟到完成：{tool:?}"
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "PreToolUse", Some("s1"), None, 4),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running)),
            "Stop Hook 续跑：{tool:?}"
        );
    }
}

/// 工具失败可由同轮后续调用恢复，但 Stop 后迟到的失败完成不得改写空闲态。
#[test]
fn claude_compatible_tool_failure_is_recoverable_and_late_failure_is_ignored() {
    for tool in [AiTool::ClaudeCode, AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let mut machine = HookStateMachine::default();
        let _ = apply_tool(&mut machine, tool, "UserPromptSubmit", Some("s1"), None, 1);
        assert_eq!(
            apply_tool(
                &mut machine,
                tool,
                "PostToolUseFailure",
                Some("s1"),
                None,
                2
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error)),
            "{tool:?}"
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "PreToolUse", Some("s1"), None, 3),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running)),
            "{tool:?}"
        );
        assert_eq!(
            apply_tool(&mut machine, tool, "Stop", Some("s1"), None, 4),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle)),
            "{tool:?}"
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                tool,
                "PostToolUseFailure",
                Some("s1"),
                None,
                5
            ),
            HookEventDecision::Ignore,
            "{tool:?}"
        );
    }
}

/// Claude 的 API 失败终止当前轮次；无 ID 活动需等到下一次明确提示。
#[test]
fn claude_stop_failure_requires_a_new_prompt_without_a_turn_id() {
    let mut machine = HookStateMachine::default();
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("s1"),
            None,
            1
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "StopFailure",
            Some("s1"),
            None,
            2
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    for event in ["PostToolUse", "PostToolUseFailure", "PreToolUse", "Stop"] {
        assert_eq!(
            apply_tool(&mut machine, AiTool::ClaudeCode, event, Some("s1"), None, 3),
            HookEventDecision::Ignore,
            "{event} must not overwrite StopFailure"
        );
    }
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("s1"),
            None,
            4
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

/// 自动权限拒绝只标记本轮错误，Claude 随后可以继续处理工具调用。
#[test]
fn claude_permission_denied_is_recoverable_within_the_same_turn() {
    let mut machine = HookStateMachine::default();
    let _ = apply_tool(
        &mut machine,
        AiTool::ClaudeCode,
        "UserPromptSubmit",
        Some("s1"),
        None,
        1,
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "PermissionDenied",
            Some("s1"),
            None,
            2,
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "PreToolUse",
            Some("s1"),
            None,
            3,
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

/// MCP 澄清应答后回到运行态；只有已知的通知类型改变状态。
#[test]
fn claude_elicitation_result_and_notification_types_have_distinct_meanings() {
    let mut machine = HookStateMachine::default();
    let _ = apply_tool(
        &mut machine,
        AiTool::ClaudeCode,
        "UserPromptSubmit",
        Some("s1"),
        None,
        1,
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "Elicitation",
            Some("s1"),
            None,
            2
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "ElicitationResult",
            Some("s1"),
            None,
            3
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::ClaudeCode,
            "Notification",
            Some("s1"),
            None,
            Some("permission_prompt"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::ClaudeCode,
            "Notification",
            Some("s1"),
            None,
            Some("auth_success"),
        ),
        HookEventDecision::Unsupported
    );
    assert_eq!(
        machine.current_display_transition(),
        Some(HookTransition::Display(HookBehavior::Asking))
    );
}

/// 三种客户端的权限通知进入询问态，空闲提醒则结束当前轮次。
#[test]
fn claude_compatible_permission_notification_is_not_an_idle_notification() {
    for tool in [AiTool::ClaudeCode, AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let mut machine = HookStateMachine::default();
        let _ = apply_tool(&mut machine, tool, "UserPromptSubmit", Some("s1"), None, 1);
        assert_eq!(
            machine.apply_event_with_status(
                tool,
                "Notification",
                Some("s1"),
                None,
                Some("permission_prompt"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking)),
            "{tool:?}"
        );
        assert_eq!(
            machine.apply_event_with_status(
                tool,
                "Notification",
                Some("s1"),
                None,
                Some("permission_prompt"),
            ),
            HookEventDecision::Ignore,
            "{tool:?}"
        );
        assert_eq!(
            machine.apply_event_with_status(
                tool,
                "Notification",
                Some("s1"),
                None,
                Some("idle_prompt"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle)),
            "{tool:?}"
        );
    }
}

/// CodeBuddy 的 MCP 澄清结果让同一轮从询问态恢复运行。
#[test]
fn codebuddy_elicitation_result_resumes_the_current_turn() {
    let mut machine = HookStateMachine::default();
    let _ = apply_tool(
        &mut machine,
        AiTool::CodeBuddy,
        "UserPromptSubmit",
        Some("session-1"),
        Some("turn-1"),
        1,
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::CodeBuddy,
            "Elicitation",
            Some("session-1"),
            Some("turn-1"),
            2,
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        apply_tool(
            &mut machine,
            AiTool::CodeBuddy,
            "ElicitationResult",
            Some("session-1"),
            Some("turn-1"),
            3,
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}
