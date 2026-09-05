// 引入 serde_json 的 Value 与 json! 宏，用于构造 Hook 配置 JSON
use serde_json::{Value, json};

// 引入父模块（hooks）中定义的公共类型与辅助函数：事件描述、事件类型枚举、
// 协议 trait、托管命令集合、按平台选择命令的辅助函数
use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, platform_command};
// 引入 AI 工具枚举、Hook 展示行为枚举、写入结果枚举
use crate::agent_hooks::{AiTool, HookBehavior, HookWriteOutcome};

// Codex 协议的单例静态实例，供注册表引用
pub(super) static CODEX: CodexProtocol = CodexProtocol;

// Codex 协议的空结构体（无状态，仅承载 trait 实现）
pub(super) struct CodexProtocol;

// Codex 支持的公开 Hook 事件列表及其归一化事件类型
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
    // 明确的停止事件
    HookEvent::new("Stop", HookEventKind::Stop),
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
    // 会话结束事件
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

// 为 Codex 实现 HookProtocol trait
impl HookProtocol for CodexProtocol {
    // 该协议对应的 AI 工具标识
    fn tool(&self) -> AiTool {
        AiTool::Codex
    }
    // 展示用的工具名称
    fn name(&self) -> &'static str {
        "Codex"
    }
    // 用于路径/标识拼接的短标识符
    fn slug(&self) -> &'static str {
        "codex"
    }
    // 配置文件名
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }
    // 预览展示用的相对路径
    fn preview_filename(&self) -> &'static str {
        ".codex/hooks.json"
    }
    // 返回该协议支持的全部事件列表
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    // 构造该事件对应的 Hook 配置条目
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // 仅在 Windows 平台下编译：优先按平台选择命令，
        // 但若并非运行在 WSL 中，则改用专门的 PowerShell 宿主命令
        #[cfg(target_os = "windows")]
        let command = if commands.is_wsl {
            platform_command(commands)
        } else {
            &commands.windows_powershell_host
        };
        // 仅在 macOS / Linux 平台下编译：直接使用按平台选择的命令
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        // 构造基础的 command 类型 Hook 条目
        let mut command = json!({
            "type": "command",
            "command": command,
        });
        // SessionEnd 事件需要限定超时时间，避免进程退出时 Hook 阻塞过久
        if event.name == "SessionEnd" {
            command["timeout"] = json!(3);
        }
        // 包装为 Codex 期望的 hooks 数组结构
        json!([{ "hooks": [command] }])
    }

    // Codex 需要运行 /hooks 审核，并重启 App 或新建任务加载新规则。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::CodexReviewRequired
    }
}
