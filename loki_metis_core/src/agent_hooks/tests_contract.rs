//! 四项 Agent 的目录、写入结果与 slug 契约。

use super::{
    ai_tool_descriptors, hook_changed_write_outcome, hook_requires_review, hook_restart_required,
    protocol,
};
use crate::agent_hooks::{AiTool, HookWriteOutcome};

const TOOL_CONTRACTS: [(AiTool, &str, HookWriteOutcome); 4] = [
    (AiTool::Codex, "Codex", HookWriteOutcome::CodexReviewRequired),
    (AiTool::ClaudeCode, "Claude Code", HookWriteOutcome::Active),
    (AiTool::Grok, "Grok Build", HookWriteOutcome::RestartRequired),
    (
        AiTool::WorkBuddy,
        "WorkBuddy",
        HookWriteOutcome::WorkBuddyReviewRequired,
    ),
];

/// 工具目录与写入结果完全由协议实现驱动，且覆盖全部四项 Agent。
#[test]
fn tool_catalog_and_write_outcomes_are_protocol_owned_and_complete() {
    let descriptors = ai_tool_descriptors();
    assert_eq!(descriptors.len(), AiTool::ALL.len());
    for (expected_tool, expected_name, expected_outcome) in TOOL_CONTRACTS {
        let descriptor = descriptors
            .iter()
            .find(|candidate| candidate.tool == expected_tool)
            .unwrap_or_else(|| panic!("missing catalog entry for {expected_tool:?}"));
        assert_eq!(descriptor.name, expected_name);
        assert_eq!(protocol(expected_tool).tool(), expected_tool);
        assert_eq!(hook_changed_write_outcome(expected_tool), expected_outcome);
        assert_eq!(
            hook_requires_review(expected_tool),
            expected_outcome.requires_review()
        );
        assert_eq!(
            hook_restart_required(expected_tool),
            expected_outcome.restart_required()
        );
    }
}

/// 展示目录按名称升序，且不含未批准工具。
#[test]
fn settings_ai_client_catalog_is_sorted_by_display_name_ascending() {
    let names: Vec<_> = ai_tool_descriptors()
        .into_iter()
        .map(|item| item.name)
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert!(!names.iter().any(|name| name.contains("Cursor")
        || name.contains("OpenCode")
        || name.contains("Hermes")));
}

/// 未批准 slug 不得解析为四项 Agent。
#[test]
fn unapproved_slugs_are_rejected() {
    assert!(super::tool_from_slug("cursor").is_none());
    assert!(super::tool_from_slug("opencode").is_none());
    assert_eq!(super::tool_from_slug("codex"), Some(AiTool::Codex));
    assert_eq!(super::tool_from_slug("claude-code"), Some(AiTool::ClaudeCode));
    assert_eq!(super::tool_from_slug("grok"), Some(AiTool::Grok));
    assert_eq!(super::tool_from_slug("workbuddy"), Some(AiTool::WorkBuddy));
}
