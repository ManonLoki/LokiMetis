//! 十四项 Agent 的 Hook 配置生成契约。

use std::path::Path;

use serde_json::Value;

use super::super::{
    MANAGED_HOOK_PREFIX, generate_hook_auxiliary_configs, hook_config_filename,
    hook_restart_required, hook_supports_wsl, managed_hook_marker, protocol, tool_from_slug,
};
use super::{command_has_marker, generate_hook_config, merge_hook_config};
use crate::agent_hooks::{AiTool, HookConfigPreview, HookError};

/// 断言实际配置的语义正文；Windows 编码载荷先解码，JSON 结构本身仍由原文解析。
pub(super) fn contract_text(preview: &HookConfigPreview) -> String {
    let mut content = preview.content.clone();
    if let Ok(value) = serde_json::from_str::<Value>(&preview.content) {
        append_commands(&value, &mut content);
    }
    content
}
/// 遍历原始配置，不替换可执行命令或把解码后的脚本当作生成配置。
fn append_commands(value: &Value, content: &mut String) {
    match value {
        Value::String(command) => { content.push_str(&command_text(command)); },
        Value::Array(values) => { for value in values { append_commands(value, content); } },
        Value::Object(values) => { for value in values.values() { append_commands(value, content); } },
        _ => {},
    }
}
/// 仅测试解码固定 EncodedCommand 参数，以断言事件、slug 和管理标识原值。
pub(super) fn command_text(command: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let Some(encoded) = command.strip_prefix(super::windows_command::PREFIX) else { return command.into() };
    let bytes = STANDARD.decode(encoded).expect("编码命令必须为 base64");
    assert_eq!(bytes.len() % 2, 0);
    String::from_utf16(&bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>()).unwrap()
}

/// 用固定示例路径生成配置，供本文件与合并测试复用。
pub(super) fn generate_test_hook_config(tool: AiTool) -> Result<HookConfigPreview, HookError> {
    generate_hook_config(tool, Path::new("/opt/LokiMetris/loki_metis_gui"))
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
        assert_eq!(marker, format!("LokiMetris:tool={slug}"));
        assert!(contract_text(&preview).contains(&marker));
        assert!(!contract_text(&preview).contains("LokiMetris|tool="));
        for event in protocol.events() {
            assert!(
                contract_text(&preview).contains(event.name),
                "{} 的生成配置缺少协议事件 {}",
                protocol.name(),
                event.name
            );
        }
        assert!(contract_text(&preview).contains("--loki-metris-hook-relay"));
        assert!(contract_text(&preview).contains("--managed-by"));
        if protocol.uses_standalone_plugin() {
            assert!(contract_text(&preview).contains("/opt/LokiMetris/loki_metis_gui"));
            assert!(contract_text(&preview).contains("hook_event_name"));
            assert!(contract_text(&preview).contains("session_id"));
            assert!(contract_text(&preview).contains("status"));
            assert!(
                generate_hook_auxiliary_configs(tool)
                    .iter()
                    .all(|file| file.content.contains(&marker))
            );
        } else {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            assert!(contract_text(&preview).contains(&format!("'{slug}'")));
        }
    }
}

/// Claude Code 覆盖权限与生命周期事件。
#[test]
fn claude_preview_covers_permission_and_lifecycle_events() {
    let preview = generate_test_hook_config(AiTool::ClaudeCode).unwrap();
    assert_eq!(preview.filename, ".claude/settings.json");
    assert!(contract_text(&preview).contains("\"SessionStart\""));
    assert!(contract_text(&preview).contains("\"PermissionRequest\""));
    assert!(contract_text(&preview).contains("\"PermissionDenied\""));
    assert!(contract_text(&preview).contains("LokiMetris:tool=claude-code"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
    assert_eq!(
        value["hooks"]["Notification"][1]["matcher"],
        "permission_prompt"
    );
    assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
}

/// 三种客户端的同名通知分组都必须写出，且分别转发至受管 relay。
#[test]
fn claude_compatible_notification_matchers_are_both_generated() {
    for tool in [AiTool::ClaudeCode, AiTool::WorkBuddy, AiTool::CodeBuddy] {
        let preview = generate_test_hook_config(tool).unwrap();
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        let groups = value["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(groups.len(), match tool { AiTool::ClaudeCode => 5, AiTool::WorkBuddy => 3, _ => 2 }, "{tool:?}");
        assert_eq!(groups[0]["matcher"], "idle_prompt", "{tool:?}");
        assert_eq!(groups[1]["matcher"], "permission_prompt", "{tool:?}");
        for group in groups {
            assert!(
                command_text(group["hooks"][0]["command"]
                    .as_str()
                    .unwrap())
                    .contains("--loki-metris-hook-relay")
            );
        }
    }
}

/// Codex 使用 PascalCase 嵌套 handler 且 SessionEnd 带超时。
#[test]
fn codex_preview_uses_pascal_case_and_nested_handlers() {
    let preview = generate_test_hook_config(AiTool::Codex).unwrap();
    assert_eq!(preview.filename, ".codex/hooks.json");
    assert!(contract_text(&preview).contains("\"SessionStart\""));
    assert!(contract_text(&preview).contains("\"UserPromptSubmit\""));
    assert!(contract_text(&preview).contains("LokiMetris:tool=codex"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command_text(session_end).contains("--loki-metris-hook-relay"));
    assert!(command_text(session_end).contains("SessionEnd"));
    assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
    let command = value["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command_text(command).contains(MANAGED_HOOK_PREFIX));
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
    assert!(contract_text(&preview).contains("LokiMetris:tool=workbuddy"));
    let value: Value = serde_json::from_str(&preview.content).unwrap();
    let command = value["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.starts_with("'/opt/LokiMetris/loki_metis_gui'"));
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
            Path::new(r"C:\Program Files\LokiMetris\loki_metis_gui.exe"),
            "/mnt/c/Program Files/LokiMetris/loki_metis_gui.exe",
        )
        .unwrap();
        assert_eq!(preview.filename, protocol(tool).preview_filename());
        assert!(contract_text(&preview).contains(
            "'/mnt/c/Program Files/LokiMetris/loki_metis_gui.exe' --loki-metris-hook-relay"
        ));
        assert!(!contract_text(&preview).contains("cmd.exe"));
    }
}

#[test]
/// 验证 Cursor 预览使用其原生事件名与扁平命令结构。
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
        assert!(contract_text(&preview).contains(&format!("\"{event}\"")));
    }
    assert!(!contract_text(&preview).contains("\"type\": \"command\""));
}

#[test]
/// 验证独立插件文件具有完整品牌标识并可重复合并。
fn standalone_plugin_files_are_branded_complete_and_idempotent() {
    let cases = [
        (AiTool::OpenCode, ".config/opencode/plugins/lokimetris.js", 0),
        (AiTool::Hermes, ".hermes/plugins/lokimetris/__init__.py", 1),
        (
            AiTool::OpenClaw,
            ".openclaw/extensions/lokimetris/index.mjs",
            2,
        ),
    ];
    for (tool, filename, auxiliary_count) in cases {
        let preview = generate_test_hook_config(tool).unwrap();
        assert_eq!(preview.filename, filename);
        assert!(contract_text(&preview).contains("LokiMetris:tool="));
        assert!(contract_text(&preview).contains("--loki-metris-hook-relay"));
        assert!(!contract_text(&preview).contains("loki-metris-hook-relay.json"));
        assert!(!contract_text(&preview).contains("127.0.0.1"));
        assert!(!contract_text(&preview).contains("/api/hooks/"));
        assert_eq!(generate_hook_auxiliary_configs(tool).len(), auxiliary_count);
        let merged = merge_hook_config(None, &preview, tool).unwrap();
        assert_eq!(merged.content, preview.content);
        let merged_again = merge_hook_config(Some(&merged.content), &preview, tool).unwrap();
        assert_eq!(merged_again.content, preview.content);
        assert!(merge_hook_config(Some("unrelated plugin"), &preview, tool).is_err());
    }
}

#[test]
/// 验证命令型适配器输出各自原生结构并声明正确写入结果。
fn command_hook_adapters_use_their_native_shapes_and_outcomes() {
    let cases = [
        (AiTool::CodeBuddy, ".codebuddy/settings.json", true),
        (AiTool::QwenCode, ".qwen/settings.json", true),
        (AiTool::Qoder, ".qoder/settings.json", false),
        (AiTool::GeminiCli, ".gemini/settings.json", true),
        (AiTool::GitHubCopilot, ".copilot/hooks/lokimetris.json", true),
        (AiTool::Grok, ".grok/hooks/lokimetris.json", true),
    ];
    for (tool, filename, restart_required) in cases {
        let preview = generate_test_hook_config(tool).unwrap();
        assert_eq!(preview.filename, filename);
        assert!(contract_text(&preview).contains("--loki-metris-hook-relay"));
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

/// Grok 使用独立 LokiMetris 文件，与既有 AIMonitor Hook 配置并存。
#[test]
fn grok_preview_never_claims_the_aimonitor_hook_file() {
    let preview = generate_test_hook_config(AiTool::Grok).unwrap();
    assert_eq!(hook_config_filename(AiTool::Grok), "hooks/lokimetris.json");
    assert_eq!(preview.filename, ".grok/hooks/lokimetris.json");
    assert_ne!(preview.filename, ".grok/hooks/aimonitor.json");
    assert!(!preview.content.to_ascii_lowercase().contains("aimonitor"));
    assert!(contract_text(&preview).contains("LokiMetris:tool=grok"));

    let value: Value = serde_json::from_str(&preview.content).unwrap();
    assert!(value.get("hooks").is_some_and(Value::is_object));
    assert!(value.get("version").is_none());
}
