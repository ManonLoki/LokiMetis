//! OpenCode 自动发现插件协议。

use super::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, HookBehavior, HookEvent, HookEventKind, HookProtocol,
    managed_hook_marker,
};

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

    /// 生成读取当前 LokiMetis relay rendezvous 的 OpenCode 插件。
    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::OpenCode);
        Some(format!(
            r#"// {marker}
import {{ readFile }} from "node:fs/promises"
import {{ request }} from "node:http"
import {{ isAbsolute, join, parse }} from "node:path"

const failedSessions = new Set()
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
    path: `/api/hooks/${{target.instanceId}}/opencode`,
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
  const supported = new Set([
    "session.created", "tool.execute.before", "tool.execute.after",
    "permission.asked", "question.asked", "session.idle", "session.deleted",
  ])
  return supported.has(event.type) ? event.type : null
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
    const target = await relayTarget()
    await postLocalHook(target, hookEvent, body)
  }} catch {{
    // LokiMetis 未运行或正在重绑时保持单次短超时的 fail-open。
  }}
}}

export const LokiMetisPlugin = async () => ({{
  event: async ({{ event }}) => send(event),
}})
"#
        ))
    }
}
