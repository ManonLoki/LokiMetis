//! OpenCode 自动发现插件协议。

use std::path::Path;

use super::{AiTool, HookBehavior, HookEvent, HookEventKind, HookProtocol, managed_hook_marker};

/// OpenCode 协议单例。
pub(super) static OPEN_CODE: OpenCodeProtocol = OpenCodeProtocol;

/// OpenCode 无状态协议实现。
pub(super) struct OpenCodeProtocol;

/// OpenCode 插件归一化后会影响展示状态的事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("session.created", HookEventKind::SessionStart),
    HookEvent::new("session.busy", HookEventKind::WorkStart),
    HookEvent::new(
        "tool.execute.before",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "tool.execute.after",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "permission.asked",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("question.asked", HookEventKind::State(HookBehavior::Asking)),
    HookEvent::new("session.retry", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("session.error", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("session.idle", HookEventKind::Stop),
    HookEvent::new("session.deleted", HookEventKind::SessionEnd),
];

impl HookProtocol for OpenCodeProtocol {
    /// 返回 OpenCode 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::OpenCode
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "OpenCode"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "opencode"
    }

    /// 返回自动发现插件文件名。
    fn config_filename(&self) -> &'static str {
        "plugins/lokimetis.js"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".config/opencode/plugins/lokimetis.js"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// 生成只通过 LokiMetis CLI relay 转发事件的 OpenCode 插件。
    fn standalone_config(&self, relay_executable: &Path) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenCode);
        let executable_literal =
            serde_json::Value::String(relay_executable.to_string_lossy().into_owned()).to_string();
        let forward_through_cli = super::js_cli_relay_forwarder("opencode", &marker);
        Some(format!(
            r#"// {marker}
import {{ spawn }} from "node:child_process"

const failedSessions = new Set()
const relayExecutable = {executable_literal}

{forward_through_cli}

const supportedEvents = new Set([
  "session.created", "tool.execute.before", "tool.execute.after",
  "permission.asked", "question.asked", "session.idle", "session.deleted",
])

const normalizedEvent = (event) => {{
  const properties = event.properties ?? {{}}
  const sessionID = properties.sessionID ?? properties.info?.id ?? null
  if (event.type === "session.status") {{
    const status = properties.status?.type
    if (status === "busy") {{
      if (sessionID) failedSessions.delete(sessionID)
      return "session.busy"
    }}
    if (status === "retry") {{
      if (sessionID) failedSessions.add(sessionID)
      return "session.retry"
    }}
    if (status === "idle") {{
      return sessionID && failedSessions.has(sessionID) ? null : "session.idle"
    }}
    return null
  }}
  if (event.type === "session.error") {{
    if (sessionID) failedSessions.add(sessionID)
    return "session.error"
  }}
  if (event.type === "session.idle" && sessionID && failedSessions.has(sessionID)) return null
  if (event.type === "session.deleted" && sessionID) failedSessions.delete(sessionID)
  return supportedEvents.has(event.type) ? event.type : null
}}

const send = async (event) => {{
  const hookEvent = normalizedEvent(event)
  if (!hookEvent) return
  const properties = event.properties ?? {{}}
  const body = JSON.stringify({{
    hook_event_name: hookEvent,
    session_id: properties.sessionID ?? properties.info?.id ?? null,
    status: properties.status?.type ?? null,
  }})
  try {{
    await forwardThroughCli(hookEvent, body)
  }} catch {{
    // LokiMetis CLI 不可用时保持单次短超时的 fail-open。
  }}
}}

export const LokiMetisPlugin = async () => ({{
  event: async ({{ event }}) => send(event),
}})
"#
        ))
    }

    /// OpenCode 通过自动发现的独立插件文件接入。
    fn uses_standalone_plugin(&self) -> bool {
        true
    }
}
