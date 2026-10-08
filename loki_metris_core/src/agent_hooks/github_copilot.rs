//! GitHub Copilot CLI command Hook 协议。

use serde_json::{Map, Value, json};

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, HookWriteOutcome,
    ManagedCommands, entry_is_managed, platform_command,
};

/// GitHub Copilot CLI 协议单例。
pub(super) static GITHUB_COPILOT: GitHubCopilotProtocol = GitHubCopilotProtocol;

/// GitHub Copilot CLI 无状态协议实现。
pub(super) struct GitHubCopilotProtocol;

/// Copilot CLI 会稳定推进四态展示的公开事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("sessionStart", HookEventKind::SessionStart),
    HookEvent::new("userPromptSubmitted", HookEventKind::WorkStart),
    HookEvent::new(
        "preToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "permissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("agentStop", HookEventKind::Stop),
    HookEvent::new(
        "subagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "subagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "preCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new("errorOccurred", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("sessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GitHubCopilotProtocol {
    /// 返回 GitHub Copilot CLI 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::GitHubCopilot
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "GitHub Copilot CLI"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "github-copilot"
    }

    /// 返回 LokiMetis 独立 Hook 文件名。
    fn config_filename(&self) -> &'static str {
        "hooks/lokimetis.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".copilot/hooks/lokimetis.json"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 生成 Copilot 所需的扁平 command handler。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        #[cfg(target_os = "windows")]
        let command = if commands.is_wsl {
            platform_command(commands)
        } else {
            &commands.windows_powershell_host
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        let mut handler = json!({
            "type": "command",
            "command": command,
        });
        if let Some(matcher) = event.matcher {
            handler["matcher"] = Value::String(matcher.to_owned());
        }
        Value::Array(vec![handler])
    }

    /// Copilot 配置根携带固定版本字段。
    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    }

    /// Copilot 的事件值是扁平数组，直接过滤受管条目。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain(|entry| !entry_is_managed(entry, self));
    }

    /// Copilot CLI 启动时加载配置，写入后需重启或新建会话。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
