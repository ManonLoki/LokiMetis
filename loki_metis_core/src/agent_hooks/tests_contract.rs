//! 十四项 AI 工具的目录、协议分发、写入结果与 slug 契约。

use std::time::Duration;

use super::{
    ai_tool_descriptors, generate_hook_auxiliary_configs, generate_hook_config,
    hook_changed_write_outcome, hook_config_write_result, hook_requires_review,
    hook_restart_required, hook_supports_wsl, managed_hook_marker, protocol, release_settle_delay,
    session_start_revives_tombstone, tool_from_slug,
};
use crate::agent_hooks::{AiTool, HookWriteOutcome};

/// 各 adapter 唯一拥有的目录契约。
const TOOL_CONTRACTS: [(AiTool, &str, &str, HookWriteOutcome); 14] = [
    (
        AiTool::Codex,
        "Codex",
        "codex",
        HookWriteOutcome::CodexReviewRequired,
    ),
    (
        AiTool::ClaudeCode,
        "Claude Code",
        "claude-code",
        HookWriteOutcome::Active,
    ),
    (AiTool::Cursor, "Cursor", "cursor", HookWriteOutcome::Active),
    (
        AiTool::OpenCode,
        "OpenCode",
        "opencode",
        HookWriteOutcome::Active,
    ),
    (
        AiTool::WorkBuddy,
        "WorkBuddy",
        "workbuddy",
        HookWriteOutcome::WorkBuddyReviewRequired,
    ),
    (
        AiTool::Hermes,
        "Hermes",
        "hermes",
        HookWriteOutcome::HermesEnableRequired,
    ),
    (
        AiTool::OpenClaw,
        "OpenClaw",
        "openclaw",
        HookWriteOutcome::OpenClawEnableRequired,
    ),
    (
        AiTool::CodeBuddy,
        "CodeBuddy",
        "codebuddy",
        HookWriteOutcome::CodeBuddyReviewRequired,
    ),
    (
        AiTool::QwenCode,
        "Qwen Code",
        "qwen-code",
        HookWriteOutcome::RestartRequired,
    ),
    (
        AiTool::KimiCode,
        "Kimi Code",
        "kimi-code",
        HookWriteOutcome::RestartRequired,
    ),
    (AiTool::Qoder, "Qoder", "qoder", HookWriteOutcome::Active),
    (
        AiTool::GeminiCli,
        "Gemini CLI",
        "gemini-cli",
        HookWriteOutcome::RestartRequired,
    ),
    (
        AiTool::GitHubCopilot,
        "GitHub Copilot CLI",
        "github-copilot",
        HookWriteOutcome::RestartRequired,
    ),
    (
        AiTool::Grok,
        "Grok Build",
        "grok",
        HookWriteOutcome::RestartRequired,
    ),
];

#[test]
/// 验证工具目录、slug 与写入结果完整且由各协议唯一声明。
fn tool_catalog_slugs_and_write_outcomes_are_protocol_owned_and_complete() {
    let descriptors = ai_tool_descriptors();
    assert_eq!(descriptors.len(), AiTool::ALL.len());
    for (tool, expected_name, slug, expected_outcome) in TOOL_CONTRACTS {
        let descriptor = descriptors
            .iter()
            .find(|candidate| candidate.tool == tool)
            .unwrap_or_else(|| panic!("missing catalog entry for {tool:?}"));
        assert_eq!(descriptor.name, expected_name);
        assert_eq!(protocol(tool).tool(), tool);
        assert_eq!(tool_from_slug(slug), Some(tool));
        assert_eq!(hook_changed_write_outcome(tool), expected_outcome);
        assert_eq!(
            hook_requires_review(tool),
            expected_outcome.requires_review()
        );
        assert_eq!(
            hook_restart_required(tool),
            expected_outcome.restart_required()
        );

        let changed = hook_config_write_result(tool, "config".to_owned(), true);
        assert_eq!(changed.outcome, expected_outcome);
        assert!(changed.config_changed);
        let unchanged = hook_config_write_result(tool, "config".to_owned(), false);
        assert_eq!(unchanged.outcome, HookWriteOutcome::Unchanged);
        assert!(!unchanged.config_changed);
        assert!(!unchanged.requires_review);
        assert!(!unchanged.restart_required);
    }
}

#[test]
/// 验证设置页 AI 客户端目录按展示名称升序排列。
fn settings_ai_client_catalog_is_sorted_by_display_name_ascending() {
    let names = ai_tool_descriptors()
        .into_iter()
        .map(|descriptor| descriptor.name)
        .collect::<Vec<_>>();
    assert_eq!(names.len(), AiTool::ALL.len());
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
/// 验证只有独立插件协议会被排除在 WSL 命令生成之外。
fn only_standalone_plugins_are_excluded_from_wsl_command_generation() {
    for tool in AiTool::ALL {
        assert_eq!(
            hook_supports_wsl(tool),
            !matches!(tool, AiTool::OpenCode | AiTool::Hermes | AiTool::OpenClaw),
            "{tool:?}"
        );
    }
}

#[test]
/// 验证附加文件仅由确实包含多文件的独立插件声明。
fn auxiliary_files_are_declared_only_by_multi_file_plugins() {
    assert_eq!(generate_hook_auxiliary_configs(AiTool::Hermes).len(), 1);
    assert_eq!(generate_hook_auxiliary_configs(AiTool::OpenClaw).len(), 2);
    for tool in AiTool::ALL {
        if !matches!(tool, AiTool::Hermes | AiTool::OpenClaw) {
            assert!(generate_hook_auxiliary_configs(tool).is_empty(), "{tool:?}");
        }
    }
}

#[test]
/// 验证独立插件只通过受管 CLI relay 转发事件。
fn standalone_plugins_forward_only_through_the_cli_relay() {
    let executable = std::path::Path::new("/opt/LokiMetis/loki_metis_gui");
    for tool in [AiTool::OpenCode, AiTool::Hermes, AiTool::OpenClaw] {
        let preview = generate_hook_config(tool, executable).unwrap();
        assert!(preview.content.contains("/opt/LokiMetis/loki_metis_gui"));
        assert!(preview.content.contains("--loki-metis-hook-relay"));
        assert!(preview.content.contains("--managed-by"));
        assert!(preview.content.contains(&managed_hook_marker(tool)));
        assert!(preview.content.contains("hook_event_name"));
        assert!(preview.content.contains("session_id"));
        assert!(preview.content.contains("status"));
        assert!(!preview.content.contains("loki-metis-hook-relay.json"));
        assert!(!preview.content.contains("127.0.0.1"));
        assert!(!preview.content.contains("/api/hooks/"));
        assert!(!preview.content.contains("node:http"));
        assert!(!preview.content.contains("urllib.request"));
        if tool == AiTool::Hermes {
            assert!(preview.content.contains("import subprocess"));
            assert!(preview.content.contains("subprocess.run("));
            assert!(preview.content.contains("input=payload"));
            assert!(preview.content.contains("text=True"));
            assert!(preview.content.contains("timeout=4"));
            assert!(!preview.content.contains("shell=True"));
        } else {
            assert!(preview.content.contains("from \"node:child_process\""));
            assert!(preview.content.contains("spawn(relayExecutable"));
            assert!(
                preview
                    .content
                    .contains("stdio: [\"pipe\", \"ignore\", \"ignore\"]")
            );
            assert!(preview.content.contains("windowsHide: true"));
            assert!(preview.content.contains("let settled = false"));
            assert!(preview.content.contains("deadline = setTimeout"));
            assert!(preview.content.contains("}, 4000)"));
            assert!(preview.content.contains("clearTimeout(deadline)"));
            assert!(preview.content.contains("relay.kill()"));
            assert!(preview.content.contains("relay.stdin.end(body, \"utf8\")"));
            assert!(!preview.content.contains("shell: true"));
        }
    }
}

#[test]
/// 验证仅 Cursor 声明延迟释放与墓碑复活限制。
fn cursor_alone_declares_release_handoff_and_tombstone_rules() {
    assert_eq!(
        release_settle_delay(AiTool::Cursor),
        Duration::from_millis(250)
    );
    assert!(!session_start_revives_tombstone(AiTool::Cursor));
    for tool in AiTool::ALL {
        if tool != AiTool::Cursor {
            assert_eq!(release_settle_delay(tool), Duration::ZERO, "{tool:?}");
            assert!(session_start_revives_tombstone(tool), "{tool:?}");
        }
    }
}

#[test]
/// 验证每个工具适配器暴露完整且精确的原生事件表。
fn every_adapter_exposes_the_complete_source_event_table() {
    let contracts: [(AiTool, &[&str]); 14] = [
        (
            AiTool::Codex,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PermissionRequest",
                "Stop",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "SessionEnd",
            ],
        ),
        (
            AiTool::ClaudeCode,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PermissionRequest",
                "Elicitation",
                "PostToolUseFailure",
                "Stop",
                "StopFailure",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
                "SessionEnd",
            ],
        ),
        (
            AiTool::Cursor,
            &[
                "workspaceOpen",
                "sessionStart",
                "beforeSubmitPrompt",
                "afterFileEdit",
                "afterShellExecution",
                "afterMCPExecution",
                "beforeShellExecution",
                "beforeMCPExecution",
                "preToolUse",
                "postToolUse",
                "postToolUseFailure",
                "subagentStart",
                "subagentStop",
                "preCompact",
                "afterAgentResponse",
                "afterAgentThought",
                "stop",
                "sessionEnd",
            ],
        ),
        (
            AiTool::OpenCode,
            &[
                "session.created",
                "session.busy",
                "tool.execute.before",
                "tool.execute.after",
                "permission.asked",
                "question.asked",
                "session.retry",
                "session.error",
                "session.idle",
                "session.deleted",
            ],
        ),
        (
            AiTool::WorkBuddy,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionRequest",
                "Elicitation",
                "Stop",
                "StopFailure",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
                "SessionEnd",
            ],
        ),
        (
            AiTool::Hermes,
            &[
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
            ],
        ),
        (
            AiTool::OpenClaw,
            &[
                "session_start",
                "before_agent_run",
                "before_tool_call",
                "after_tool_call",
                "agent_end",
                "session_end",
            ],
        ),
        (
            AiTool::CodeBuddy,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionRequest",
                "PermissionDenied",
                "Elicitation",
                "Stop",
                "StopFailure",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
                "SessionEnd",
            ],
        ),
        (
            AiTool::QwenCode,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionRequest",
                "PermissionDenied",
                "Stop",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
                "SessionEnd",
            ],
        ),
        (
            AiTool::KimiCode,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionRequest",
                "Stop",
                "StopFailure",
                "Interrupt",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "SessionEnd",
            ],
        ),
        (
            AiTool::Qoder,
            &[
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "Stop",
            ],
        ),
        (
            AiTool::GeminiCli,
            &[
                "SessionStart",
                "BeforeAgent",
                "BeforeModel",
                "AfterModel",
                "BeforeToolSelection",
                "BeforeTool",
                "AfterTool",
                "PreCompress",
                "Notification",
                "AfterAgent",
                "SessionEnd",
            ],
        ),
        (
            AiTool::GitHubCopilot,
            &[
                "sessionStart",
                "userPromptSubmitted",
                "preToolUse",
                "postToolUse",
                "postToolUseFailure",
                "permissionRequest",
                "agentStop",
                "subagentStart",
                "subagentStop",
                "preCompact",
                "errorOccurred",
                "sessionEnd",
            ],
        ),
        (
            AiTool::Grok,
            &[
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "PermissionDenied",
                "Stop",
                "StopFailure",
                "StopCancelled",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "SessionEnd",
            ],
        ),
    ];
    for (tool, expected) in contracts {
        let actual = protocol(tool)
            .events()
            .iter()
            .map(|event| event.name)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{tool:?}");
    }
}

#[test]
/// 验证带 matcher 的事件与各上游协议子类型契约一致。
fn event_matchers_match_the_source_protocols() {
    let expected = [
        (AiTool::ClaudeCode, "Notification", "idle_prompt"),
        (AiTool::WorkBuddy, "Notification", "idle_prompt"),
        (AiTool::CodeBuddy, "Notification", "idle_prompt"),
        (AiTool::QwenCode, "Notification", "idle_prompt"),
        (AiTool::GeminiCli, "Notification", "ToolPermission"),
    ];
    for tool in AiTool::ALL {
        for event in protocol(tool).events() {
            let matcher = expected
                .iter()
                .find(|(expected_tool, expected_event, _)| {
                    *expected_tool == tool && *expected_event == event.name
                })
                .map(|(_, _, matcher)| *matcher);
            assert_eq!(event.matcher, matcher, "{tool:?} {}", event.name);
        }
    }
}
