//! 持久化层 catalog adapter 与数据根写许可辅助。

use std::path::PathBuf;

#[cfg(test)]
use crate::backend::local_index::ScanPermit;
use crate::backend::local_index::{
    ClaudeDiscoveredRoot, DiscoveredRoot, DiscoveryMethod, LocalIndex, RegisterDiscoveredRoot,
};
use crate::dto::AgentClientKindDto;
use crate::dto::{SourceRootMutationDto, UiMessageCodeDto};
use crate::local_view::load_source_roots_for_parser;
use crate::runtime::AppRuntimeState;
use loki_metis_core::SourceRootLookupError;
use loki_metis_core::source_client_app_data_dir;
use loki_metis_core::{
    CatalogFuture, CoverageState, SourceClientKind, SourceRootCandidate, SourceRootCatalog,
    SourceRootCatalogError, SourceRootCatalogOperationError, SourceRootCatalogRecord,
    SourceRootMutationMessageCode, SourceRootMutationOutcome, source_root_mutation_feedback,
    source_root_operations_blocked_by_scan_message,
};

/// 测试专用别名，供获取与扫描共用的数据根写许可类型标注复用。
#[cfg(test)]
pub(crate) type SourceRootWriteLease = ScanPermit;

/// 持久化层 adapter：按 client 类型映射为同一 core catalog 接口。
pub(crate) struct SourceRootCatalogAdapter {
    /// 本机索引所在的 app-data 目录。
    app_data_dir: PathBuf,
    /// 该 catalog 适配器绑定的客户端类型。
    source_client: SourceClientKind,
}

impl SourceRootCatalogAdapter {
    /// 构建当前客户端的 catalog 适配器。
    pub(crate) fn new(app_data_dir: PathBuf, source_client: SourceClientKind) -> Self {
        Self {
            app_data_dir,
            source_client,
        }
    }

    /// 统一构造“索引不可用”这一固定错误值。
    const fn index_error() -> SourceRootCatalogError {
        SourceRootCatalogError::CatalogUnavailable
    }

    /// 按当前客户端的 app-data 目录与 parser version 打开本机索引。
    async fn open_index(&self) -> Result<LocalIndex, SourceRootCatalogError> {
        LocalIndex::open_in_app_data(&self.app_data_dir, self.source_client.parser_version())
            .await
            .map_err(|_| Self::index_error())
    }
}

impl SourceRootCatalog for SourceRootCatalogAdapter {
    /// 列出当前客户端索引中的全部数据根记录。
    fn list_roots(
        &self,
    ) -> CatalogFuture<'_, Result<Vec<SourceRootCatalogRecord>, SourceRootCatalogError>> {
        Box::pin(async move {
            let index = self.open_index().await?;
            let roots = index.all_roots().await.map_err(|_| Self::index_error())?;
            Ok(roots
                .into_iter()
                .filter_map(|root| {
                    root.root_id.map(|root_id| SourceRootCatalogRecord {
                        root_id,
                        enabled: root.enabled,
                    })
                })
                .collect())
        })
    }

    /// 按客户端类型构造对应的发现根类型，仅在不存在同一物理目录时登记。
    fn register_root_if_new(
        &mut self,
        candidate: &SourceRootCandidate,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
        let candidate = candidate.clone();
        Box::pin(async move {
            let mut index = self.open_index().await?;
            match self.source_client {
                SourceClientKind::Codex => {
                    let root = DiscoveredRoot {
                        path: candidate.path.clone(),
                        root_id: candidate.root_id.clone(),
                        alias: candidate.alias.clone(),
                        discovery_method: DiscoveryMethod::Registered,
                        has_sessions: false,
                        sessions_inspection_complete: false,
                        has_archived_sessions: false,
                        archived_sessions_inspection_complete: false,
                    };
                    index
                        .register_root_if_new(&root)
                        .await
                        .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
                }
                SourceClientKind::ClaudeCode => {
                    let root = ClaudeDiscoveredRoot {
                        path: candidate.path.clone(),
                        root_id: candidate.root_id.clone(),
                        alias: candidate.alias.clone(),
                        discovery_method: DiscoveryMethod::Registered,
                        evidence: loki_metis_core::RootCandidateEvidence::ClaudeTranscript,
                    };
                    index
                        .register_claude_root_if_new(&root)
                        .await
                        .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
                }
                SourceClientKind::GrokBuildCli => {
                    let root = crate::backend::local_index::GrokDiscoveredRoot {
                        path: candidate.path.clone(),
                        root_id: candidate.root_id.clone(),
                        alias: candidate.alias.clone(),
                        discovery_method: DiscoveryMethod::Registered,
                        evidence: loki_metis_core::RootCandidateEvidence::GrokSessionUpdates,
                    };
                    index
                        .register_grok_root_if_new(&root)
                        .await
                        .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
                }
                SourceClientKind::WorkBuddy => {
                    unreachable!("catalog adapter 只由 AgentClientKindDto 的三个批准客户端构造")
                }
            }
        })
    }

    /// 启用或停用指定数据根。
    fn set_root_enabled(
        &mut self,
        root_id: &str,
        enabled: bool,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
        let root_id = root_id.to_owned();
        Box::pin(async move {
            let mut index = self.open_index().await?;
            index
                .set_root_enabled(&root_id, enabled)
                .await
                .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
        })
    }

    /// 更新指定数据根的用户别名。
    fn set_root_alias(
        &mut self,
        root_id: &str,
        alias: &str,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
        let root_id = root_id.to_owned();
        let alias = alias.to_owned();
        Box::pin(async move {
            let mut index = self.open_index().await?;
            index
                .set_root_alias(&root_id, &alias)
                .await
                .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
        })
    }

    /// 从当前客户端索引中移除指定数据根。
    fn remove_root(
        &mut self,
        root_id: &str,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
        let root_id = root_id.to_owned();
        Box::pin(async move {
            let mut index = self.open_index().await?;
            index
                .remove_root(&root_id)
                .await
                .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
        })
    }

    /// 设置或清除 Codex 主数据目录选择。
    fn set_primary_root(
        &mut self,
        root_id: Option<&str>,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
        let root_id = root_id.map(str::to_owned);
        Box::pin(async move {
            let mut index = self.open_index().await?;
            index
                .set_primary_root(root_id.as_deref())
                .await
                .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
        })
    }
}

/// 统一 GUI DTO 与 core 语义客户端的映射，避免手写分支重复。
pub(crate) const fn to_source_client_kind(client: AgentClientKindDto) -> SourceClientKind {
    match client {
        AgentClientKindDto::Codex => SourceClientKind::Codex,
        AgentClientKindDto::ClaudeCode => SourceClientKind::ClaudeCode,
        AgentClientKindDto::GrokBuildCli => SourceClientKind::GrokBuildCli,
    }
}

/// 扫描 writer 运行时拒绝 registry 变更，避免中途改变扫描边界。
pub(crate) fn ensure_local_scan_not_running(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
) -> Result<(), String> {
    if state.local_scan.get(client.into()).is_running() {
        Err(source_root_operations_blocked_by_scan_message().to_owned())
    } else {
        Ok(())
    }
}

/// 原子取得与扫描共用的客户端 writer 许可。
#[cfg(test)]
pub(crate) fn acquire_source_root_write_permit(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
) -> Result<SourceRootWriteLease, String> {
    state
        .local_scan
        .get(client.into())
        .try_start()
        .map_err(|_| source_root_operations_blocked_by_scan_message().to_owned())
}

/// 将 Core 侧归约结果转为前端可消费的固定响应对象。
// 这是 add/enable/rename/remove/set_primary 五个 command 成功路径共同收敛
// 的唯一出口，在这里记一次 info 日志既覆盖了全部数据根变更事件，也避免
// 在每个 command 里各写一遍同样的 tracing 调用。只记 root_id（本身已是
// 校验过的稳定 ID，不是路径）与变更语义，不记用户可自由输入的别名文本。
pub(crate) fn build_source_root_mutation_response(
    client: AgentClientKindDto,
    outcome: SourceRootMutationOutcome,
) -> SourceRootMutationDto {
    tracing::info!(
        client = client.display_name(),
        changed = outcome.changed,
        kind = ?outcome.kind,
        "source root mutation applied"
    );
    let feedback = source_root_mutation_feedback(client.display_name(), outcome.kind);
    let message_code = match feedback.message_code {
        SourceRootMutationMessageCode::SourceRegistered => UiMessageCodeDto::SourceRegistered,
        SourceRootMutationMessageCode::SourceAlreadyRegistered => {
            UiMessageCodeDto::SourceAlreadyRegistered
        }
        SourceRootMutationMessageCode::SourceEnabled => UiMessageCodeDto::SourceEnabled,
        SourceRootMutationMessageCode::SourceDisabled => UiMessageCodeDto::SourceDisabled,
        SourceRootMutationMessageCode::SourceRenamed => UiMessageCodeDto::SourceRenamed,
        SourceRootMutationMessageCode::SourceRemoved => UiMessageCodeDto::SourceRemoved,
        SourceRootMutationMessageCode::PrimaryChanged => UiMessageCodeDto::PrimaryChanged,
        SourceRootMutationMessageCode::PrimaryAlreadySelected => {
            UiMessageCodeDto::PrimaryAlreadySelected
        }
        SourceRootMutationMessageCode::PrimaryCleared => UiMessageCodeDto::PrimaryCleared,
        SourceRootMutationMessageCode::PrimaryNotSet => UiMessageCodeDto::PrimaryNotSet,
    };

    SourceRootMutationDto {
        changed: outcome.changed,
        message: feedback.message,
        message_code,
    }
}

/// 变更后刷新安全来源摘要，并把覆盖状态降为需重新扫描的部分覆盖。
pub(crate) async fn refresh_source_roots_snapshot(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
) {
    let app_data_dir =
        source_client_app_data_dir(&state.app_data_dir, to_source_client_kind(client));
    let parser_version = parser_version_for_client(client);
    let environment_label = to_source_client_kind(client).source_environment_label();
    if let Ok(roots) =
        load_source_roots_for_parser(&app_data_dir, parser_version, environment_label).await
    {
        *state.roots.get(client.into()).write().await = roots;
    }
    state.coverages.get(client.into()).write().await.state = CoverageState::Partial;
}

/// 返回固定客户端对应的独立 parser version。
pub(crate) const fn parser_version_for_client(client: AgentClientKindDto) -> u32 {
    to_source_client_kind(client).parser_version()
}

/// 映射 catalog 操作错误到统一字符串，不泄露底层类型细节。
// 同 [`build_source_root_mutation_response`]，这是 enable/rename/remove/
// set_primary 四个 command 失败路径共同收敛的唯一出口，在这里记一次
// warn 日志即可覆盖全部数据根变更失败事件。
pub(crate) fn source_root_catalog_error_message(
    error: SourceRootCatalogOperationError,
    source_root_lookup_message: impl FnOnce(SourceRootLookupError) -> &'static str,
) -> String {
    tracing::warn!(?error, "source root mutation failed");
    match error {
        SourceRootCatalogOperationError::CatalogUnavailable => {
            loki_metis_core::source_root_store_error_message().to_owned()
        }
        SourceRootCatalogOperationError::SourceRootLookup(error) => {
            source_root_lookup_message(error).to_owned()
        }
    }
}
