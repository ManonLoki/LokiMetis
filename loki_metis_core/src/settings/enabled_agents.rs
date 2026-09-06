//! 用户显式开放的本机 Agent 集合；缺省为空，关闭项不得扫描或上报。

use crate::SourceClientKind;

/// 无法把用户输入解释成已批准本机 Agent 时的稳定错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnabledAgentsError {
    /// 出现 Cursor 或任何未批准标识。
    UnknownAgent,
}

/// 用户已开放监控和上报的本机 Agent 集合；默认全关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnabledAgents {
    /// 是否开放 Codex。
    codex: bool,
    /// 是否开放 Claude Code。
    claude_code: bool,
    /// 是否开放 Grok Build CLI。
    grok_build_cli: bool,
}

impl EnabledAgents {
    /// 返回三个本机 Agent 全部关闭的集合。
    pub const fn empty() -> Self {
        Self {
            codex: false,
            claude_code: false,
            grok_build_cli: false,
        }
    }

    /// 从稳定线标解析集合；未知标识一律拒绝，重复项只生效一次。
    pub fn try_from_labels<S: AsRef<str>>(labels: &[S]) -> Result<Self, EnabledAgentsError> {
        let mut enabled = Self::empty();
        for label in labels {
            let client = parse_agent_label(label.as_ref())?;
            enabled = enabled.with(client, true);
        }
        Ok(enabled)
    }

    /// 旧设置缺少该字段时视为全部关闭；逐项忽略当前看板无法识别的历史标识。
    pub fn from_stored_labels(labels: Option<&[String]>) -> Result<Self, EnabledAgentsError> {
        let mut enabled = Self::empty();
        for label in labels.into_iter().flatten() {
            if let Some(client) = parse_single_agent_label(label) {
                enabled = enabled.with(client, true);
            }
        }
        Ok(enabled)
    }

    /// 打开或关闭一个已批准 Agent，不改变其余项。
    pub const fn with(self, client: SourceClientKind, enabled: bool) -> Self {
        match client {
            SourceClientKind::Codex => Self {
                codex: enabled,
                ..self
            },
            SourceClientKind::ClaudeCode => Self {
                claude_code: enabled,
                ..self
            },
            SourceClientKind::GrokBuildCli => Self {
                grok_build_cli: enabled,
                ..self
            },
            SourceClientKind::WorkBuddy => self,
        }
    }

    /// 判断该 Agent 是否已由用户显式开放。
    pub const fn contains(self, client: SourceClientKind) -> bool {
        match client {
            SourceClientKind::Codex => self.codex,
            SourceClientKind::ClaudeCode => self.claude_code,
            SourceClientKind::GrokBuildCli => self.grok_build_cli,
            SourceClientKind::WorkBuddy => false,
        }
    }

    /// 是否一个都未开放。
    pub const fn is_empty(self) -> bool {
        !self.codex && !self.claude_code && !self.grok_build_cli
    }

    /// 按固定顺序返回已开放 Agent，供页头、周期扫描和 Collect 共用。
    pub fn iter(self) -> impl Iterator<Item = SourceClientKind> {
        crate::public_dashboard_clients()
            .filter(|client| *client != SourceClientKind::WorkBuddy)
            .filter(move |client| self.contains(*client))
    }

    /// 返回已开放 Agent 的稳定线标，供设置文件与 IPC 使用。
    pub fn labels(self) -> Vec<&'static str> {
        self.iter().map(agent_wire_label).collect()
    }
}

/// 返回本机 Agent 在设置文件和 IPC 中的稳定标识。
pub const fn agent_wire_label(client: SourceClientKind) -> &'static str {
    match client {
        SourceClientKind::Codex => "codex",
        SourceClientKind::ClaudeCode => "claudeCode",
        SourceClientKind::GrokBuildCli => "grokBuildCli",
        SourceClientKind::WorkBuddy => "workbuddy",
    }
}

/// 把稳定线标解析为单个本机 Agent；未知值返回 `None`，供“最近一次选中 Agent”等单值场景复用。
pub fn parse_single_agent_label(label: &str) -> Option<SourceClientKind> {
    parse_agent_label(label).ok()
}

/// 把稳定线标解析为本机 Agent；未知值不得伪装成已开放项。
fn parse_agent_label(label: &str) -> Result<SourceClientKind, EnabledAgentsError> {
    match label {
        "codex" => Ok(SourceClientKind::Codex),
        "claudeCode" => Ok(SourceClientKind::ClaudeCode),
        "grokBuildCli" => Ok(SourceClientKind::GrokBuildCli),
        _ => Err(EnabledAgentsError::UnknownAgent),
    }
}

/// 返回开放集合校验失败时的稳定中文说明。
pub const fn enabled_agents_error_message(error: EnabledAgentsError) -> &'static str {
    match error {
        EnabledAgentsError::UnknownAgent => "只能开放 Codex、Claude Code 或 Grok。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省与旧文件缺字段都必须是全关，不能回填成三个全开。
    #[test]
    fn defaults_and_missing_storage_are_empty() {
        assert_eq!(EnabledAgents::default(), EnabledAgents::empty());
        assert!(EnabledAgents::empty().is_empty());
        assert_eq!(
            EnabledAgents::from_stored_labels(None),
            Ok(EnabledAgents::empty())
        );
        assert!(!EnabledAgents::empty().contains(SourceClientKind::Codex));
        assert!(!EnabledAgents::empty().contains(SourceClientKind::ClaudeCode));
        assert!(!EnabledAgents::empty().contains(SourceClientKind::GrokBuildCli));
        assert_eq!(EnabledAgents::empty().labels(), Vec::<&str>::new());
    }

    /// 合法线标去重后按固定顺序开放；未知标识必须拒绝。
    #[test]
    fn parses_approved_labels_and_rejects_unknown() {
        let enabled = EnabledAgents::try_from_labels(&["grokBuildCli", "codex", "codex"])
            .expect("approved labels parse");
        assert!(enabled.contains(SourceClientKind::Codex));
        assert!(!enabled.contains(SourceClientKind::ClaudeCode));
        assert!(enabled.contains(SourceClientKind::GrokBuildCli));
        assert_eq!(enabled.labels(), vec!["codex", "grokBuildCli"]);
        assert_eq!(
            EnabledAgents::try_from_labels(&["cursor"]),
            Err(EnabledAgentsError::UnknownAgent)
        );
        assert_eq!(
            EnabledAgents::from_stored_labels(Some(&["claudeCode".to_owned()])),
            Ok(EnabledAgents::empty().with(SourceClientKind::ClaudeCode, true))
        );
        assert_eq!(
            EnabledAgents::from_stored_labels(Some(&[
                "cursor".to_owned(),
                "futureAgent".to_owned(),
                "codex".to_owned(),
            ])),
            Ok(EnabledAgents::empty().with(SourceClientKind::Codex, true))
        );
    }

    /// 单值解析必须接受三个稳定线标，拒绝未知标识而不是猜测一个默认值。
    #[test]
    fn parses_single_agent_label_and_rejects_unknown() {
        assert_eq!(
            parse_single_agent_label("codex"),
            Some(SourceClientKind::Codex)
        );
        assert_eq!(
            parse_single_agent_label("claudeCode"),
            Some(SourceClientKind::ClaudeCode)
        );
        assert_eq!(
            parse_single_agent_label("grokBuildCli"),
            Some(SourceClientKind::GrokBuildCli)
        );
        assert_eq!(parse_single_agent_label("cursor"), None);
    }

    /// 关闭一项不得把其余已开放项改掉，也不得把关闭项伪装成仍开放。
    #[test]
    fn toggling_one_agent_does_not_enable_the_others() {
        let only_claude = EnabledAgents::empty()
            .with(SourceClientKind::ClaudeCode, true)
            .with(SourceClientKind::Codex, true)
            .with(SourceClientKind::Codex, false);
        assert!(!only_claude.contains(SourceClientKind::Codex));
        assert!(only_claude.contains(SourceClientKind::ClaudeCode));
        assert!(!only_claude.contains(SourceClientKind::GrokBuildCli));
        assert_eq!(
            only_claude.iter().collect::<Vec<_>>(),
            vec![SourceClientKind::ClaudeCode]
        );
    }
}
