//! WorkBuddy 扫描数据源策略：是否进入扫描集合、根 ID/别名与本机目录发现。
//! 文件系统探测只依赖调用方注入的用户主目录，便于用临时目录覆盖开启/关闭/未安装。
//!
//! WorkBuddy 国内版与国际版使用两个互不相同的固定安装目录，但共同构成同一个
//! 可选逻辑来源；本模块只负责按稳定顺序推导这两个候选根，不负责合并用量。

use std::path::{Path, PathBuf};

use crate::settings::EnabledAgents;
use crate::source_root::SourceRootCandidate;
use crate::{SourceClientKind, path_key, source_root_alias_from_path, stable_id};

/// WorkBuddy 固定安装目录名，位于当前用户主目录下。
pub const WORKBUDDY_HOME_DIR_NAME: &str = ".workbuddy";
/// WorkBuddy 国际版固定安装目录名，位于当前用户主目录下。
pub const WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME: &str = ".workbuddy-ai";
/// WorkBuddy 逐请求项目记录固定目录名，用作可统计结构证据。
pub const WORKBUDDY_PROJECTS_DIR_NAME: &str = "projects";

/// 已批准的两个固定 WorkBuddy 安装目录名，按国内版、国际版稳定顺序排列。
pub const WORKBUDDY_HOME_DIR_NAMES: [&str; 2] = [
    WORKBUDDY_HOME_DIR_NAME,
    WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME,
];

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

/// 由用户主目录推导 WorkBuddy 国内版本机根，不探测目录是否存在。
pub fn workbuddy_home_from_user_home(user_home: &Path) -> PathBuf {
    user_home.join(WORKBUDDY_HOME_DIR_NAME)
}

/// 由用户主目录按稳定顺序推导国内版与国际版两个固定候选根，不探测文件系统。
pub fn workbuddy_homes_from_user_home(user_home: &Path) -> [PathBuf; 2] {
    [
        workbuddy_home_from_user_home(user_home),
        user_home.join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME),
    ]
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

/// 按稳定顺序返回主目录下真实存在的 WorkBuddy 普通目录；两个都未安装时返回空。
///
/// 单一版本缺失是正常情况，不构成失败；链接或非目录一律跳过，避免把其它目录
/// 误标成 WorkBuddy。国内版与国际版都不会在此处合并用量，只提供只读来源身份。
pub fn discover_workbuddy_scan_sources(user_home: &Path) -> Vec<WorkbuddyScanSource> {
    workbuddy_homes_from_user_home(user_home)
        .into_iter()
        .filter(|path| is_workbuddy_home_directory(path))
        .map(workbuddy_scan_source_from_path)
        .collect()
}

/// 判断候选路径是否是可直接读取的普通目录；链接、文件与不存在都返回 false。
fn is_workbuddy_home_directory(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    !metadata.file_type().is_symlink() && metadata.is_dir()
}

/// 开关关闭时不返回扫描源，即使本机根已经存在，避免把其它目录误标成 WorkBuddy。
pub fn list_workbuddy_scan_sources(
    workbuddy_stats_enabled: bool,
    user_home: &Path,
) -> Vec<WorkbuddyScanSource> {
    if !workbuddy_stats_enabled {
        return Vec::new();
    }
    discover_workbuddy_scan_sources(user_home)
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

    /// 在临时主目录下创建国内版 WorkBuddy 普通目录。
    fn install_workbuddy_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("temp home");
        fs::create_dir(home.path().join(WORKBUDDY_HOME_DIR_NAME)).expect("workbuddy home");
        home
    }

    /// 在临时主目录下同时创建国内版与国际版 WorkBuddy 普通目录。
    fn install_both_workbuddy_homes() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("temp home");
        fs::create_dir(home.path().join(WORKBUDDY_HOME_DIR_NAME)).expect("workbuddy home");
        fs::create_dir(home.path().join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME))
            .expect("workbuddy international home");
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
        assert!(listed[0].root_id.starts_with("workbuddy-root-"));
        assert_eq!(listed[0].alias, WORKBUDDY_HOME_DIR_NAME);
        assert_eq!(listed[0].path, workbuddy_home_from_user_home(home.path()));
        let mut catalog = MemoryCatalog::default();
        source_root_add(&mut catalog, &workbuddy_scan_source_candidate(&listed[0]))
            .await
            .expect("register");
        let roots = catalog.list_roots().await.expect("list");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root_id, listed[0].root_id);
        assert!(roots[0].enabled);
    }

    /// 国内版与国际版同时安装时必须按稳定顺序返回两个互不冲突的只读来源。
    #[test]
    fn both_editions_are_discovered_in_stable_order() {
        let home = install_both_workbuddy_homes();
        let discovered = discover_workbuddy_scan_sources(home.path());
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].alias, WORKBUDDY_HOME_DIR_NAME);
        assert_eq!(discovered[1].alias, WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME);
        assert_eq!(
            discovered[0].path,
            home.path().join(WORKBUDDY_HOME_DIR_NAME)
        );
        assert_eq!(
            discovered[1].path,
            home.path().join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME)
        );
        assert_ne!(discovered[0].root_id, discovered[1].root_id);
        assert_eq!(list_workbuddy_scan_sources(true, home.path()).len(), 2);
    }

    /// 只安装国际版时也必须被发现，单一版本缺失不得让整个来源消失。
    #[test]
    fn international_only_installation_is_discovered() {
        let home = tempfile::tempdir().expect("temp home");
        fs::create_dir(home.path().join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME))
            .expect("workbuddy international home");
        let discovered = discover_workbuddy_scan_sources(home.path());
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].alias, WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME);
    }

    /// 关闭时即使本机根存在也不得进入扫描源集合或列举结果。
    #[test]
    fn disabled_workbuddy_is_absent_even_when_installed() {
        let home = install_workbuddy_home();
        assert!(list_scan_source_clients(EnabledAgents::empty(), false).is_empty());
        assert!(list_workbuddy_scan_sources(false, home.path()).is_empty());
        assert_eq!(discover_workbuddy_scan_sources(home.path()).len(), 1);
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
        assert!(discover_workbuddy_scan_sources(home.path()).is_empty());
        assert!(list_workbuddy_scan_sources(true, home.path()).is_empty());
    }

    /// 候选根为链接或普通文件时不得被当成 WorkBuddy 安装目录。
    #[test]
    fn non_directory_candidates_are_rejected() {
        let home = tempfile::tempdir().expect("temp home");
        fs::write(
            home.path().join(WORKBUDDY_HOME_DIR_NAME),
            b"not a directory",
        )
        .expect("plain file candidate");
        assert!(discover_workbuddy_scan_sources(home.path()).is_empty());
    }
}
