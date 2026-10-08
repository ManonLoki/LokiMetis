// 引入 serde_json 的 Value，用于构造 Hook 配置 JSON
use serde_json::Value;

// 引入父模块（hooks）中定义的公共类型与辅助函数
use super::{
    // 单个 Hook 事件描述、事件类型枚举、协议 trait、托管命令集合、
    // 生成 command group JSON 的辅助函数、按平台选择命令的辅助函数
    HookEvent,
    HookEventKind,
    HookProtocol,
    ManagedCommands,
    command_group,
    platform_command,
};
// 引入 AI 工具枚举与 Hook 展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// Claude Code 协议的单例静态实例，供注册表引用
pub(super) static CLAUDE_CODE: ClaudeCodeProtocol = ClaudeCodeProtocol;

/// Claude Code 的无状态 Hook 协议适配器。
pub(super) struct ClaudeCodeProtocol;

// Claude Code 支持的公开 Hook 事件列表及其归一化事件类型
const EVENTS: &[HookEvent] = &[
    // 会话开始事件
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    // 用户提交提示词，视为一次工作开始
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    // 工具调用前，视为工作进度（运行中）
    HookEvent::new(
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    // 工具调用后，视为工作完成（运行中展示态）
    HookEvent::new(
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    // 请求权限，视为询问态
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    // 自动权限模式拒绝工具调用；Claude 仍可处理或重试，不结束本轮。
    HookEvent::new(
        "PermissionDenied",
        HookEventKind::WorkProgress(HookBehavior::Error),
    ),
    // 需要用户澄清/补充信息，同样视为询问态
    HookEvent::new("Elicitation", HookEventKind::State(HookBehavior::Asking)),
    // 工具调用失败不代表轮次结束，后续工具调用仍可继续。
    HookEvent::new(
        "PostToolUseFailure",
        HookEventKind::WorkCompletion(HookBehavior::Error),
    ),
    // 明确的停止事件
    HookEvent::new("Stop", HookEventKind::Stop),
    // API 错误使本轮结束，不能再接纳该轮迟到的工具事件。
    HookEvent::new(
        "StopFailure",
        HookEventKind::TerminalState(HookBehavior::Error),
    ),
    // MCP 澄清回答已提交，工具调用继续。
    HookEvent::new(
        "ElicitationResult",
        HookEventKind::State(HookBehavior::Running),
    ),
    // 子代理开始，视为工作进度
    HookEvent::new(
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    // 子代理结束，视为工作完成
    HookEvent::new(
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    // 压缩前，视为工作进度
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    // 压缩后，视为工作完成
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    // 空闲提醒与权限请求共用原生 Notification 事件名，分别保留两个 matcher。
    HookEvent::with_matcher("Notification", "idle_prompt", HookEventKind::Stop),
    HookEvent::with_matcher(
        "Notification",
        "permission_prompt",
        HookEventKind::State(HookBehavior::Asking),
    ),
    // 会话结束事件
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

// 为 Claude Code 实现 HookProtocol trait
impl HookProtocol for ClaudeCodeProtocol {
    /// 返回 Claude Code 的统一工具标识。
    fn tool(&self) -> AiTool {
        AiTool::ClaudeCode
    }
    /// 返回用于界面展示的 Claude Code 名称。
    fn name(&self) -> &'static str {
        "Claude Code"
    }
    /// 返回用于路径和配置标识的稳定短名。
    fn slug(&self) -> &'static str {
        "claude-code"
    }
    /// 返回 Claude Code 配置根下的 Hook 配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }
    /// 返回面向用户展示的 Hook 配置相对路径。
    fn preview_filename(&self) -> &'static str {
        ".claude/settings.json"
    }
    /// 返回 Claude Code 支持的完整 Hook 事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 按当前平台命令和可选 matcher 构造事件处理配置。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }
}
