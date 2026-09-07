//! 十四项 Agent 的 Hook 配置生成契约。

use std::path::Path;

use serde_json::Value;

use super::super::{
    MANAGED_HOOK_PREFIX, generate_hook_auxiliary_configs, hook_config_filename,
    hook_restart_required, hook_supports_wsl, managed_hook_marker, protocol, tool_from_slug,
};
use super::{command_has_marker, generate_hook_config, merge_hook_config};
use crate::agent_hooks::{AiTool, HookConfigPreview, HookError};

/// 用固定示例路径生成配置，供本文件与合并测试复用。
pub(super) fn generate_test_hook_config(tool: AiTool) -> Result<HookConfigPreview, HookError> {
    generate_hook_config(tool, Path::new("/opt/LokiMetis/loki_metis_gui"))
}

/// 十四项工具的生成配置都使用规范 slug、事件名与管理标识。
#[test]
fn every_generated_hook_contract_uses_canonical_slugs_events_and_markers() {
    for tool in AiTool::ALL {
        let protocol = protocol(tool);
        let slug = protocol.slug();
        let marker = managed_hook_marker(tool);
        let preview = generate_test_hook_config(tool).unwrap();
        assert_eq!(tool_from_slug(slug), Some(tool));
        assert_eq!(marker, format!("LokiMetis:tool={slug}"));
        assert!(preview.content.contains(&marker));
        assert!(!preview.content.contains("LokiMetis|tool="));
        for event in protocol.events() {
            assert!(
                preview.content.contains(event.name),
                "{} 的生成配置缺少协议事件 {}",
                protocol.name(),
                event.name
            );
        }
        assert!(preview.content.contains("--loki-metis-hook-relay"));
        assert!(preview.content.contains("--managed-by"));
        if protocol.uses_standalone_plugin() {
            assert!(preview.content.contains("/opt/LokiMetis/loki_metis_gui"));
            assert!(preview.content.contains("hook_event_name"));
            assert!(preview.content.contains("session_id"));
            assert!(preview.content.contains("status"));
            assert!(
                generate_hook_auxiliary_configs(tool)
                    .iter()
                    .all(|file| file.content.contains(&marker))
            );
        } else {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            assert!(preview.content.contains(&format!("'{slug}'")));
        }
    }
}

/// Claude Code 覆盖权限与生命周期事件。
#[test]
fn claude_preview_covers_permission_and_lifecycle_events() {
    let preview = generate_test_hook_config(AiTool::ClaudeCode).unwrap();
    assert_eq!(preview.filename, ".claude/settings.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"PermissionRequest\""));
    assert!(preview.content.contains("LokiMetis:tool=claude-code"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
    assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
}

/// Codex 使用 PascalCase 嵌套 handler 且 SessionEnd 带超时。
#[test]
fn codex_preview_uses_pascal_case_and_nested_handlers() {
    let preview = generate_test_hook_config(AiTool::Codex).unwrap();
    assert_eq!(preview.filename, ".codex/hooks.json");
    assert!(preview.content.contains("\"SessionStart\""));
    assert!(preview.content.contains("\"UserPromptSubmit\""));
    assert!(preview.content.contains("LokiMetis:tool=codex"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(session_end.contains("--loki-metis-hook-relay"));
    assert!(session_end.contains("SessionEnd"));
    assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
    let command = value["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.contains(MANAGED_HOOK_PREFIX));
    assert!(command_has_marker(
        command,
        &managed_hook_marker(AiTool::Codex)
    ));
}

/// WorkBuddy 写入独立 settings 且使用 POSIX 命令。
#[test]
fn work_buddy_preview_targets_its_independent_settings_file() {
    let preview = generate_test_hook_config(AiTool::WorkBuddy).unwrap();
    assert_eq!(preview.filename, ".workbuddy/settings.json");
    assert!(preview.content.contains("LokiMetis:tool=workbuddy"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let command = value["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.starts_with("'/opt/LokiMetis/loki_metis_gui'"));
    assert!(!command.contains("cmd.exe"));
    assert!(hook_restart_required(AiTool::WorkBuddy));
}

/// 全部 command Hook 的 WSL 生成使用 POSIX 可执行路径。
#[test]
fn every_wsl_command_hook_uses_its_posix_executable_and_config_path() {
    for tool in AiTool::ALL
        .into_iter()
        .filter(|tool| hook_supports_wsl(*tool))
    {
        let preview = super::generate_wsl_hook_config(
            tool,
            Path::new(r"C:\Program Files\LokiMetis\loki_metis_gui.exe"),
            "/mnt/c/Program Files/LokiMetis/loki_metis_gui.exe",
        )
        .unwrap();
        assert_eq!(preview.filename, protocol(tool).preview_filename());
        assert!(preview.content.contains(
            "'/mnt/c/Program Files/LokiMetis/loki_metis_gui.exe' --loki-metis-hook-relay"
        ));
        assert!(!preview.content.contains("cmd.exe"));
    }
}

#[test]
fn cursor_preview_uses_cursor_event_names_and_flat_shape() {
    let preview = generate_test_hook_config(AiTool::Cursor).unwrap();
    assert_eq!(preview.filename, ".cursor/hooks.json");
    for event in [
        "workspaceOpen",
        "beforeSubmitPrompt",
        "beforeShellExecution",
        "beforeMCPExecution",
        "afterFileEdit",
        "postToolUse",
        "subagentStart",
        "subagentStop",
        "afterAgentResponse",
        "sessionEnd",
    ] {
        assert!(preview.content.contains(&format!("\"{event}\"")));
    }
    assert!(!preview.content.contains("\"type\": \"command\""));
}

#[test]
fn standalone_plugin_files_are_branded_complete_and_idempotent() {
    let cases = [
        (AiTool::OpenCode, ".config/opencode/plugins/lokimetis.js", 0),
        (AiTool::Hermes, ".hermes/plugins/lokimetis/__init__.py", 1),
        (
            AiTool::OpenClaw,
            ".openclaw/extensions/lokimetis/index.mjs",
            2,
        ),
    ];
    for (tool, filename, auxiliary_count) in cases {
        let preview = generate_test_hook_config(tool).unwrap();
        assert_eq!(preview.filename, filename);
        assert!(preview.content.contains("LokiMetis:tool="));
        assert!(preview.content.contains("--loki-metis-hook-relay"));
        assert!(!preview.content.contains("loki-metis-hook-relay.json"));
        assert!(!preview.content.contains("127.0.0.1"));
        assert!(!preview.content.contains("/api/hooks/"));
        assert_eq!(generate_hook_auxiliary_configs(tool).len(), auxiliary_count);
        let merged = merge_hook_config(None, &preview, tool).unwrap();
        assert_eq!(merged.content, preview.content);
        let merged_again = merge_hook_config(Some(&merged.content), &preview, tool).unwrap();
        assert_eq!(merged_again.content, preview.content);
        assert!(merge_hook_config(Some("unrelated plugin"), &preview, tool).is_err());
    }
}

#[test]
fn command_hook_adapters_use_their_native_shapes_and_outcomes() {
    let cases = [
        (AiTool::CodeBuddy, ".codebuddy/settings.json", true),
        (AiTool::QwenCode, ".qwen/settings.json", true),
        (AiTool::Qoder, ".qoder/settings.json", false),
        (AiTool::GeminiCli, ".gemini/settings.json", true),
        (AiTool::GitHubCopilot, ".copilot/hooks/lokimetis.json", true),
        (AiTool::Grok, ".grok/hooks/lokimetis.json", true),
    ];
    for (tool, filename, restart_required) in cases {
        let preview = generate_test_hook_config(tool).unwrap();
        assert_eq!(preview.filename, filename);
        assert!(preview.content.contains("--loki-metis-hook-relay"));
        assert_eq!(hook_restart_required(tool), restart_required);
    }
    let copilot = generate_test_hook_config(AiTool::GitHubCopilot).unwrap();
    let value: Value = serde_json::from_str(&copilot.content).unwrap();
    assert_eq!(value["version"], 1);
    assert!(value["hooks"]["sessionStart"][0].get("hooks").is_none());
    let gemini = generate_test_hook_config(AiTool::GeminiCli).unwrap();
    let value: Value = serde_json::from_str(&gemini.content).unwrap();
    assert_eq!(
        value["hooks"]["Notification"][0]["matcher"],
        "ToolPermission"
    );
}

/// Grok 使用独立 LokiMetis 文件，与既有 AIMonitor Hook 配置并存。
#[test]
fn grok_preview_never_claims_the_aimonitor_hook_file() {
    let preview = generate_test_hook_config(AiTool::Grok).unwrap();
    assert_eq!(hook_config_filename(AiTool::Grok), "hooks/lokimetis.json");
    assert_eq!(preview.filename, ".grok/hooks/lokimetis.json");
    assert_ne!(preview.filename, ".grok/hooks/aimonitor.json");
    assert!(!preview.content.to_ascii_lowercase().contains("aimonitor"));
    assert!(preview.content.contains("LokiMetis:tool=grok"));

    let value: Value = serde_json::from_str(&preview.content).unwrap();
    assert!(value.get("hooks").is_some_and(Value::is_object));
    assert!(value.get("version").is_none());
}
