//! 四项 Agent 的 Hook 配置生成契约。

use std::path::Path;

use serde_json::Value;

use super::super::{
    MANAGED_HOOK_PREFIX, hook_restart_required, managed_hook_marker, protocol, tool_from_slug,
};
use super::{command_has_marker, generate_hook_config};
use crate::agent_hooks::{AiTool, HookConfigPreview, HookError};

/// 用固定示例路径生成配置，供本文件与合并测试复用。
pub(super) fn generate_test_hook_config(tool: AiTool) -> Result<HookConfigPreview, HookError> {
    generate_hook_config(tool, Path::new("/opt/LokiMetis/loki_metis_gui"))
}

/// 四项 Agent 生成配置都使用规范 slug、事件名与管理标识。
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
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(preview.content.contains(&format!("'{slug}'")));
        assert!(preview.content.contains("--managed-by"));
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
    assert!(command_has_marker(command, &managed_hook_marker(AiTool::Codex)));
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

/// 四项 command Hook 的 WSL 生成使用 POSIX 可执行路径。
#[test]
fn every_wsl_command_hook_uses_its_posix_executable_and_config_path() {
    let cases = [
        (AiTool::Codex, ".codex/hooks.json"),
        (AiTool::ClaudeCode, ".claude/settings.json"),
        (AiTool::Grok, ".grok/hooks/aimonitor.json"),
        (AiTool::WorkBuddy, ".workbuddy/settings.json"),
    ];
    for (tool, expected_filename) in cases {
        let preview = super::generate_wsl_hook_config(
            tool,
            Path::new(r"C:\Program Files\LokiMetis\loki_metis_gui.exe"),
            "/mnt/c/Program Files/LokiMetis/loki_metis_gui.exe",
        )
        .unwrap();
        assert_eq!(preview.filename, expected_filename);
        assert!(preview.content.contains(
            "'/mnt/c/Program Files/LokiMetis/loki_metis_gui.exe' --loki-metis-hook-relay"
        ));
        assert!(!preview.content.contains("cmd.exe"));
    }
}
