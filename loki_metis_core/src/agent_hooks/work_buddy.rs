// serde_json 的通用值类型，handler 用它拼装 JSON 片段
use serde_json::Value;

// 引入 Hook 事件/事件类型定义、协议 trait、受管命令集合，以及公共的
// command-group 构造辅助函数
use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group};
// 引入 AI 工具枚举、四态展示行为枚举，以及写入结果枚举
use crate::agent_hooks::{AiTool, HookBehavior, HookWriteOutcome};

// WorkBuddy 协议的静态单例，供 `protocol()` 分发函数按 AiTool::WorkBuddy 取用
pub(super) static WORK_BUDDY: WorkBuddyProtocol = WorkBuddyProtocol;

/// WorkBuddy 的无状态 Hook 协议适配器。
pub(super) struct WorkBuddyProtocol;

// WorkBuddy 内置 CodeBuddy Agent 引擎，并从 v2.48 起使用独立的
// ~/.workbuddy/settings.json；其 hooks 结构与 CodeBuddy/Claude Code 兼容。
const EVENTS: &[HookEvent] = &[
    // 会话开始 -> 会话开始
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    // 用户提交提示词 -> 工作开始
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    HookEvent::new(
        // 工具调用前 -> 工作进行中（运行态）
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        // 工具调用后 -> 工作完成（仍视为运行态，等待下一步）
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        // 工具调用失败 -> 出错态
        "PostToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        // 请求权限 -> 等待用户确认态
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    // 请求澄清 -> 等待用户确认态
    HookEvent::new("Elicitation", HookEventKind::State(HookBehavior::Asking)),
    // 停止 -> 回到空闲展示
    HookEvent::new("Stop", HookEventKind::Stop),
    // 停止失败 -> 出错态
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new(
        // 子代理开始 -> 工作进行中（运行态）
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        // 子代理结束 -> 工作完成（运行态）
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        // 压缩上下文前 -> 工作进行中（运行态）
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        // 压缩上下文后 -> 工作完成（运行态）
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    // 带 idle_prompt matcher 的 Notification -> 回到空闲展示
    HookEvent::with_matcher("Notification", "idle_prompt", HookEventKind::Stop),
    // 会话结束 -> 会话结束
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

// 为 WorkBuddy 实现统一的 Hook 协议 trait
impl HookProtocol for WorkBuddyProtocol {
    /// 返回 WorkBuddy 的统一工具标识。
    fn tool(&self) -> AiTool {
        AiTool::WorkBuddy
    }
    /// 返回用于界面展示的 WorkBuddy 名称。
    fn name(&self) -> &'static str {
        "WorkBuddy"
    }
    /// 返回用于路径和配置标识的稳定短名。
    fn slug(&self) -> &'static str {
        "workbuddy"
    }
    /// 返回 WorkBuddy 配置根下的 Hook 配置文件名。
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }
    /// 返回面向用户展示的 Hook 配置相对路径。
    fn preview_filename(&self) -> &'static str {
        ".workbuddy/settings.json"
    }
    /// 返回 WorkBuddy 支持的完整 Hook 事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 使用 WorkBuddy 固定要求的 POSIX 命令构造事件处理配置。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // WorkBuddy 的内置 CodeBuddy 引擎在 Windows 上也固定使用 Git Bash
        // 执行 command Hook；cmd.exe 包装命令会被 Bash 错误解析。
        // 因此始终使用 POSIX 命令变体，不走 `platform_command` 的平台分支。
        command_group(&commands.posix, event.matcher)
    }

    /// 标记 WorkBuddy 写入后需要在 Hooks 面板审核并重新加载规则。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::WorkBuddyReviewRequired
    }
}
