//! 把 GUI 专属的 [`DiscoveredRoot`]/[`ClaudeDiscoveredRoot`] 适配到
//! `core::LocalIndex` 的原始字段存储原语。
//!
//! `LocalIndex` 现在定义在 `loki_metis_core`，Rust 不允许
//! 跨 crate 为外部类型追加固有方法（inherent impl），所以这里改用扩展
//! trait：`impl RegisterDiscoveredRoot for LocalIndex` 让调用方仍然可以
//! 写 `index.register_root(&root).await`，方法调用语法在迁移前后保持一致。

use std::path::{Path, PathBuf};

use loki_metis_core::{DiscoveryMethod, LocalError, LocalErrorKind, LocalIndex};
use tauri::async_runtime::spawn_blocking;

use super::claude::ClaudeDiscoveredRoot;
use super::discovery::{
    DiscoveredRoot, registered_path_matches_candidate, validate_local_plain_directory,
};
use super::grok::GrokDiscoveredRoot;

/// Codex 与 Claude 发现结果共享的四个身份字段；写入存储层时不需要关心
/// 候选具体是哪一种发现结果类型，只需要能读出这四个字段。
trait RootIdentityFields {
    /// 内容无关、按规范化访问位置生成的稳定根 ID。
    fn root_id(&self) -> &str;
    /// adapter 内部用于只读访问和 registry 持久化的路径。
    fn path(&self) -> &Path;
    /// 默认供 GUI 展示的安全别名。
    fn alias(&self) -> &str;
    /// 根的发现或登记方式。
    fn discovery_method(&self) -> DiscoveryMethod;
}

impl RootIdentityFields for DiscoveredRoot {
    /// 返回该 Codex 发现根的稳定根 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 返回该 Codex 发现根的访问路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Codex 发现根的安全展示别名。
    fn alias(&self) -> &str {
        &self.alias
    }

    /// 返回该 Codex 发现根的发现方式。
    fn discovery_method(&self) -> DiscoveryMethod {
        self.discovery_method
    }
}

impl RootIdentityFields for GrokDiscoveredRoot {
    /// 返回该 Grok 发现根的稳定根 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 返回该 Grok 发现根的访问路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Grok 发现根的安全展示别名。
    fn alias(&self) -> &str {
        &self.alias
    }

    /// 返回该 Grok 发现根的发现方式。
    fn discovery_method(&self) -> DiscoveryMethod {
        self.discovery_method
    }
}

impl RootIdentityFields for ClaudeDiscoveredRoot {
    /// 返回该 Claude 发现根的稳定根 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 返回该 Claude 发现根的访问路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Claude 发现根的安全展示别名。
    fn alias(&self) -> &str {
        &self.alias
    }

    /// 返回该 Claude 发现根的发现方式。
    fn discovery_method(&self) -> DiscoveryMethod {
        self.discovery_method
    }
}

/// 为 `LocalIndex` 补充“把已确认数据根登记到存储层”的 GUI 专属方法。
pub(crate) trait RegisterDiscoveredRoot {
    /// 把已确认数据根登记到索引；访问路径只保留在本产品数据库内部。
    async fn register_root(&mut self, root: &DiscoveredRoot) -> Result<(), LocalError>;

    /// 仅在当前客户端索引尚无同一物理目录时登记 Codex 根，并返回是否发生写入。
    async fn register_root_if_new(&mut self, root: &DiscoveredRoot) -> Result<bool, LocalError>;

    /// 把已通过 Claude transcript 签名的根登记在当前客户端私有索引。
    async fn register_claude_root(&mut self, root: &ClaudeDiscoveredRoot)
    -> Result<(), LocalError>;

    /// 仅在当前客户端索引尚无同一物理目录时登记 Claude Code 根，并返回是否发生写入。
    async fn register_claude_root_if_new(
        &mut self,
        root: &ClaudeDiscoveredRoot,
    ) -> Result<bool, LocalError>;

    /// 把已通过 Grok 会话签名的根登记在当前客户端私有索引。
    async fn register_grok_root(&mut self, root: &GrokDiscoveredRoot) -> Result<(), LocalError>;

    /// 仅在当前客户端索引尚无同一物理目录时登记 Grok 根。
    async fn register_grok_root_if_new(
        &mut self,
        root: &GrokDiscoveredRoot,
    ) -> Result<bool, LocalError>;
}

impl RegisterDiscoveredRoot for LocalIndex {
    /// 无条件登记（或更新）一个 Codex 根。
    async fn register_root(&mut self, root: &DiscoveredRoot) -> Result<(), LocalError> {
        register(self, root).await
    }

    /// 仅当不存在同一物理目录时登记 Codex 根。
    async fn register_root_if_new(&mut self, root: &DiscoveredRoot) -> Result<bool, LocalError> {
        register_if_new(self, root).await
    }

    /// 无条件登记（或更新）一个 Claude 根。
    async fn register_claude_root(
        &mut self,
        root: &ClaudeDiscoveredRoot,
    ) -> Result<(), LocalError> {
        register(self, root).await
    }

    /// 仅当不存在同一物理目录时登记 Claude 根。
    async fn register_claude_root_if_new(
        &mut self,
        root: &ClaudeDiscoveredRoot,
    ) -> Result<bool, LocalError> {
        register_if_new(self, root).await
    }

    /// 无条件登记（或更新）一个 Grok 根。
    async fn register_grok_root(&mut self, root: &GrokDiscoveredRoot) -> Result<(), LocalError> {
        register(self, root).await
    }

    /// 仅当不存在同一物理目录时登记 Grok 根。
    async fn register_grok_root_if_new(
        &mut self,
        root: &GrokDiscoveredRoot,
    ) -> Result<bool, LocalError> {
        register_if_new(self, root).await
    }
}

/// 把已确认数据根登记到索引；访问路径只保留在本产品数据库内部。
async fn register(
    index: &mut LocalIndex,
    root: &impl RootIdentityFields,
) -> Result<(), LocalError> {
    index
        .register_root_fields(
            root.root_id(),
            root.path(),
            root.alias(),
            root.discovery_method(),
        )
        .await
}

/// FS 去重岛的判定结果；写库仍在 async 侧执行。
enum DedupDecision {
    /// 候选与某个已登记路径指向同一物理目录，跳过写入。
    Duplicate,
    /// 候选生成的根 ID 与已登记项冲突，但不是同一物理目录。
    Collision,
    /// 候选未与任何已登记项重复，可以安全写入。
    New,
}

/// 在写入前做一次“同一物理目录”模糊去重；去重检查与最终写入不在同一个
/// 数据库事务里（那部分模糊比较依赖平台卷分类，只能留在 GUI），中间存在
/// 极窄的竞态窗口。最坏后果只是手动添加时多出一行重复登记的根，用户可以
/// 随时移除，不是数据损坏或安全问题——见
/// `loki_metis_core::local_index` 里 `register_root_fields`
/// 的模块文档。
async fn register_if_new(
    index: &mut LocalIndex,
    root: &impl RootIdentityFields,
) -> Result<bool, LocalError> {
    let registered = index.registered_root_paths().await?;
    let candidate = root.path().to_path_buf();
    let root_id_owned = root.root_id().to_owned();
    let decision = spawn_blocking(move || {
        dedup_candidate_against_registered(&candidate, &root_id_owned, &registered)
    })
    .await
    .map_err(|_| {
        LocalError::new(
            LocalErrorKind::SourceUnavailable,
            "local source operation failed",
        )
    })??;
    match decision {
        DedupDecision::Duplicate => Ok(false),
        DedupDecision::Collision => Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "root identity collides with another directory",
        )),
        DedupDecision::New => {
            index
                .register_root_fields(
                    root.root_id(),
                    root.path(),
                    root.alias(),
                    root.discovery_method(),
                )
                .await?;
            Ok(true)
        }
    }
}

/// 在 blocking 岛内校验候选并与已登记路径做物理去重。
fn dedup_candidate_against_registered(
    candidate: &Path,
    root_id: &str,
    registered: &[(String, PathBuf)],
) -> Result<DedupDecision, LocalError> {
    validate_local_plain_directory(candidate)?;
    for (registered_id, registered_path) in registered {
        if registered_path_matches_candidate(registered_path, candidate)? {
            return Ok(DedupDecision::Duplicate);
        }
        if registered_id == root_id {
            return Ok(DedupDecision::Collision);
        }
    }
    Ok(DedupDecision::New)
}
