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

// Grok Build 协议的空结构体（无状态，仅承载 trait 实现）
pub(super) struct GrokProtocol;

// 官方个人 hooks 使用 PascalCase 事件名与 Claude 风格 command group。
// 不订阅 Claude 专属 PermissionRequest / Elicitation；官方表没有 ask/Q&A 事件，
// Asking 为契约降级。不给 Notification 加 matcher（idle_prompt / permission_prompt
// 不是可移植的无 matcher 语义）。StopCancelled 是当前官方 turn-end 事件，
// 用于用户中断后回到 Idle。
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
        "PermissionDenied",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("StopCancelled", HookEventKind::Stop),
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
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GrokProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Grok
    }

    fn name(&self) -> &'static str {
        "Grok Build"
    }

    fn slug(&self) -> &'static str {
        "grok"
    }

    // application 层把用户选择的 ~/.grok（或 $GROK_HOME）与该相对路径组合成
    // 官方个人 hooks 目录下的独立托管文件。
    fn config_filename(&self) -> &'static str {
        "hooks/lokimetis.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".grok/hooks/lokimetis.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        command_group(platform_command(commands), event.matcher)
    }

    // 个人 hooks 热加载需 /hooks 重载或新会话；写入后按重启/新建会话处理。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
