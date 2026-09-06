//! Hermes observer 插件协议。

use super::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, HookBehavior, HookConfigPreview, HookEvent,
    HookEventKind, HookProtocol, HookWriteOutcome, managed_hook_marker,
};

/// Hermes 协议单例。
pub(super) static HERMES: HermesProtocol = HermesProtocol;

/// Hermes 无状态协议实现。
pub(super) struct HermesProtocol;

/// Hermes observer 插件订阅的官方生命周期事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("on_session_start", HookEventKind::SessionStart),
    HookEvent::new("pre_llm_call", HookEventKind::WorkStart),
    HookEvent::new(
        "pre_tool_call",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "post_tool_call",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "pre_approval_request",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "post_approval_response",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "api_request_error",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new("post_llm_call", HookEventKind::Stop),
    HookEvent::new("on_session_end", HookEventKind::Stop),
    HookEvent::new("on_session_finalize", HookEventKind::SessionEnd),
    HookEvent::new("on_session_reset", HookEventKind::SessionEnd),
];

impl HookProtocol for HermesProtocol {
    /// 返回 Hermes 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::Hermes
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Hermes"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "hermes"
    }

    /// 返回插件入口文件名。
    fn config_filename(&self) -> &'static str {
        "plugins/lokimetis/__init__.py"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".hermes/plugins/lokimetis/__init__.py"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// Hermes 插件需显式启用并新建会话或重启。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::HermesEnableRequired
    }

    /// 生成读取当前 LokiMetis relay rendezvous 的 Hermes 插件。
    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::Hermes);
        Some(format!(
            r#"# {marker}
"""Relay Hermes observer lifecycle events to the local LokiMetis app."""

from __future__ import annotations

import json
import os
import sys
import urllib.request
import uuid


def _absolute_env(name: str):
    value = os.environ.get(name)
    if not value or not os.path.isabs(value):
        return None
    normalized = os.path.normpath(value)
    return value if os.path.dirname(normalized) != normalized else None


def _rendezvous_path():
    if sys.platform.startswith("linux"):
        home = _absolute_env("HOME")
        if home:
            return os.path.join(home, ".cache", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
        raise RuntimeError("LokiMetis relay requires absolute HOME")
    if sys.platform == "darwin":
        home = _absolute_env("HOME")
        if home:
            return os.path.join(home, "Library", "Caches", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
        raise RuntimeError("LokiMetis relay requires absolute HOME")
    if sys.platform == "win32":
        user_profile = _absolute_env("USERPROFILE")
        if user_profile:
            return os.path.join(user_profile, "AppData", "Local", "lokimetis", "{HOOK_RELAY_RENDEZVOUS_FILENAME}")
        raise RuntimeError("LokiMetis relay requires absolute USERPROFILE")
    raise RuntimeError("unsupported LokiMetis relay platform")


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


NO_REDIRECT_OPENER = urllib.request.build_opener(
    urllib.request.ProxyHandler({{}}),
    _NoRedirectHandler(),
)


def _relay_target():
    with open(_rendezvous_path(), "r", encoding="utf-8") as source:
        target = json.load(source)
    port = target.get("port")
    instance_id = target.get("instanceId")
    parsed_instance_id = uuid.UUID(instance_id) if isinstance(instance_id, str) else None
    if (
        target.get("schemaVersion") != {HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION}
        or not isinstance(port, int)
        or isinstance(port, bool)
        or not 1 <= port <= 65535
        or not isinstance(instance_id, str)
        or parsed_instance_id.version != 4
        or str(parsed_instance_id) != instance_id
    ):
        raise ValueError("invalid LokiMetis relay rendezvous")
    return port, instance_id


def _send(hook_event: str, **kwargs) -> None:
    session_id = kwargs.get("session_id") or kwargs.get("session_key")
    turn_id = kwargs.get("turn_id") or kwargs.get("task_id")
    if hook_event == "on_session_reset":
        session_id = kwargs.get("old_session_id") or session_id
    status = kwargs.get("status") or kwargs.get("choice") or kwargs.get("reason")
    payload = json.dumps({{
        "hook_event_name": hook_event,
        "session_id": session_id,
        "turn_id": turn_id,
        "status": str(status) if status is not None else None,
    }}).encode("utf-8")
    try:
        port, instance_id = _relay_target()
        request = urllib.request.Request(
            f"http://127.0.0.1:{{port}}/api/hooks/{{instance_id}}/hermes",
            data=payload,
            headers={{
                "Content-Type": "application/json; charset=utf-8",
                "Content-Length": str(len(payload)),
                "{HOOK_EVENT_TYPE_HEADER}": hook_event,
                "{HOOK_RELAY_INSTANCE_HEADER}": instance_id,
            }},
            method="POST",
        )
        with NO_REDIRECT_OPENER.open(request, timeout=1) as response:
            if not 200 <= response.status < 300:
                raise RuntimeError("LokiMetis relay rejected the Hook")
            if response.getheader("{HOOK_RELAY_INSTANCE_HEADER}") != instance_id:
                raise RuntimeError("LokiMetis relay identity mismatch")
            response.read()
    except Exception:
        # LokiMetis 未运行或正在重绑时保持单次短超时的 fail-open。
        pass


def register(ctx) -> None:
    for hook_event in (
        "on_session_start",
        "pre_llm_call",
        "pre_tool_call",
        "post_tool_call",
        "pre_approval_request",
        "post_approval_response",
        "api_request_error",
        "post_llm_call",
        "on_session_end",
        "on_session_finalize",
        "on_session_reset",
    ):
        ctx.register_hook(
            hook_event,
            lambda _event=hook_event, **kwargs: _send(_event, **kwargs),
        )
"#
        ))
    }

    /// 返回 Hermes 插件清单。
    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        let marker = managed_hook_marker(AiTool::Hermes);
        vec![HookConfigPreview {
            filename: "plugins/lokimetis/plugin.yaml".to_owned(),
            content: format!(
                r#"name: lokimetis
version: 1.0.0
description: "{marker} - relay Hermes lifecycle state to LokiMetis"
author: LokiMetis
hooks:
  - on_session_start
  - pre_llm_call
  - pre_tool_call
  - post_tool_call
  - pre_approval_request
  - post_approval_response
  - api_request_error
  - post_llm_call
  - on_session_end
  - on_session_finalize
  - on_session_reset
"#
            ),
        }]
    }
}
