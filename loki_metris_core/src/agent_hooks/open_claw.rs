//! OpenClaw 扩展插件协议。

use std::path::Path;

use super::{
    AiTool, HookBehavior, HookConfigPreview, HookEvent, HookEventKind, HookProtocol,
    HookWriteOutcome, managed_hook_marker,
};

/// OpenClaw 协议单例。
pub(super) static OPEN_CLAW: OpenClawProtocol = OpenClawProtocol;

/// OpenClaw 无状态协议实现。
pub(super) struct OpenClawProtocol;

/// OpenClaw register() 订阅的公开生命周期事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("session_start", HookEventKind::SessionStart),
    HookEvent::new("before_agent_run", HookEventKind::WorkStart),
    HookEvent::new(
        "before_tool_call",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "after_tool_call",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("agent_end", HookEventKind::Stop),
    HookEvent::new("session_end", HookEventKind::SessionEnd),
];

impl HookProtocol for OpenClawProtocol {
    /// 返回 OpenClaw 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::OpenClaw
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "OpenClaw"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "openclaw"
    }

    /// 返回扩展入口文件名。
    fn config_filename(&self) -> &'static str {
        "extensions/lokimetis/index.mjs"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".openclaw/extensions/lokimetis/index.mjs"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// agent_end 的失败结果映射为错误态。
    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        if event.name == "agent_end" && matches!(status, Some("error" | "failed" | "false")) {
            HookEventKind::State(HookBehavior::Error)
        } else {
            event.kind
        }
    }

    /// 插件写入后需要显式启用并重启 Gateway。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::OpenClawEnableRequired
    }

    /// 生成只通过 LokiMetis CLI relay 转发事件的 OpenClaw 扩展。
    fn standalone_config(&self, relay_executable: &Path) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenClaw);
        let executable_literal =
            serde_json::Value::String(relay_executable.to_string_lossy().into_owned()).to_string();
        let forward_through_cli = super::js_cli_relay_forwarder("openclaw", &marker);
        Some(format!(
            r#"// {marker}
import {{ spawn }} from "node:child_process"

const relayExecutable = {executable_literal}

{forward_through_cli}

const send = async (hookEvent, event = {{}}, ctx = {{}}) => {{
  const body = JSON.stringify({{
    hook_event_name: hookEvent,
    session_id: ctx.sessionId ?? ctx.sessionKey ?? event.sessionId ?? event.sessionKey ?? null,
    turn_id: event.runId ?? ctx.runId ?? null,
    status: hookEvent === "agent_end"
      ? (event.success === false ? "failed" : (event.outcome ?? "success"))
      : null,
  }})
  try {{
    await forwardThroughCli(hookEvent, body)
  }} catch {{
    // LokiMetis CLI 不可用时保持单次短超时的 fail-open。
  }}
}}

export default {{
  id: "lokimetis",
  name: "LokiMetis",
  description: "Relay OpenClaw lifecycle state through the local LokiMetis CLI",
  register(api) {{
    for (const hook of [
      "session_start", "before_agent_run", "before_tool_call",
      "after_tool_call", "agent_end", "session_end",
    ]) {{
      api.on(hook, (event, ctx) => send(hook, event, ctx))
    }}
  }},
}}
"#
        ))
    }

    /// OpenClaw 通过扩展独立插件文件接入。
    fn uses_standalone_plugin(&self) -> bool {
        true
    }

    /// 返回 OpenClaw 扩展清单与 package.json。
    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        let marker = managed_hook_marker(AiTool::OpenClaw);
        vec![
            HookConfigPreview {
                filename: "extensions/lokimetis/openclaw.plugin.json".to_owned(),
                content: format!(
                    r#"{{
  "id": "lokimetis",
  "name": "LokiMetis",
  "description": "{marker} - relay OpenClaw lifecycle state to LokiMetis",
  "version": "1.0.0",
  "activation": {{ "onStartup": true }},
  "configSchema": {{ "type": "object", "additionalProperties": false }}
}}"#
                ),
            },
            HookConfigPreview {
                filename: "extensions/lokimetis/package.json".to_owned(),
                content: format!(
                    r#"{{
  "name": "lokimetis-openclaw-plugin",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "description": "{marker}",
  "openclaw": {{ "extensions": ["./index.mjs"] }}
}}"#
                ),
            },
        ]
    }
}
