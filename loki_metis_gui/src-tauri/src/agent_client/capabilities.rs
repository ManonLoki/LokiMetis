//! 定义 GUI 运行时可组装的源端能力绑定。

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_client::analysis::LocalIndexBinding;
use crate::agent_client::scanners::{
    ClaudeLocalUsageScanner, CodexLocalUsageScanner, GrokLocalUsageScanner,
};
use tokio::sync::Mutex;

/// 将本机索引绑定与扫描器装配为单一来源客户端能力。
pub(crate) struct AgentClientCapabilities {
    /// 本机索引路径与写路径绑定。
    pub(crate) local_analysis: Arc<LocalIndexBinding>,
    /// 本机扫描入口。
    pub(crate) local_scanner: Arc<dyn loki_metis_core::LocalUsageScanner>,
}

impl AgentClientCapabilities {
    /// 装配 Codex 本机能力集合。
    pub(crate) fn codex(app_data_dir: PathBuf, account_context_gate: Arc<Mutex<()>>) -> Self {
        Self {
            local_analysis: Arc::new(LocalIndexBinding::for_codex(app_data_dir.clone())),
            local_scanner: Arc::new(CodexLocalUsageScanner::with_account_context_gate(
                app_data_dir,
                account_context_gate,
            )),
        }
    }

    /// 装配 Claude Code 本机能力集合。
    pub(crate) fn claude_code(product_app_data_dir: PathBuf, app_data_dir: PathBuf) -> Self {
        Self {
            local_analysis: Arc::new(LocalIndexBinding::for_claude_code(app_data_dir.clone())),
            local_scanner: Arc::new(ClaudeLocalUsageScanner::new(
                app_data_dir,
                product_app_data_dir,
            )),
        }
    }

    /// 装配 Grok Build CLI 本机能力集合。
    pub(crate) fn grok_build_cli(product_app_data_dir: PathBuf, app_data_dir: PathBuf) -> Self {
        Self {
            local_analysis: Arc::new(LocalIndexBinding::for_grok_build_cli(app_data_dir.clone())),
            local_scanner: Arc::new(GrokLocalUsageScanner::new(
                app_data_dir,
                product_app_data_dir,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::dto::AgentClientKindDto;
    use loki_metis_core::AgentClientRegistry;

    /// 验证注册表按客户端枚举精确取值，不会取到另一客户端的槽位。
    #[test]
    fn agent_client_registry_selects_exact_slot() {
        let registry = AgentClientRegistry::new("codex", "claude", "grok");
        assert_eq!(*registry.get(AgentClientKindDto::Codex.into()), "codex");
        assert_eq!(
            *registry.get(AgentClientKindDto::ClaudeCode.into()),
            "claude"
        );
        assert_eq!(
            *registry.get(AgentClientKindDto::GrokBuildCli.into()),
            "grok"
        );
    }
}
