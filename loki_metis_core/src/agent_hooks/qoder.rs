//! Qoder command Hook 协议。

use serde_json::Value;

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group,
    platform_command,
};

/// Qoder 协议单例。
pub(super) static QODER: QoderProtocol = QoderProtocol;

/// Qoder 无状态协议实现。
pub(super) struct QoderProtocol;

/// IDE、JetBrains 插件和 CLI 共用的五事件兼容基线。
const EVENTS: &[HookEvent] = &[
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
    HookEvent::new("Stop", HookEventKind::Stop),
];

impl HookProtocol for QoderProtocol {
    /// 返回 Qoder 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::Qoder
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Qoder"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "qoder"
    }

    /// 返回配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".qoder/settings.json"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 生成 Claude-compatible command group。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }
}
