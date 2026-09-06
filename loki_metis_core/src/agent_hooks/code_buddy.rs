//! CodeBuddy command Hook 协议。

use serde_json::Value;

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, HookWriteOutcome,
    ManagedCommands, command_group,
};

/// CodeBuddy 协议单例。
pub(super) static CODE_BUDDY: CodeBuddyProtocol = CodeBuddyProtocol;

/// CodeBuddy 无状态协议实现。
pub(super) struct CodeBuddyProtocol;

/// CodeBuddy Code v1.16+ 中会影响展示状态的公开 Hook 生命周期。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    HookEvent::new(
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "PermissionDenied",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new("Elicitation", HookEventKind::State(HookBehavior::Asking)),
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new(
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::with_matcher("Notification", "idle_prompt", HookEventKind::Stop),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CodeBuddyProtocol {
    /// 返回 CodeBuddy 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::CodeBuddy
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "CodeBuddy"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "codebuddy"
    }

    /// 返回配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".codebuddy/settings.json"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// CodeBuddy 在各平台都经 Git Bash 执行 POSIX 命令。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(&commands.posix, event.matcher)
    }

    /// 写入后需在 Hooks 面板审核并重启或新建会话。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::CodeBuddyReviewRequired
    }
}
