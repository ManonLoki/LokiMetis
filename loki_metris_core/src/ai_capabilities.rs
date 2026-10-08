//! 产品公开 AI 类型的唯一目录，以及看板与监控各自的可选能力映射。
//!
//! Hook 协议全集仍由 `AiTool::ALL` 拥有；本模块只决定当前产品公开哪些类型。

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
        dashboard_client: Some(SourceClientKind::GrokBuildCli),
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

/// 返回当前公开 AI 类型目录。
pub const fn public_ai_capabilities() -> &'static [PublicAiCapability] {
    &PUBLIC_AI_CAPABILITIES
}

/// 按公开目录顺序返回看板能够识别的数据源，无法映射的类型被忽略。
pub fn public_dashboard_clients() -> impl Iterator<Item = SourceClientKind> {
    PUBLIC_AI_CAPABILITIES
        .iter()
        .filter_map(|capability| capability.dashboard_client)
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

/// 合并升级前分散保存的看板与 Hooks 选择，并按公开目录规范化顺序。
pub fn merge_public_ai_selections(
    dashboard_clients: impl IntoIterator<Item = SourceClientKind>,
    workbuddy_enabled: bool,
    monitor_tools: &[AiTool],
) -> Vec<AiTool> {
    let dashboard = dashboard_clients
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let monitor = monitor_tools
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    PUBLIC_AI_CAPABILITIES
        .iter()
        .filter_map(|capability| {
            let tool = capability.monitor_tool?;
            let enabled_by_dashboard = match capability.dashboard_client {
                Some(SourceClientKind::WorkBuddy) => workbuddy_enabled,
                Some(client) => dashboard.contains(&client),
                None => false,
            };
            (enabled_by_dashboard || monitor.contains(&tool)).then_some(tool)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    dashboard_client: Some(SourceClientKind::GrokBuildCli),
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
            public_dashboard_clients().collect::<Vec<_>>(),
            vec![
                SourceClientKind::Codex,
                SourceClientKind::ClaudeCode,
                SourceClientKind::GrokBuildCli,
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

    /// 升级合并保留任一区域已开启项，同时丢弃隐藏协议并恢复固定顺序。
    #[test]
    fn legacy_selections_merge_without_losing_enabled_agents() {
        assert_eq!(
            merge_public_ai_selections(
                [SourceClientKind::GrokBuildCli],
                true,
                &[AiTool::Cursor, AiTool::OpenCode],
            ),
            vec![AiTool::Cursor, AiTool::Grok, AiTool::WorkBuddy]
        );
    }
}
