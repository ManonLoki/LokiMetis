//! 本文件收纳：Grok Build 的无状态 Hook 协议适配器；返回 Grok Build 的统一工具标识；返回用于界面展示的 Grok Build 名称。

// 引入 serde_json 的 Value，用于构造 Hook 配置 JSON
use serde_json::Value;

// 引入父模块中定义的公共类型与辅助函数
use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group, platform_command,
};
// 引入 AI 工具枚举、展示行为枚举与写入后结果
use crate::agent_hooks::{AiTool, HookBehavior, HookWriteOutcome};

// Grok Build 协议的单例静态实例，供注册表引用
pub(super) static GROK: GrokProtocol = GrokProtocol;

/// Grok Build 的无状态 Hook 协议适配器。
pub(super) struct GrokProtocol;

// 官方个人 hooks 使用 PascalCase 事件名与 Claude 风格 command group。
// 真实权限等待通过 Notification.permission_prompt 表达；不猜测 Claude 专属事件。
// https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md
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
        HookEventKind::WorkCompletion(HookBehavior::Error),
    ),
    HookEvent::new(
        "PermissionDenied",
        HookEventKind::WorkProgress(HookBehavior::Error),
    ),
    HookEvent::new("Stop", HookEventKind::ContinuableStop),
    HookEvent::new("StopFailure", HookEventKind::TerminalState(HookBehavior::Error)),
    HookEvent::new("StopCancelled", HookEventKind::Cancelled),
    HookEvent::with_matcher("Notification", "permission_prompt", HookEventKind::State(HookBehavior::Asking)),
    HookEvent::with_matcher("Notification", "agent_error", HookEventKind::TerminalState(HookBehavior::Error)),
    HookEvent::new(
        "SubagentStart",
        HookEventKind::UnscopedWorkStart(HookBehavior::Running),
    ),
    HookEvent::new(
        "SubagentStop",
        HookEventKind::UnscopedWorkCompletion,
    ),
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GrokProtocol {
    /// 返回 Grok Build 的统一工具标识。
    fn tool(&self) -> AiTool {
        AiTool::Grok
    }

    /// 返回与公开目录一致的界面名称。
    fn name(&self) -> &'static str {
        "Grok"
    }

    /// 返回用于路径和配置标识的稳定短名。
    fn slug(&self) -> &'static str {
        "grok"
    }

    /// 返回官方个人 hooks 目录内的独立托管配置相对路径。
    ///
    /// application 层会将其与用户选择的 `~/.grok` 或 `$GROK_HOME` 组合。
    fn config_filename(&self) -> &'static str {
        "hooks/lokimetris.json"
    }

    /// 返回面向用户展示的 Grok Hook 配置相对路径。
    fn preview_filename(&self) -> &'static str {
        ".grok/hooks/lokimetris.json"
    }

    /// 返回 Grok Build 支持的完整 Hook 事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }
    /// 已证实的提问工具会阻塞用户；普通执行前事件仍保持运行。
    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        match (event.name, status) {
            ("PreToolUse", Some("awaiting_user")) => HookEventKind::State(HookBehavior::Asking),
            ("PostToolUseFailure", Some("cancelled")) => HookEventKind::WorkCompletion(HookBehavior::Running),
            _ => event.kind,
        }
    }

    /// 按当前平台命令和可选 matcher 构造事件处理配置。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }

    /// 标记个人 hooks 写入后需要 `/hooks` 重载或新建会话。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
