//! 本机 Agent 在设置文件、Hook 协议与稳定身份中的线标。

use crate::SourceClientKind;

/// 返回本机 Agent 在设置文件和 IPC 中的稳定标识。
pub const fn agent_wire_label(client: SourceClientKind) -> &'static str {
    match client {
        SourceClientKind::Codex => "codex",
        SourceClientKind::ClaudeCode => "claudeCode",
        SourceClientKind::Grok => "grokBuildCli",
        SourceClientKind::WorkBuddy => "workbuddy",
    }
}
