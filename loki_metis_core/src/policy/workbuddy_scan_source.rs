//! WorkBuddy 扫描数据源策略：是否进入扫描集合、根 ID/别名与本机目录发现。
//! 文件系统探测只依赖调用方注入的用户主目录，便于用临时目录覆盖开启/关闭/未安装。

use std::path::{Path, PathBuf};

use crate::settings::EnabledAgents;
use crate::source_root::SourceRootCandidate;
use crate::{SourceClientKind, path_key, source_root_alias_from_path, stable_id};

/// WorkBuddy 固定安装目录名，位于当前用户主目录下。
pub const WORKBUDDY_HOME_DIR_NAME: &str = ".workbuddy";
/// WorkBuddy 逐请求项目记录固定目录名，用作可统计结构证据。
pub const WORKBUDDY_PROJECTS_DIR_NAME: &str = "projects";

/// 一个可发现或登记的 WorkBuddy 扫描数据根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbuddyScanSource {
    /// 稳定根 ID，前缀为 `workbuddy-root-`。
    pub root_id: String,
    /// 安全展示别名，不含绝对路径。
    pub alias: String,
    /// adapter 内部只读访问路径。
    pub path: PathBuf,
}

/// 由用户主目录推导 WorkBuddy 本机根，不探测目录是否存在。
pub fn workbuddy_home_from_user_home(user_home: &Path) -> PathBuf {
    user_home.join(WORKBUDDY_HOME_DIR_NAME)
}

/// 返回周期扫描与数据源列举应包含的客户端；WorkBuddy 只在独立开关开启时加入。
pub fn list_scan_source_clients(
    enabled_agents: EnabledAgents,
    workbuddy_stats_enabled: bool,
) -> Vec<SourceClientKind> {
    let mut clients: Vec<SourceClientKind> = enabled_agents.iter().collect();
    if workbuddy_stats_enabled {
        clients.push(SourceClientKind::WorkBuddy);
    }
    clients
}

/// 主目录下存在 `.workbuddy` 普通目录时返回扫描数据源；未安装返回 `None`。
pub fn discover_workbuddy_scan_source(user_home: &Path) -> Option<WorkbuddyScanSource> {
    let path = workbuddy_home_from_user_home(user_home);
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return None;
    }
    Some(workbuddy_scan_source_from_path(path))
}

/// 开关关闭时不返回扫描源，即使本机根已经存在，避免把其它目录误标成 WorkBuddy。
pub fn list_workbuddy_scan_sources(
    workbuddy_stats_enabled: bool,
    user_home: &Path,
) -> Vec<WorkbuddyScanSource> {
    if !workbuddy_stats_enabled {
        return Vec::new();
    }
    discover_workbuddy_scan_source(user_home)
        .into_iter()
        .collect()
}

/// 把已发现的 WorkBuddy 根转成 catalog 可登记候选。
pub fn workbuddy_scan_source_candidate(source: &WorkbuddyScanSource) -> SourceRootCandidate {
    SourceRootCandidate {
        root_id: source.root_id.clone(),
        alias: source.alias.clone(),
        path: source.path.clone(),
    }
}

/// 按稳定命名空间从路径生成 WorkBuddy 扫描源身份。
fn workbuddy_scan_source_from_path(path: PathBuf) -> WorkbuddyScanSource {
    let client = SourceClientKind::WorkBuddy;
    WorkbuddyScanSource {
        root_id: stable_id(client.root_id_namespace(), &path_key(&path)),
        alias: source_root_alias_from_path(&path, client),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_root::{
        CatalogFuture, SourceRootCatalog, SourceRootCatalogError, SourceRootCatalogRecord,
        source_root_add,
    };
    use std::collections::HashMap;
    use std::fs;

    /// 内存 catalog，只覆盖登记/列举，供扫描源登记入口测试。
    struct MemoryCatalog {
        roots: HashMap<String, bool>,
    }

    impl Default for MemoryCatalog {
        /// 构造空的内存 catalog。
        fn default() -> Self {
            Self {
                roots: HashMap::new(),
            }
        }
    }

    impl SourceRootCatalog for MemoryCatalog {
        /// 列出已登记根。
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

        /// 仅在根 ID 尚未存在时登记。
        fn register_root_if_new(
            &mut self,
            candidate: &SourceRootCandidate,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            let inserted = self.roots.insert(candidate.root_id.clone(), true).is_none();
            Box::pin(async move { Ok(inserted) })
        }

        /// 测试不覆盖启停。
        fn set_root_enabled(
            &mut self,
            _root_id: &str,
            _enabled: bool,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            Box::pin(async move { Ok(false) })
        }

        /// 测试不覆盖改名。
        fn set_root_alias(
            &mut self,
            _root_id: &str,
            _alias: &str,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            Box::pin(async move { Ok(false) })
        }

        /// 测试不覆盖删除。
        fn remove_root(
            &mut self,
            _root_id: &str,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            Box::pin(async move { Ok(false) })
        }

        /// 测试不覆盖主目录。
        fn set_primary_root(
            &mut self,
            _root_id: Option<&str>,
        ) -> CatalogFuture<'_, Result<bool, SourceRootCatalogError>> {
            Box::pin(async move { Ok(false) })
        }
    }

    /// 在临时主目录下创建 `.workbuddy` 普通目录。
    fn install_workbuddy_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("temp home");
        fs::create_dir(home.path().join(WORKBUDDY_HOME_DIR_NAME)).expect("workbuddy home");
        home
    }

    /// 开启且本机根存在时，扫描源集合必须含 WorkBuddy，发现结果可登记。
    #[tokio::test]
    async fn enabled_installed_workbuddy_is_a_scan_source_and_can_register() {
        let home = install_workbuddy_home();
        let clients = list_scan_source_clients(EnabledAgents::empty(), true);
        assert_eq!(clients, vec![SourceClientKind::WorkBuddy]);
        let listed = list_workbuddy_scan_sources(true, home.path());
        assert_eq!(listed.len(), 1);
        let discovered = discover_workbuddy_scan_source(home.path()).expect("installed root");
        assert_eq!(listed[0], discovered);
        assert!(discovered.root_id.starts_with("workbuddy-root-"));
        assert_eq!(discovered.alias, WORKBUDDY_HOME_DIR_NAME);
        assert_eq!(discovered.path, workbuddy_home_from_user_home(home.path()));
        let mut catalog = MemoryCatalog::default();
        source_root_add(&mut catalog, &workbuddy_scan_source_candidate(&discovered))
            .await
            .expect("register");
        let roots = catalog.list_roots().await.expect("list");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root_id, discovered.root_id);
        assert!(roots[0].enabled);
    }

    /// 关闭时即使本机根存在也不得进入扫描源集合或列举结果。
    #[test]
    fn disabled_workbuddy_is_absent_even_when_installed() {
        let home = install_workbuddy_home();
        assert!(list_scan_source_clients(EnabledAgents::empty(), false).is_empty());
        assert!(list_workbuddy_scan_sources(false, home.path()).is_empty());
        assert!(discover_workbuddy_scan_source(home.path()).is_some());
    }

    /// 未安装时开启开关也不得发明扫描根，也不得把其它 Agent 根冒充 WorkBuddy。
    #[test]
    fn missing_workbuddy_home_is_not_discovered() {
        let home = tempfile::tempdir().expect("empty home");
        fs::create_dir(home.path().join(".codex")).expect("codex home");
        assert!(
            list_scan_source_clients(
                EnabledAgents::empty().with(SourceClientKind::Codex, true),
                true
            )
            .contains(&SourceClientKind::WorkBuddy)
        );
        assert!(discover_workbuddy_scan_source(home.path()).is_none());
        assert!(list_workbuddy_scan_sources(true, home.path()).is_empty());
    }
}
