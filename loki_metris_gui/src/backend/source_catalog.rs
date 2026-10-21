//! 根登记的 SQLite 宿主端口；业务前置条件和结果归约仍由 core 决定。

use std::path::PathBuf;
use crate::backend::local_index::{ClaudeDiscoveredRoot, DiscoveredRoot, DiscoveryMethod, LocalIndex, RegisterDiscoveredRoot};
use loki_metris_core::{CatalogFuture, SourceClientKind, SourceRootCandidate, SourceRootCatalog, SourceRootCatalogError, SourceRootCatalogRecord};

/// 持久化层 adapter：按 client 类型映射为同一 core catalog 接口。
pub(crate) struct SourceRootCatalogAdapter {
    /// 本机索引所在的 app-data 目录。
    app_data_dir: PathBuf,
    /// 该 catalog 适配器绑定的客户端类型。
    source_client: SourceClientKind,
    /// 写入前最后复核的同一 writer 取消令牌。
    cancellation:loki_metris_core::ScanCancellation,
}

impl SourceRootCatalogAdapter {
    /// 构建当前客户端的 catalog 适配器。
    pub(crate) fn new(app_data_dir: PathBuf, source_client: SourceClientKind) -> Result<Self, SourceRootCatalogError> {
        if source_client == SourceClientKind::WorkBuddy { return Err(Self::index_error()); }
        Ok(Self { app_data_dir, source_client,cancellation:loki_metris_core::ScanCancellation::new() })
    }

    /// 应用显式写请求把许可令牌传到实际 SQLite 变更边界。
    pub(crate) fn with_cancellation(mut self,cancellation:loki_metris_core::ScanCancellation)->Self {
        self.cancellation=cancellation;self
    }

    /// 统一构造“索引不可用”这一固定错误值。
    const fn index_error() -> SourceRootCatalogError {
        SourceRootCatalogError::CatalogUnavailable
    }

    /// 按当前客户端的 app-data 目录与 parser version 打开本机索引。
    async fn open_index(&self) -> Result<LocalIndex, SourceRootCatalogError> {
        self.ensure_active()?;
        let index=LocalIndex::open_in_app_data(&self.app_data_dir, self.source_client.parser_version())
            .await
            .map_err(|_| Self::index_error())?;
        self.ensure_active()?;
        Ok(index)
    }

    /// 原子变更一旦开始就等待实际提交；取消只在开始前拒绝。
    fn ensure_active(&self)->Result<(),SourceRootCatalogError> {
        if self.cancellation.is_cancelled() {Err(Self::index_error())}else{Ok(())}
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
                        evidence: loki_metris_core::RootCandidateEvidence::ClaudeTranscript,
                    };
                    index
                        .register_claude_root_if_new(&root)
                        .await
                        .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
                }
                SourceClientKind::Grok => {
                    let root = crate::backend::local_index::GrokDiscoveredRoot {
                        path: candidate.path.clone(),
                        root_id: candidate.root_id.clone(),
                        alias: candidate.alias.clone(),
                        discovery_method: DiscoveryMethod::Registered,
                        evidence: loki_metris_core::RootCandidateEvidence::GrokSessionUpdates,
                    };
                    index
                        .register_grok_root_if_new(&root)
                        .await
                        .map_err(|_| SourceRootCatalogError::CatalogUnavailable)
                }
                SourceClientKind::WorkBuddy => {
                    Err(SourceRootCatalogError::CatalogUnavailable)
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
