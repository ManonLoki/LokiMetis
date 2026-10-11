//! 源根变更与持久层边界解耦。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use thiserror::Error;

use crate::{
    SourceRootLookupError, SourceRootMutationOutcome, resolve_enabled_source_root_by_id,
    resolve_source_root_by_id, source_root_add_outcome, source_root_primary_outcome,
    source_root_remove_outcome, source_root_rename_outcome, source_root_set_enabled_outcome,
};

/// catalog 能力的 object-safe async 返回值别名。
pub type CatalogFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 表示 source root registry 映射到 core 时的统一输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootCandidate {
    /// 规范化后的稳定 ID。
    pub root_id: String,
    /// 安全展示别名。
    pub alias: String,
    /// 用于内部校验的根路径。
    pub path: PathBuf,
}

/// 表示 registry 中可复用到的最小字段摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootCatalogRecord {
    /// 稳定识别 ID。
    pub root_id: String,
    /// 是否参与扫描。
    pub enabled: bool,
}

/// 表示已经通过 catalog 存在性与启用状态校验的单根重新索引请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootReindexRequest {
    /// 待重新索引的数据根稳定 ID；不包含文件系统路径。
    root_id: String,
}

impl SourceRootReindexRequest {
    /// 返回已经通过业务层校验的数据根稳定 ID。
    pub fn root_id(&self) -> &str {
        &self.root_id
    }
}

/// 表示 catalog 持久化层不可达。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootCatalogError {
    /// catalog 打开或写入失败。
    #[error("source root catalog operation failed")]
    CatalogUnavailable,
}

/// 表示 registry 业务层的结果与底层错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootCatalogOperationError {
    /// 业务入口 root_id 校验未通过或状态不满足。
    #[error("source root lookup failed")]
    SourceRootLookup(SourceRootLookupError),
    /// catalog 持久化层操作失败。
    #[error("source root catalog is unavailable")]
    CatalogUnavailable,
}

impl From<SourceRootCatalogError> for SourceRootCatalogOperationError {
    /// 将目录领域错误收敛为持久化端口的稳定错误。
    fn from(_: SourceRootCatalogError) -> Self {
        Self::CatalogUnavailable
    }
}

/// transport/adapter 实现此 trait 的统一异步入口。
pub trait SourceRootCatalog: Send {
    /// 返回全部 registry 根快照，用于业务层前置校验。
    /// 列出指定客户端的安全数据根记录。
    fn list_roots(
        &self,
    ) -> CatalogFuture<'_, Result<Vec<SourceRootCatalogRecord>, SourceRootCatalogError>>;

    /// 在 registry 中尝试新增，返回是否新增成功。
    /// 仅在路径尚未登记时创建新数据根记录。
    fn register_root_if_new(
        &mut self,
        candidate: &SourceRootCandidate,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>>;

    /// 启用/停用指定根。
    /// 按稳定标识切换数据根启用状态。
    fn set_root_enabled(
        &mut self,
        root_id: &str,
        enabled: bool,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>>;

    /// 更新指定根的展示别名。
    /// 按稳定标识更新数据根安全别名。
    fn set_root_alias(
        &mut self,
        root_id: &str,
        alias: &str,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>>;

    /// 删除指定根。
    /// 按稳定标识移除数据根登记并返回是否存在。
    fn remove_root(
        &mut self,
        root_id: &str,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>>;

    /// 变更 Codex 主数据根。
    /// 为支持主目录能力的客户端设置唯一主数据根。
    fn set_primary_root(
        &mut self,
        root_id: Option<&str>,
    ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>>;
}

/// 按业务约束执行新增源根变更。
pub async fn source_root_add(
    catalog: &mut impl SourceRootCatalog,
    candidate: &SourceRootCandidate,
) -> Result<SourceRootMutationOutcome, SourceRootCatalogOperationError> {
    let changed = catalog
        .register_root_if_new(candidate)
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    Ok(source_root_add_outcome(changed))
}

/// 按业务约束执行源根启停变更。
pub async fn source_root_set_enabled(
    catalog: &mut impl SourceRootCatalog,
    root_id: &str,
    enabled: bool,
) -> Result<SourceRootMutationOutcome, SourceRootCatalogOperationError> {
    let roots = catalog
        .list_roots()
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    resolve_source_root_by_id(&roots, root_id, |root| Some(root.root_id.as_str()))
        .map_err(SourceRootCatalogOperationError::SourceRootLookup)?;

    let changed = catalog
        .set_root_enabled(root_id, enabled)
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    Ok(source_root_set_enabled_outcome(enabled, changed))
}

/// 按业务约束执行别名变更。
pub async fn source_root_rename(
    catalog: &mut impl SourceRootCatalog,
    root_id: &str,
    alias: &str,
) -> Result<SourceRootMutationOutcome, SourceRootCatalogOperationError> {
    let roots = catalog
        .list_roots()
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    resolve_source_root_by_id(&roots, root_id, |root| Some(root.root_id.as_str()))
        .map_err(SourceRootCatalogOperationError::SourceRootLookup)?;

    let changed = catalog
        .set_root_alias(root_id, alias)
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    Ok(source_root_rename_outcome(changed))
}

/// 按业务约束执行源根删除。
pub async fn source_root_remove(
    catalog: &mut impl SourceRootCatalog,
    root_id: &str,
) -> Result<SourceRootMutationOutcome, SourceRootCatalogOperationError> {
    let roots = catalog
        .list_roots()
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    resolve_source_root_by_id(&roots, root_id, |root| Some(root.root_id.as_str()))
        .map_err(SourceRootCatalogOperationError::SourceRootLookup)?;

    let changed = catalog
        .remove_root(root_id)
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    Ok(source_root_remove_outcome(changed))
}

/// 按业务约束执行主数据目录设置。
pub async fn source_root_set_primary(
    catalog: &mut impl SourceRootCatalog,
    requested_primary_root: Option<&str>,
) -> Result<SourceRootMutationOutcome, SourceRootCatalogOperationError> {
    if let Some(primary_root_id) = requested_primary_root {
        let roots = catalog
            .list_roots()
            .await
            .map_err(SourceRootCatalogOperationError::from)?;
        resolve_enabled_source_root_by_id(
            &roots,
            primary_root_id,
            |root| Some(root.root_id.as_str()),
            |root| root.enabled,
        )
        .map_err(SourceRootCatalogOperationError::SourceRootLookup)?;
    }
    let changed = catalog
        .set_primary_root(requested_primary_root)
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    Ok(source_root_primary_outcome(
        requested_primary_root.is_some(),
        changed,
    ))
}

/// 校验单根重新索引请求；仅允许当前 catalog 中已启用的数据根。
pub async fn source_root_reindex_request(
    catalog: &impl SourceRootCatalog,
    root_id: &str,
) -> Result<SourceRootReindexRequest, SourceRootCatalogOperationError> {
    let roots = catalog
        .list_roots()
        .await
        .map_err(SourceRootCatalogOperationError::from)?;
    resolve_enabled_source_root_by_id(
        &roots,
        root_id,
        |root| Some(root.root_id.as_str()),
        |root| root.enabled,
    )
    .map_err(SourceRootCatalogOperationError::SourceRootLookup)?;
    Ok(SourceRootReindexRequest {
        root_id: root_id.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 内存 catalog，仅用于验证 async helper 的业务前置校验与 outcome。
    struct MemoryCatalog {
        roots: HashMap<String, bool>,
        primary: Option<String>,
    }

    impl SourceRootCatalog for MemoryCatalog {
        /// 列出内存目录中的全部测试数据根。
        fn list_roots(
            &self,
        ) -> CatalogFuture<'_, Result<Vec<SourceRootCatalogRecord>, SourceRootCatalogError>>
        {
            let roots = self
                .roots
                .iter()
                .map(|(root_id, enabled)| SourceRootCatalogRecord {
                    root_id: root_id.clone(),
                    enabled: *enabled,
                })
                .collect();
            Box::pin(async move { Ok(roots) })
        }

        /// 在内存目录中仅登记尚不存在的测试根。
        fn register_root_if_new(
            &mut self,
            candidate: &SourceRootCandidate,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            let inserted = self.roots.insert(candidate.root_id.clone(), true).is_none();
            Box::pin(async move { Ok(inserted) })
        }

        /// 更新内存测试根的启用状态并报告是否变化。
        fn set_root_enabled(
            &mut self,
            root_id: &str,
            enabled: bool,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            let changed = self
                .roots
                .get_mut(root_id)
                .map(|current| {
                    let changed = *current != enabled;
                    *current = enabled;
                    changed
                })
                .unwrap_or(false);
            Box::pin(async move { Ok(changed) })
        }

        /// 模拟别名更新成功，测试不持久化展示文本。
        fn set_root_alias(
            &mut self,
            _root_id: &str,
            _alias: &str,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            Box::pin(async { Ok(true) })
        }

        /// 从内存目录移除测试根并同步清理主根引用。
        fn remove_root(
            &mut self,
            root_id: &str,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            let removed = self.roots.remove(root_id).is_some();
            if self.primary.as_deref() == Some(root_id) {
                self.primary = None;
            }
            Box::pin(async move { Ok(removed) })
        }

        /// 更新内存目录中的唯一主根并报告是否变化。
        fn set_primary_root(
            &mut self,
            root_id: Option<&str>,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            let next = root_id.map(str::to_owned);
            let changed = self.primary != next;
            self.primary = next;
            Box::pin(async move { Ok(changed) })
        }
    }

    /// 新增成功与重复登记分别映射到正确 outcome。
    #[tokio::test]
    async fn source_root_add_reports_added_or_already_present() {
        let mut catalog = MemoryCatalog {
            roots: HashMap::new(),
            primary: None,
        };
        let candidate = SourceRootCandidate {
            root_id: "root-a".into(),
            alias: "A".into(),
            path: PathBuf::from("/tmp/a"),
        };
        let first = source_root_add(&mut catalog, &candidate)
            .await
            .expect("add succeeds");
        assert!(first.changed);
        let second = source_root_add(&mut catalog, &candidate)
            .await
            .expect("duplicate is not an error");
        assert!(!second.changed);
    }

    /// 主目录只能设到已启用根；未知 ID 被 lookup 拒绝。
    #[tokio::test]
    async fn source_root_set_primary_requires_enabled_root() {
        let mut catalog = MemoryCatalog {
            roots: HashMap::from([("root-a".into(), false), ("root-b".into(), true)]),
            primary: None,
        };
        let disabled = source_root_set_primary(&mut catalog, Some("root-a")).await;
        assert!(matches!(
            disabled,
            Err(SourceRootCatalogOperationError::SourceRootLookup(_))
        ));
        let enabled = source_root_set_primary(&mut catalog, Some("root-b"))
            .await
            .expect("enabled root becomes primary");
        assert!(enabled.changed);
    }

    /// 单根重新索引请求只携带已存在且已启用的数据根稳定 ID。
    #[tokio::test]
    async fn source_root_reindex_request_requires_enabled_root() {
        let catalog = MemoryCatalog {
            roots: HashMap::from([("root-a".into(), false), ("root-b".into(), true)]),
            primary: None,
        };

        let request = source_root_reindex_request(&catalog, "root-b")
            .await
            .expect("enabled root may be reindexed");
        assert_eq!(request.root_id(), "root-b");
        assert_eq!(
            source_root_reindex_request(&catalog, "root-a").await,
            Err(SourceRootCatalogOperationError::SourceRootLookup(
                SourceRootLookupError::NotEnabled,
            ))
        );
        assert_eq!(
            source_root_reindex_request(&catalog, "root-missing").await,
            Err(SourceRootCatalogOperationError::SourceRootLookup(
                SourceRootLookupError::Missing,
            ))
        );
    }
}
