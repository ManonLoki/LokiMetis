// 本文件是 tests_contract.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证每个工具适配器暴露完整且精确的原生事件表。；验证带 matcher 的事件与各上游协议子类型契约一致。

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
                "PermissionDenied",
                "Elicitation",
                "PostToolUseFailure",
                "Stop",
                "StopFailure",
                "ElicitationResult",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
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
                "PermissionDenied",
                "Elicitation",
                "ElicitationResult",
                "Stop",
                "StopFailure",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
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
                "ElicitationResult",
                "Stop",
                "StopFailure",
                "SubagentStart",
                "SubagentStop",
                "PreCompact",
                "PostCompact",
                "Notification",
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
        (AiTool::ClaudeCode, "Notification", 0, "idle_prompt"),
        (AiTool::ClaudeCode, "Notification", 1, "permission_prompt"),
        (AiTool::WorkBuddy, "Notification", 0, "idle_prompt"),
        (AiTool::WorkBuddy, "Notification", 1, "permission_prompt"),
        (AiTool::CodeBuddy, "Notification", 0, "idle_prompt"),
        (AiTool::CodeBuddy, "Notification", 1, "permission_prompt"),
        (AiTool::QwenCode, "Notification", 0, "idle_prompt"),
        (AiTool::GeminiCli, "Notification", 0, "ToolPermission"),
    ];
    for tool in AiTool::ALL {
        let mut occurrences = std::collections::HashMap::<&str, usize>::new();
        for event in protocol(tool).events() {
            let occurrence = occurrences.entry(event.name).or_default();
            let matcher = expected
                .iter()
                .find(|(expected_tool, expected_event, expected_index, _)| {
                    *expected_tool == tool
                        && *expected_event == event.name
                        && *expected_index == *occurrence
                })
                .map(|(_, _, _, matcher)| *matcher);
            assert_eq!(event.matcher, matcher, "{tool:?} {}", event.name);
            *occurrence += 1;
        }
    }
}
