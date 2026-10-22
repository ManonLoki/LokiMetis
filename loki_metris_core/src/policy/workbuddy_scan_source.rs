//! WorkBuddy 扫描数据源策略：是否进入扫描集合、根 ID/别名与本机目录发现。
//! 文件系统探测只依赖调用方注入的用户主目录，便于用临时目录覆盖开启/关闭/数据目录缺失。
//!
//! WorkBuddy 国内版与国际版使用两个互不相同的固定数据目录，但共同构成同一个
//! 可选逻辑来源；本模块只负责按稳定顺序推导这两个候选根，不负责合并用量。

use std::path::{Path, PathBuf};

use crate::{SourceClientKind, path_key, source_root_alias_from_path, stable_id};

/// WorkBuddy 固定数据目录名，位于当前用户主目录下。
pub const WORKBUDDY_HOME_DIR_NAME: &str = ".workbuddy";
/// WorkBuddy 国际版固定数据目录名，位于当前用户主目录下。
pub const WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME: &str = ".workbuddy-ai";
/// WorkBuddy 逐请求项目记录固定目录名，用作可统计结构证据。
pub const WORKBUDDY_PROJECTS_DIR_NAME: &str = "projects";

/// 一个可发现或登记的 WorkBuddy 扫描数据根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkBuddyScanSource {
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

/// 按稳定顺序返回主目录下真实存在的 WorkBuddy 普通数据目录；两个都缺失时返回空。
///
/// 单一版本缺失是正常情况，不构成失败；链接或非目录一律跳过，避免把其它目录
/// 误标成 WorkBuddy。国内版与国际版都不会在此处合并用量，只提供只读来源身份。
pub fn discover_workbuddy_scan_sources(user_home: &Path) -> Vec<WorkBuddyScanSource> {
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

/// 按稳定命名空间从路径生成 WorkBuddy 扫描源身份。
fn workbuddy_scan_source_from_path(path: PathBuf) -> WorkBuddyScanSource {
    let client = SourceClientKind::WorkBuddy;
    WorkBuddyScanSource {
        root_id: stable_id(client.root_id_namespace(), &path_key(&path)),
        alias: source_root_alias_from_path(&path, client),
        path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    /// 存在的国内版数据目录生成稳定命名空间根身份，路径与固定目录一致。
    #[test]
    fn installed_workbuddy_home_is_discovered_with_stable_identity() {
        let home = install_workbuddy_home();
        let discovered = discover_workbuddy_scan_sources(home.path());
        assert_eq!(discovered.len(), 1);
        assert!(discovered[0].root_id.starts_with("workbuddy-root-"));
        assert_eq!(discovered[0].alias, WORKBUDDY_HOME_DIR_NAME);
        assert_eq!(discovered[0].path, workbuddy_home_from_user_home(home.path()));
    }

    /// 两版数据目录同时存在时按国内版、国际版稳定顺序返回且身份互不相同。
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
    }

    /// 主目录下没有任何 WorkBuddy 数据目录时不发现来源。
    #[test]
    fn missing_workbuddy_home_is_not_discovered() {
        let home = tempfile::tempdir().expect("empty home");
        fs::create_dir(home.path().join(".codex")).expect("codex home");
        assert!(discover_workbuddy_scan_sources(home.path()).is_empty());
    }

    /// 仅国际版数据目录存在时也必须被发现，单一版本缺失不得让整个来源消失。
    #[test]
    fn international_only_installation_is_discovered() {
        let home = tempfile::tempdir().expect("temp home");
        fs::create_dir(home.path().join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME))
            .expect("workbuddy international home");
        let discovered = discover_workbuddy_scan_sources(home.path());
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].alias, WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME);
    }

    /// 候选根为链接或普通文件时不得被当成 WorkBuddy 数据目录。
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
