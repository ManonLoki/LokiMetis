//! OpenClaw 扩展插件协议。

use super::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, HookBehavior, HookConfigPreview, HookEvent,
    HookEventKind, HookProtocol, HookWriteOutcome, managed_hook_marker,
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

    /// 生成读取当前 LokiMetis relay rendezvous 的 OpenClaw 扩展。
    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenClaw);
        Some(format!(
            r#"// {marker}
import {{ readFile }} from "node:fs/promises"
import {{ request }} from "node:http"
import {{ isAbsolute, join, parse }} from "node:path"

const instancePattern = /^[0-9a-f]{{8}}-[0-9a-f]{{4}}-4[0-9a-f]{{3}}-[89ab][0-9a-f]{{3}}-[0-9a-f]{{12}}$/

const absoluteEnv = (name) => {{
  const value = process.env[name]
  return value && isAbsolute(value) && parse(value).root !== value ? value : null
}}

const rendezvousPath = () => {{
  if (process.platform === "linux") {{
    const home = absoluteEnv("HOME")
    if (home) return join(home, ".cache", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
    throw new Error("LokiMetis relay requires absolute HOME")
  }}
  if (process.platform === "darwin") {{
    const home = absoluteEnv("HOME")
    if (home) return join(home, "Library", "Caches", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
    throw new Error("LokiMetis relay requires absolute HOME")
  }}
  if (process.platform === "win32") {{
    const userProfile = absoluteEnv("USERPROFILE")
    if (userProfile) return join(userProfile, "AppData", "Local", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
    throw new Error("LokiMetis relay requires absolute USERPROFILE")
  }}
  throw new Error("unsupported LokiMetis relay platform")
}}

const relayTarget = async () => {{
  const target = JSON.parse(await readFile(rendezvousPath(), "utf8"))
  if (target.schemaVersion !== {HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION}
      || !Number.isInteger(target.port) || target.port < 1 || target.port > 65535
      || typeof target.instanceId !== "string" || !instancePattern.test(target.instanceId)) {{
    throw new Error("invalid LokiMetis relay rendezvous")
  }}
  return target
}}

const postLocalHook = (target, hookEvent, body) => new Promise((resolve, reject) => {{
  let settled = false
  let responseStarted = false
  let relayRequest
  let deadline
  const settle = (error) => {{
    if (settled) return
    settled = true
    if (deadline) clearTimeout(deadline)
    if (error) reject(error)
    else resolve()
  }}
  const requestBody = Buffer.from(body, "utf8")
  relayRequest = request({{
    protocol: "http:",
    host: "127.0.0.1",
    port: target.port,
    path: `/api/hooks/${{target.instanceId}}/openclaw`,
    method: "POST",
    agent: false,
    headers: {{
      "Content-Type": "application/json; charset=utf-8",
      "Content-Length": String(requestBody.byteLength),
      "{HOOK_EVENT_TYPE_HEADER}": hookEvent,
      "{HOOK_RELAY_INSTANCE_HEADER}": target.instanceId,
    }},
  }}, (response) => {{
    responseStarted = true
    response.once("error", settle)
    response.once("aborted", () => settle(new Error("LokiMetis relay response aborted")))
    response.once("close", () => {{
      if (!settled) settle(new Error("LokiMetis relay response closed"))
    }})
    response.once("end", () => {{
      const status = response.statusCode ?? 0
      if (status < 200 || status >= 300
          || response.headers["x-lokimetis-hook-instance"] !== target.instanceId) {{
        settle(new Error("LokiMetis relay identity or status mismatch"))
      }} else {{
        settle()
      }}
    }})
    response.resume()
  }})
  relayRequest.once("error", settle)
  relayRequest.once("close", () => {{
    if (!settled && !responseStarted) settle(new Error("LokiMetis relay connection closed"))
  }})
  deadline = setTimeout(() => {{
    const error = new Error("LokiMetis relay deadline exceeded")
    relayRequest.destroy(error)
    settle(error)
  }}, 3000)
  relayRequest.end(requestBody)
}})

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
    const target = await relayTarget()
    await postLocalHook(target, hookEvent, body)
  }} catch {{
    // LokiMetis 未运行或正在重绑时保持单次短超时的 fail-open。
  }}
}}

export default {{
  id: "lokimetis",
  name: "LokiMetis",
  description: "Relay OpenClaw lifecycle state to the local LokiMetis desktop app",
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
