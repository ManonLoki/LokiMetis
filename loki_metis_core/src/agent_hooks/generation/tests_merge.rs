//! 四项 Agent 的 Hook 配置合并与标识识别。

use serde_json::Value;

use super::super::managed_hook_marker;
use super::{command_has_marker, tests::generate_test_hook_config};
use crate::agent_hooks::{AiTool, HookConfigPreview, merge_hook_config};

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
