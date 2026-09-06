//! Cursor command Hook 协议。

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::{
    AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, ManagedCommands,
    entry_is_managed, platform_command,
};

/// Cursor 协议单例。
pub(super) static CURSOR: CursorProtocol = CursorProtocol;

/// Cursor 无状态协议实现。
pub(super) struct CursorProtocol;

/// Cursor 快速重入同一 conversation 时的释放交接缓冲。
const RELEASE_SETTLE_DELAY: Duration = Duration::from_millis(250);

/// Cursor 支持的公开 Hook 事件及归一化语义。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("workspaceOpen", HookEventKind::WorkspaceStart),
    HookEvent::new("sessionStart", HookEventKind::SessionStart),
    HookEvent::new("beforeSubmitPrompt", HookEventKind::WorkStart),
    HookEvent::new(
        "afterFileEdit",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterShellExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterMCPExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "beforeShellExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "beforeMCPExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
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
        HookEventKind::WorkProgress(HookBehavior::Error),
    ),
    HookEvent::new(
        "subagentStart",
        HookEventKind::UnscopedWorkStart(HookBehavior::Running),
    ),
    HookEvent::new("subagentStop", HookEventKind::UnscopedWorkCompletion),
    HookEvent::new(
        "preCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentResponse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentThought",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("stop", HookEventKind::Stop),
    HookEvent::new("sessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CursorProtocol {
    /// 返回 Cursor 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::Cursor
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Cursor"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "cursor"
    }

    /// 返回配置文件名。
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".cursor/hooks.json"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// Cursor 的 stop 结合 status 区分正常结束和错误终止。
    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        let failed = status.is_some_and(|value| value.eq_ignore_ascii_case("error"));
        if event.name == "stop" && failed {
            HookEventKind::TerminalState(HookBehavior::Error)
        } else {
            event.kind
        }
    }

    /// 返回 Cursor 的释放交接缓冲。
    fn release_settle_delay(&self) -> Duration {
        RELEASE_SETTLE_DELAY
    }

    /// Cursor 的 sessionStart 不复活已经结束的墓碑会话。
    fn session_start_revives_tombstone(&self) -> bool {
        false
    }

    /// 生成 Cursor 所需的扁平 command 条目。
    fn handler(&self, _event: &HookEvent, commands: &ManagedCommands) -> Value {
        #[cfg(target_os = "windows")]
        let command = if commands.is_wsl {
            platform_command(commands)
        } else {
            &commands.windows_powershell_host
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let command = platform_command(commands);
        json!([{ "command": command }])
    }

    /// Cursor 配置根携带固定版本字段。
    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    }

    /// Cursor 的事件值是扁平数组，直接过滤受管条目。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain(|entry| !entry_is_managed(entry, self));
    }
}
