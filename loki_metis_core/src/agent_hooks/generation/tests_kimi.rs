//! Kimi Code TOML 生成、合并与状态机回归。

use std::time::Duration;

use serde_json::{Map, Value, json};

use super::{merge_hook_config, tests::generate_test_hook_config};
use crate::agent_hooks::{
    AiTool, HookBehavior, HookEventDecision, HookStateMachine, HookTransition,
};

#[test]
fn kimi_code_preview_uses_managed_toml_rules() {
    let preview = generate_test_hook_config(AiTool::KimiCode).unwrap();
    let parsed = toml::from_str::<toml::Value>(&preview.content).unwrap();
    assert_eq!(preview.filename, ".kimi-code/config.toml");
    assert!(preview.content.contains("LokiMetis:tool=kimi-code begin"));
    for event in [
        "SessionStart",
        "PermissionRequest",
        "Interrupt",
        "SessionEnd",
    ] {
        assert!(preview.content.contains(&format!("event = \"{event}\"")));
    }
    assert_eq!(preview.content.matches("[[hooks]]").count(), 14);
    assert!(
        preview
            .content
            .contains("--loki-metis-hook-relay 'kimi-code'")
    );
    assert!(!preview.content.contains("cmd.exe"));
    assert_eq!(
        parsed
            .get("hooks")
            .and_then(toml::Value::as_array)
            .map(Vec::len),
        Some(14)
    );
}

#[test]
fn kimi_code_toml_serializer_round_trips_special_characters() {
    use super::super::{HookProtocol, kimi_code::KIMI_CODE};

    let special_command = "printf '\"quoted\" \\ slash\nnext\tcolumn\u{0007}雪'";
    let special_matcher = "tool-\"name\"\\path\nnext\t\u{0001}雪";
    let mut hooks = Map::new();
    for event in KIMI_CODE.events() {
        let mut handler = json!({ "command": special_command });
        if event.name == "SessionStart" {
            handler["matcher"] = Value::String(special_matcher.to_owned());
        }
        hooks.insert(event.name.to_owned(), handler);
    }
    let rendered = KIMI_CODE.render_config(hooks).unwrap();
    let parsed = toml::from_str::<toml::Value>(&rendered).unwrap();
    let session_start = parsed
        .get("hooks")
        .and_then(toml::Value::as_array)
        .unwrap()
        .iter()
        .find(|hook| hook.get("event").and_then(toml::Value::as_str) == Some("SessionStart"))
        .unwrap();
    assert_eq!(
        session_start.get("command").and_then(toml::Value::as_str),
        Some(special_command)
    );
    assert_eq!(
        session_start.get("matcher").and_then(toml::Value::as_str),
        Some(special_matcher)
    );
}

#[test]
fn kimi_code_merge_preserves_user_toml_and_is_idempotent() {
    let generated = generate_test_hook_config(AiTool::KimiCode).unwrap();
    let existing = "default_model = \"kimi-code/k3\"\n\n[background]\nkeep_alive_on_exit = true\n";
    let first = merge_hook_config(Some(existing), &generated, AiTool::KimiCode).unwrap();
    assert!(first.content.starts_with(existing));
    assert_eq!(
        first
            .content
            .matches("LokiMetis:tool=kimi-code begin")
            .count(),
        1
    );
    let second = merge_hook_config(Some(&first.content), &generated, AiTool::KimiCode).unwrap();
    assert_eq!(first.content, second.content);
}

#[test]
fn kimi_code_merge_rejects_a_broken_managed_block() {
    let generated = generate_test_hook_config(AiTool::KimiCode).unwrap();
    let broken = "default_model = \"kimi-code/k3\"\n# LokiMetis:tool=kimi-code begin\n";
    assert!(merge_hook_config(Some(broken), &generated, AiTool::KimiCode).is_err());
}

#[test]
fn kimi_code_state_machine_covers_permission_failure_stop_and_release() {
    let mut machine = HookStateMachine::default();
    let mut apply = |event: &str| {
        machine.apply_event_with_status_at(
            AiTool::KimiCode,
            event,
            Some("kimi"),
            None,
            None,
            Duration::ZERO,
        )
    };
    assert_eq!(
        apply("SessionStart"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        apply("UserPromptSubmit"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        apply("PermissionRequest"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    assert_eq!(
        apply("PostToolUseFailure"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    assert_eq!(
        apply("Interrupt"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        apply("SessionEnd"),
        HookEventDecision::Forward(HookTransition::Release)
    );
}
