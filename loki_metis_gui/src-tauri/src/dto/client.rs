//! 标识当前查看和操作的固定本机客户端；三个批准客户端之外一律拒绝。

use serde::{Deserialize, Serialize};

/// 标识当前查看和操作的固定 Agent 客户端；未知字符串在反序列化边界直接拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentClientKindDto {
    /// 使用 Codex 本机 rollout 记录。
    Codex,
    /// 使用 Claude Code 本机 transcript。
    ClaudeCode,
    /// 使用 Grok Build CLI 本机 `updates.jsonl`。
    GrokBuildCli,
}

impl AgentClientKindDto {
    /// 返回全部固定客户端，供生命周期循环避免遗漏新槽位。
    // 把“有哪些客户端”这个事实集中定义在这一个数组常量里，
    // 其余代码（如 runtime.rs 里 `for client in AgentClientKindDto::ALL`）
    // 全部通过遍历它来处理“对每个客户端都要做一遍”的逻辑，
    // 未来如果新增客户端，只需要改这一处就不会有遗漏。
    pub(crate) const ALL: [Self; 3] = [Self::Codex, Self::ClaudeCode, Self::GrokBuildCli];

    /// 返回纯中文界面可安全展示的客户端名称。
    pub(crate) const fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::GrokBuildCli => "Grok",
        }
    }
}

impl From<AgentClientKindDto> for loki_metis_core::SourceClientKind {
    /// 把 IPC 边界的客户端枚举映射为 core 的语义客户端类型。
    fn from(client: AgentClientKindDto) -> Self {
        match client {
            AgentClientKindDto::Codex => Self::Codex,
            AgentClientKindDto::ClaudeCode => Self::ClaudeCode,
            AgentClientKindDto::GrokBuildCli => Self::GrokBuildCli,
        }
    }
}

impl From<loki_metis_core::SourceClientKind> for AgentClientKindDto {
    /// 把 core 的语义客户端类型映射回 IPC 边界的客户端枚举；WorkBuddy 不是物理扫描
    /// 客户端，不占用本枚举槽位，调用方不得把它传入本转换。
    fn from(client: loki_metis_core::SourceClientKind) -> Self {
        match client {
            loki_metis_core::SourceClientKind::Codex => Self::Codex,
            loki_metis_core::SourceClientKind::ClaudeCode => Self::ClaudeCode,
            loki_metis_core::SourceClientKind::GrokBuildCli => Self::GrokBuildCli,
            loki_metis_core::SourceClientKind::WorkBuddy => {
                unreachable!("WorkBuddy 不是物理扫描客户端，不应转换为 AgentClientKindDto")
            }
        }
    }
}

/// 标识可在用量界面选择的本机客户端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageClientKindDto {
    /// Codex 本机 rollout。
    Codex,
    /// Claude Code 本机 transcript。
    ClaudeCode,
    /// Grok Build CLI 本机会话用量。
    GrokBuildCli,
}

impl UsageClientKindDto {
    /// 返回全部可展示的用量客户端。
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::Codex, Self::ClaudeCode, Self::GrokBuildCli];

    /// 把用量客户端映射为本机数据源客户端。
    pub(crate) const fn local_client(self) -> AgentClientKindDto {
        match self {
            Self::Codex => AgentClientKindDto::Codex,
            Self::ClaudeCode => AgentClientKindDto::ClaudeCode,
            Self::GrokBuildCli => AgentClientKindDto::GrokBuildCli,
        }
    }
}

impl From<AgentClientKindDto> for UsageClientKindDto {
    /// 把本机客户端枚举映射为用量客户端枚举。
    fn from(client: AgentClientKindDto) -> Self {
        match client {
            AgentClientKindDto::Codex => Self::Codex,
            AgentClientKindDto::ClaudeCode => Self::ClaudeCode,
            AgentClientKindDto::GrokBuildCli => Self::GrokBuildCli,
        }
    }
}

/// 标识概览与调用允许选择的只读视图；`All` 不可用于任何写操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageViewKindDto {
    /// 所有已开启物理 Agent 的只读联合视图。
    All,
    /// Codex 本机用量视图。
    Codex,
    /// Claude Code 本机用量视图。
    ClaudeCode,
    /// Grok Build CLI 本机用量视图。
    GrokBuildCli,
    /// WorkBuddy 本机只读统计视图；不是物理扫描客户端。
    Workbuddy,
}

impl UsageViewKindDto {
    /// 具体 Agent 返回现有物理客户端；联合视图不伪装成任何一个客户端。
    pub(crate) const fn local_client(self) -> Option<AgentClientKindDto> {
        match self {
            Self::All | Self::Workbuddy => None,
            Self::Codex => Some(AgentClientKindDto::Codex),
            Self::ClaudeCode => Some(AgentClientKindDto::ClaudeCode),
            Self::GrokBuildCli => Some(AgentClientKindDto::GrokBuildCli),
        }
    }
}

impl From<AgentClientKindDto> for UsageViewKindDto {
    /// 把具体物理 Agent 映射为同名只读视图。
    fn from(client: AgentClientKindDto) -> Self {
        match client {
            AgentClientKindDto::Codex => Self::Codex,
            AgentClientKindDto::ClaudeCode => Self::ClaudeCode,
            AgentClientKindDto::GrokBuildCli => Self::GrokBuildCli,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentClientKindDto, UsageClientKindDto, UsageViewKindDto};

    /// 验证客户端枚举只接受批准的三个稳定 wire 值，并完整列出生命周期槽位。
    #[test]
    fn agent_client_kind_rejects_unknown_wire_values() {
        assert_eq!(
            serde_json::from_str::<AgentClientKindDto>("\"codex\"").unwrap(),
            AgentClientKindDto::Codex
        );
        assert_eq!(
            serde_json::from_str::<AgentClientKindDto>("\"claudeCode\"").unwrap(),
            AgentClientKindDto::ClaudeCode
        );
        assert_eq!(
            serde_json::from_str::<AgentClientKindDto>("\"grokBuildCli\"").unwrap(),
            AgentClientKindDto::GrokBuildCli
        );
        assert_eq!(AgentClientKindDto::Codex.display_name(), "Codex");
        assert_eq!(AgentClientKindDto::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(AgentClientKindDto::GrokBuildCli.display_name(), "Grok");
        assert_eq!(
            AgentClientKindDto::ALL,
            [
                AgentClientKindDto::Codex,
                AgentClientKindDto::ClaudeCode,
                AgentClientKindDto::GrokBuildCli
            ]
        );
        assert!(serde_json::from_str::<AgentClientKindDto>("\"cursor\"").is_err());
        assert!(serde_json::from_str::<AgentClientKindDto>("\"all\"").is_err());
    }

    /// 验证用量客户端与本机客户端同一集合，且拒绝 Cursor 与未知 wire 值。
    #[test]
    fn usage_client_kind_rejects_cursor_and_unknown_wire_values() {
        assert_eq!(
            serde_json::from_str::<UsageClientKindDto>("\"codex\"").unwrap(),
            UsageClientKindDto::Codex
        );
        assert_eq!(
            serde_json::from_str::<UsageClientKindDto>("\"claudeCode\"").unwrap(),
            UsageClientKindDto::ClaudeCode
        );
        assert_eq!(
            serde_json::from_str::<UsageClientKindDto>("\"grokBuildCli\"").unwrap(),
            UsageClientKindDto::GrokBuildCli
        );
        assert!(serde_json::from_str::<UsageClientKindDto>("\"cursor\"").is_err());
        assert!(serde_json::from_str::<UsageClientKindDto>("\"all\"").is_err());
        assert_eq!(
            UsageClientKindDto::ALL,
            [
                UsageClientKindDto::Codex,
                UsageClientKindDto::ClaudeCode,
                UsageClientKindDto::GrokBuildCli,
            ]
        );
        assert_eq!(
            UsageClientKindDto::from(AgentClientKindDto::Codex).local_client(),
            AgentClientKindDto::Codex
        );
        assert_eq!(
            UsageClientKindDto::from(AgentClientKindDto::ClaudeCode).local_client(),
            AgentClientKindDto::ClaudeCode
        );
        assert_eq!(
            UsageClientKindDto::from(AgentClientKindDto::GrokBuildCli).local_client(),
            AgentClientKindDto::GrokBuildCli
        );
    }

    /// 只有概览/调用视图接受 `all`，并且它不能映射为物理客户端。
    #[test]
    fn usage_view_kind_accepts_all_without_expanding_physical_clients() {
        assert_eq!(
            serde_json::from_str::<UsageViewKindDto>("\"all\"").unwrap(),
            UsageViewKindDto::All
        );
        assert_eq!(UsageViewKindDto::All.local_client(), None);
        assert_eq!(
            UsageViewKindDto::from(AgentClientKindDto::ClaudeCode).local_client(),
            Some(AgentClientKindDto::ClaudeCode)
        );
        assert_eq!(
            serde_json::from_str::<UsageViewKindDto>("\"workbuddy\"").unwrap(),
            UsageViewKindDto::Workbuddy
        );
        assert_eq!(UsageViewKindDto::Workbuddy.local_client(), None);
        assert!(serde_json::from_str::<UsageViewKindDto>("\"cursor\"").is_err());
        assert!(serde_json::from_str::<AgentClientKindDto>("\"all\"").is_err());
        assert!(serde_json::from_str::<UsageClientKindDto>("\"all\"").is_err());
        assert!(serde_json::from_str::<AgentClientKindDto>("\"workbuddy\"").is_err());
    }
}
