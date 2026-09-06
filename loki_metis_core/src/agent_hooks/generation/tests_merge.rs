//! 全部受支持 AI 工具的 Hook 配置合并、迁移与标识识别。

use serde_json::Value;

use super::super::{hook_transition, managed_hook_marker};
use super::{command_has_marker, tests::generate_test_hook_config};
use crate::agent_hooks::{
    AiTool, HookBehavior, HookConfigPreview, HookTransition, merge_hook_config,
    remove_managed_hook_entries,
};

/// Codex 合并幂等，并保留用户其它命令。
#[test]
fn codex_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Codex).unwrap();
    let first = merge_hook_config(None, &generated, AiTool::Codex).unwrap();
    let mut value: Value = serde_json::from_str(&first.content).unwrap();
    value["permissions"] = serde_json::json!({ "allow": ["Bash"] });
    value["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "hooks": [{ "type": "command", "command": "other-app notify" }]
        }));
    let existing = serde_json::to_string_pretty(&value).unwrap();
    let merged = merge_hook_config(Some(&existing), &generated, AiTool::Codex).unwrap();
    let value: Value = serde_json::from_str(&merged.content).unwrap();
    let stop = value["hooks"]["Stop"].as_array().unwrap();
    let serialized = serde_json::to_string(stop).unwrap();
    assert_eq!(serialized.matches("other-app notify").count(), 1);
    assert_eq!(
        serialized
            .matches(&managed_hook_marker(AiTool::Codex))
            .count(),
        1
    );
    assert_eq!(value["permissions"]["allow"][0], "Bash");
}

/// 只有完整管理标识才会被识别。
#[test]
fn only_the_canonical_managed_marker_is_recognized() {
    assert!(command_has_marker(
        "'LokiMetis' --managed-by 'LokiMetis:tool=codex'",
        "LokiMetis:tool=codex"
    ));
    assert!(!command_has_marker(
        ": 'LokiMetis|tool=codex'; curl current",
        "LokiMetis:tool=codex"
    ));
}

/// 非法现有配置应拒绝合并。
#[test]
fn merge_rejects_an_invalid_existing_config() {
    let generated = HookConfigPreview {
        filename: ".codex/hooks.json".to_owned(),
        content: r#"{"hooks":{}}"#.to_owned(),
    };
    assert!(merge_hook_config(Some(r#"{"hooks":[]}"#), &generated, AiTool::Codex).is_err());
}

/// 生成命令只调用本机 relay，不含局域网地址或 curl。
#[test]
fn hook_commands_use_the_embedded_minimizing_relay() {
    let preview = generate_test_hook_config(AiTool::Codex).unwrap();
    assert!(preview.content.contains("--loki-metis-hook-relay"));
    assert!(preview.content.contains("SessionStart"));
    assert!(!preview.content.contains("Invoke-RestMethod"));
    assert!(!preview.content.contains("curl"));
    assert!(!preview.content.contains("mdns"));
    assert!(!preview.content.contains("192.168.1.100:8080"));
}

/// 生成配置是纯函数，不依赖展示内容。
#[test]
fn hook_config_is_identical_when_display_content_changes() {
    assert_eq!(
        generate_test_hook_config(AiTool::Codex).unwrap().content,
        generate_test_hook_config(AiTool::Codex).unwrap().content
    );
}

/// Grok 合并幂等并保留用户其它命令。
#[test]
fn grok_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Grok).unwrap();
    let existing = r#"{
      "hooks": {
        "SessionStart": [
          { "hooks": [{ "type": "command", "command": "other-session-start" }]}
        ],
        "Stop": [
          { "hooks": [{ "type": "command", "command": "other-stop" }]}
        ]
      }
    }"#;
    let first = merge_hook_config(Some(existing), &generated, AiTool::Grok).unwrap();
    let value: Value = serde_json::from_str(&first.content).unwrap();
    let stop = serde_json::to_string(&value["hooks"]["Stop"]).unwrap();
    assert_eq!(stop.matches("other-stop").count(), 1);
    assert_eq!(stop.matches("LokiMetis:tool=grok").count(), 1);
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::Grok).unwrap();
    assert_eq!(first.content, second.content);
}

/// 旧 Grok 文件迁移仅移除 LokiMetis Grok handler，并保留用户、AIMonitor 与根内容。
#[test]
fn grok_managed_entry_cleanup_preserves_mixed_and_root_content_idempotently() {
    let existing = r#"{
      "owner": "user",
      "hooks": {
        "SessionStart": [
          { "hooks": [
            { "type": "command", "command": "user-session-start" },
            { "type": "command", "command": "legacy AIMonitor session start" },
            { "type": "command", "command": "'LokiMetis' --loki-metis-hook-relay grok SessionStart --managed-by 'LokiMetis:tool=grok'" }
          ]},
          { "hooks": [
            { "type": "command", "command": "'LokiMetis' --loki-metis-hook-relay grok SessionStart --managed-by 'LokiMetis:tool=grok'" }
          ]}
        ],
        "Custom": [
          { "hooks": [
            { "type": "command", "command": "'LokiMetis' --loki-metis-hook-relay codex Stop --managed-by 'LokiMetis:tool=codex'" }
          ]}
        ]
      }
    }"#;
    let cleaned = remove_managed_hook_entries(AiTool::Grok, existing)
        .unwrap()
        .expect("mixed config remains");
    assert!(!cleaned.contains("LokiMetis:tool=grok"));
    assert!(cleaned.contains("user-session-start"));
    assert!(cleaned.contains("legacy AIMonitor session start"));
    assert!(cleaned.contains("LokiMetis:tool=codex"));
    let value: Value = serde_json::from_str(&cleaned).unwrap();
    assert_eq!(value["owner"], "user");
    assert_eq!(
        remove_managed_hook_entries(AiTool::Grok, &cleaned).unwrap(),
        Some(cleaned)
    );
}

/// 无受管条目时逐字不变；只有受管 hooks 时为空，额外根字段则必须保留。
#[test]
fn grok_managed_entry_cleanup_has_explicit_empty_and_unchanged_semantics() {
    let unmanaged =
        " {\n  \"hooks\": {\"Stop\": [{\"hooks\": [{\"command\": \"user-stop\"}]}]}\n}\n";
    assert_eq!(
        remove_managed_hook_entries(AiTool::Grok, unmanaged).unwrap(),
        Some(unmanaged.to_owned())
    );

    let managed_group = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"'LokiMetis' --loki-metis-hook-relay grok Stop --managed-by 'LokiMetis:tool=grok'"}]}]}}"#;
    assert_eq!(
        remove_managed_hook_entries(AiTool::Grok, managed_group).unwrap(),
        None
    );

    let with_root_content = r#"{
      "keep": true,
      "hooks": {
        "Stop": [{
          "hooks": [{
            "type": "command",
            "command": "'LokiMetis' --loki-metis-hook-relay grok Stop --managed-by 'LokiMetis:tool=grok'"
          }]
        }]
      }
    }"#;
    let cleaned = remove_managed_hook_entries(AiTool::Grok, with_root_content)
        .unwrap()
        .expect("root content keeps document");
    let value: Value = serde_json::from_str(&cleaned).unwrap();
    assert_eq!(value["keep"], true);
    assert!(value["hooks"].as_object().unwrap().is_empty());
}

#[test]
fn cursor_merge_is_idempotent_and_preserves_other_commands() {
    let generated = generate_test_hook_config(AiTool::Cursor).unwrap();
    let existing = r#"{
      "version": 1,
      "hooks": {
        "stop": [
          { "command": "other-app stop" },
          { "command": "'LokiMetis' --loki-metis-hook-relay cursor stop --managed-by 'LokiMetis:tool=cursor'" }
        ]
      }
    }"#;
    let first = merge_hook_config(Some(existing), &generated, AiTool::Cursor).unwrap();
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::Cursor).unwrap();
    let value: Value = serde_json::from_str(&second.content).unwrap();
    let stop = serde_json::to_string(&value["hooks"]["stop"]).unwrap();
    assert_eq!(stop.matches("other-app stop").count(), 1);
    assert_eq!(stop.matches("LokiMetis:tool=cursor").count(), 1);
}

/// 验证 Claude 分组型协议保留用户条目且重复合并幂等。
fn assert_grouped_merge_is_idempotent(tool: AiTool, event: &str, other_command: &str) {
    let generated = generate_test_hook_config(tool).unwrap();
    let existing = serde_json::json!({ "hooks": {} });
    let mut existing = existing;
    existing["hooks"][event] = serde_json::json!([
        { "hooks": [{ "type": "command", "command": other_command }] }
    ]);
    let first = merge_hook_config(
        Some(&serde_json::to_string_pretty(&existing).unwrap()),
        &generated,
        tool,
    )
    .unwrap();
    let second = merge_hook_config(Some(&first.content), &generated, tool).unwrap();
    assert_eq!(first.content, second.content);
    let value: Value = serde_json::from_str(&second.content).unwrap();
    let serialized = serde_json::to_string(&value["hooks"][event]).unwrap();
    assert_eq!(serialized.matches(other_command).count(), 1);
    assert_eq!(serialized.matches(&managed_hook_marker(tool)).count(), 1);
}

#[test]
fn grouped_phase_two_merges_are_idempotent_and_preserve_user_commands() {
    assert_grouped_merge_is_idempotent(AiTool::QwenCode, "Stop", "other-stop");
    assert_grouped_merge_is_idempotent(AiTool::Qoder, "PostToolUse", "other-post-tool");
    assert_grouped_merge_is_idempotent(AiTool::GeminiCli, "SessionEnd", "other-session-end");
}

#[test]
fn github_copilot_merge_is_idempotent_and_preserves_other_handlers() {
    let generated = generate_test_hook_config(AiTool::GitHubCopilot).unwrap();
    let existing = r#"{
      "version": 99,
      "hooks": {
        "userPromptSubmitted": [
          { "type": "command", "command": "other-user-prompt" },
          { "type": "command", "command": "'LokiMetis' --loki-metis-hook-relay github-copilot userPromptSubmitted --managed-by 'LokiMetis:tool=github-copilot'" }
        ]
      }
    }"#;
    let first = merge_hook_config(Some(existing), &generated, AiTool::GitHubCopilot).unwrap();
    let second =
        merge_hook_config(Some(&first.content), &generated, AiTool::GitHubCopilot).unwrap();
    assert_eq!(first.content, second.content);
    let value: Value = serde_json::from_str(&second.content).unwrap();
    let prompt = serde_json::to_string(&value["hooks"]["userPromptSubmitted"]).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(prompt.matches("other-user-prompt").count(), 1);
    assert_eq!(prompt.matches("LokiMetis:tool=github-copilot").count(), 1);
}

#[test]
fn hook_transitions_are_owned_by_each_tool_protocol() {
    assert_eq!(
        hook_transition(AiTool::ClaudeCode, "Notification"),
        Some(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        hook_transition(AiTool::Codex, "PermissionRequest"),
        Some(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        hook_transition(AiTool::Cursor, "sessionEnd"),
        Some(HookTransition::Release)
    );
    assert_eq!(
        hook_transition(AiTool::OpenCode, "session.busy"),
        Some(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(hook_transition(AiTool::Codex, "Unknown"), None);
}
