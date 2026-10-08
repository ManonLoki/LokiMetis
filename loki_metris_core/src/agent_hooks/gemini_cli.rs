//! Gemini CLI command Hook 协议。

use serde_json::Value;

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, HookWriteOutcome,
    ManagedCommands, command_group, platform_command,
};

/// Gemini CLI 协议单例。
pub(super) static GEMINI_CLI: GeminiCliProtocol = GeminiCliProtocol;

/// Gemini CLI 无状态协议实现。
pub(super) struct GeminiCliProtocol;

/// Gemini CLI 用户级设置中会影响展示状态的公开事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("BeforeAgent", HookEventKind::WorkStart),
    HookEvent::new(
        "BeforeModel",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "AfterModel",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "BeforeToolSelection",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "BeforeTool",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "AfterTool",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompress",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::with_matcher(
        "Notification",
        "ToolPermission",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("AfterAgent", HookEventKind::Stop),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GeminiCliProtocol {
    /// 返回 Gemini CLI 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::GeminiCli
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "gemini-cli"
    }

    /// 返回配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".gemini/settings.json"
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
