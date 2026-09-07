//! Hermes observer 插件协议。

use std::path::Path;

use super::{
    AiTool, HookBehavior, HookConfigPreview, HookEvent, HookEventKind, HookProtocol,
    HookWriteOutcome, managed_hook_marker,
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

    /// 生成只通过 LokiMetis CLI relay 转发事件的 Hermes 插件。
    fn standalone_config(&self, relay_executable: &Path) -> Option<String> {
        let marker = managed_hook_marker(AiTool::Hermes);
        let executable_literal =
            serde_json::Value::String(relay_executable.to_string_lossy().into_owned()).to_string();
        Some(format!(
            r#"# {marker}
"""Relay Hermes observer lifecycle events through the LokiMetis CLI."""

from __future__ import annotations

import json
import subprocess


RELAY_EXECUTABLE = {executable_literal}


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
    }})
    try:
        subprocess.run(
            [
                RELAY_EXECUTABLE,
                "--loki-metis-hook-relay",
                "hermes",
                hook_event,
                "--managed-by",
                "{marker}",
            ],
            input=payload,
            text=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=4,
            check=False,
        )
    except Exception:
        # LokiMetis CLI 不可用时保持单次短超时的 fail-open。
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

    /// Hermes 通过 observer 独立插件文件接入。
    fn uses_standalone_plugin(&self) -> bool {
        true
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
