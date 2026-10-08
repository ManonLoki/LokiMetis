//! Qwen Code command Hook 协议。

use serde_json::Value;

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, HookWriteOutcome,
    ManagedCommands, command_group, platform_command,
};

/// Qwen Code 协议单例。
pub(super) static QWEN_CODE: QwenCodeProtocol = QwenCodeProtocol;

/// Qwen Code 无状态协议实现。
pub(super) struct QwenCodeProtocol;

/// Qwen Code 用户级设置中会稳定推进四态展示的事件。
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
    HookEvent::new("Stop", HookEventKind::Stop),
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

impl HookProtocol for QwenCodeProtocol {
    /// 返回 Qwen Code 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::QwenCode
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Qwen Code"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "qwen-code"
    }

    /// 返回配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".qwen/settings.json"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 生成 Claude-compatible command group。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }

    /// 新会话才会重新读取用户设置。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
