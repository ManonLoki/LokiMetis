//! 产品公开 AI 类型的唯一目录，以及看板与监控各自的可选能力映射。
//!
//! Hook 协议全集仍由 `AiTool::ALL` 拥有；本模块只决定当前产品公开哪些类型。
//!
//! 命名约定（新代码须遵循；线标是已持久化的历史合同，不随本表改名）：
//!
//! | 展示名 | Rust 类型/变体 | snake / 常量 | 历史线标 |
//! |---|---|---|---|
//! | Codex | `Codex` | `codex` / `CODEX` | `codex` |
//! | Claude Code | `ClaudeCode` | `claude_code` / `CLAUDE_CODE` | `claudeCode`、`claude-code` |
//! | Cursor | `Cursor` | `cursor` / `CURSOR` | `cursor` |
//! | Grok | `Grok` | `grok` / `GROK` | `grok`、`grokBuildCli`、`grok-build-cli` |
//! | WorkBuddy | `WorkBuddy` | `workbuddy` / `WORKBUDDY` | `workbuddy`、`workBuddy`、`workbuddyProjectJsonl` |
//!
//! 界面名称一律经 [`SourceClientKind::display_name`]、[`SkinHostKind::display_name`]
//! 或 `ai_tool_name` 读取本目录，不在适配器中另写字符串。

use crate::{AiTool, SkinHostKind, SourceClientKind};

/// 一个公开 AI 类型在各产品区域中的可选能力映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicAiCapability {
    /// 跨区域一致的用户可见名称。
    pub name: &'static str,
    /// 看板可识别的数据源；`None` 表示看板忽略该类型。
    pub dashboard_client: Option<SourceClientKind>,
    /// 监控可识别的 Hook 工具；`None` 表示监控忽略该类型。
    pub monitor_tool: Option<AiTool>,
    /// 可应用本机皮肤的桌面宿主；`None` 表示换皮页忽略该类型。
    pub skin_host: Option<SkinHostKind>,
}

/// 当前公开 AI 类型的唯一固定目录；数组顺序就是各区域筛选后的展示顺序。
pub const PUBLIC_AI_CAPABILITIES: [PublicAiCapability; 5] = [
    PublicAiCapability {
        name: "Codex",
        dashboard_client: Some(SourceClientKind::Codex),
        monitor_tool: Some(AiTool::Codex),
        skin_host: Some(SkinHostKind::Codex),
    },
    PublicAiCapability {
        name: "Claude Code",
        dashboard_client: Some(SourceClientKind::ClaudeCode),
        monitor_tool: Some(AiTool::ClaudeCode),
        skin_host: None,
    },
    PublicAiCapability {
        name: "Cursor",
        dashboard_client: None,
        monitor_tool: Some(AiTool::Cursor),
        skin_host: None,
    },
    PublicAiCapability {
        name: "Grok",
        dashboard_client: Some(SourceClientKind::Grok),
        monitor_tool: Some(AiTool::Grok),
        skin_host: None,
    },
    PublicAiCapability {
        name: "WorkBuddy",
        dashboard_client: Some(SourceClientKind::WorkBuddy),
        monitor_tool: Some(AiTool::WorkBuddy),
        skin_host: Some(SkinHostKind::WorkBuddy),
    },
];

impl SourceClientKind {
    /// 看板来源的唯一展示名称，直接取自公开目录对应条目。
    pub const fn display_name(self) -> &'static str {
        PUBLIC_AI_CAPABILITIES[match self {
            Self::Codex => 0,
            Self::ClaudeCode => 1,
            Self::Grok => 3,
            Self::WorkBuddy => 4,
        }]
        .name
    }
}

impl SkinHostKind {
    /// 换皮宿主的唯一展示名称，界面与稳定错误共用。
    pub const fn display_name(self) -> &'static str {
        PUBLIC_AI_CAPABILITIES[match self {
            Self::Codex => 0,
            Self::WorkBuddy => 4,
        }]
        .name
    }
}

/// 返回当前公开 AI 类型目录。
pub const fn public_ai_capabilities() -> &'static [PublicAiCapability] {
    &PUBLIC_AI_CAPABILITIES
}

/// 按公开目录顺序返回监控能够识别的 Hook 工具，无法映射的类型被忽略。
pub fn public_monitor_ai_tools() -> impl Iterator<Item = AiTool> {
    PUBLIC_AI_CAPABILITIES
        .iter()
        .filter_map(|capability| capability.monitor_tool)
}

/// 判断工具是否属于当前公开的监控能力集合。
pub fn is_public_monitor_tool(tool: AiTool) -> bool {
    public_monitor_ai_tools().any(|candidate| candidate == tool)
}

/// 把统一 Agent 选择映射为看板物理客户端与独立 WorkBuddy 统计开关。
pub fn dashboard_selection_for_ai_tools(selected: &[AiTool]) -> (Vec<SourceClientKind>, bool) {
    let selected = selected
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut workbuddy = false;
    let clients = PUBLIC_AI_CAPABILITIES
        .iter()
        .filter(|capability| {
            capability
                .monitor_tool
                .is_some_and(|tool| selected.contains(&tool))
        })
        .filter_map(|capability| match capability.dashboard_client {
            Some(SourceClientKind::WorkBuddy) => {
                workbuddy = true;
                None
            }
            client => client,
        })
        .collect();
    (clients, workbuddy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三种身份枚举和 Hook 协议名称都必须回到同一目录条目，杜绝界面出现别名。
    #[test]
    fn every_identity_resolves_to_its_catalog_name() {
        for capability in PUBLIC_AI_CAPABILITIES {
            if let Some(client) = capability.dashboard_client {
                assert_eq!(client.display_name(), capability.name);
            }
            if let Some(host) = capability.skin_host {
                assert_eq!(host.display_name(), capability.name);
            }
            if let Some(tool) = capability.monitor_tool {
                assert_eq!(crate::ai_tool_name(tool), capability.name);
            }
        }
    }

    /// 唯一目录必须维持产品指定的五项顺序与跨区域映射。
    #[test]
    fn public_catalog_has_five_types_in_product_order() {
        assert_eq!(
            public_ai_capabilities(),
            &[
                PublicAiCapability {
                    name: "Codex",
                    dashboard_client: Some(SourceClientKind::Codex),
                    monitor_tool: Some(AiTool::Codex),
                    skin_host: Some(SkinHostKind::Codex),
                },
                PublicAiCapability {
                    name: "Claude Code",
                    dashboard_client: Some(SourceClientKind::ClaudeCode),
                    monitor_tool: Some(AiTool::ClaudeCode),
                    skin_host: None,
                },
                PublicAiCapability {
                    name: "Cursor",
                    dashboard_client: None,
                    monitor_tool: Some(AiTool::Cursor),
                    skin_host: None,
                },
                PublicAiCapability {
                    name: "Grok",
                    dashboard_client: Some(SourceClientKind::Grok),
                    monitor_tool: Some(AiTool::Grok),
                    skin_host: None,
                },
                PublicAiCapability {
                    name: "WorkBuddy",
                    dashboard_client: Some(SourceClientKind::WorkBuddy),
                    monitor_tool: Some(AiTool::WorkBuddy),
                    skin_host: Some(SkinHostKind::WorkBuddy),
                },
            ]
        );
    }

    /// 各区域只取得自身可映射项，不能把隐藏协议或 Cursor 伪装成看板能力。
    #[test]
    fn surface_mappings_filter_unmatched_and_hidden_tools() {
        assert_eq!(
            public_ai_capabilities().iter().filter_map(|capability| capability.dashboard_client).collect::<Vec<_>>(),
            vec![
                SourceClientKind::Codex,
                SourceClientKind::ClaudeCode,
                SourceClientKind::Grok,
                SourceClientKind::WorkBuddy,
            ]
        );
        assert_eq!(
            public_monitor_ai_tools().collect::<Vec<_>>(),
            vec![
                AiTool::Codex,
                AiTool::ClaudeCode,
                AiTool::Cursor,
                AiTool::Grok,
                AiTool::WorkBuddy,
            ]
        );
        assert!(is_public_monitor_tool(AiTool::WorkBuddy));
        assert!(!is_public_monitor_tool(AiTool::OpenCode));
        assert_eq!(AiTool::ALL.len(), 14);
    }

    /// 一个统一选择必须只投影到各区域真实支持的子集。
    #[test]
    fn unified_selection_maps_dashboard_and_skin_capabilities() {
        let selected = vec![AiTool::Codex, AiTool::Cursor, AiTool::WorkBuddy];
        let (dashboard, workbuddy) = dashboard_selection_for_ai_tools(&selected);
        assert_eq!(dashboard, vec![SourceClientKind::Codex]);
        assert!(workbuddy);
        assert_eq!(
            PUBLIC_AI_CAPABILITIES
                .iter()
                .filter(|capability| {
                    capability
                        .monitor_tool
                        .is_some_and(|tool| selected.contains(&tool))
                })
                .filter_map(|capability| capability.skin_host)
                .collect::<Vec<_>>(),
            vec![SkinHostKind::Codex, SkinHostKind::WorkBuddy]
        );
    }

}
