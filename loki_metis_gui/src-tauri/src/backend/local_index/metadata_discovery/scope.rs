//! 定义当前开发版的扫描范围与高优先级入队顺序。

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::{RootDiscoveryPlatform, RootDiscoveryScope};

use super::super::{
    LocalPathStatus, LocalVolumeRoots, classify_local_path, current_user_home,
    enumerate_local_volume_roots, metadata_is_link_like, validate_local_plain_directory,
};

/// 返回当前编译目标对应的数据源发现平台。
pub(super) const fn current_platform() -> RootDiscoveryPlatform {
    if cfg!(target_os = "windows") {
        RootDiscoveryPlatform::Windows
    } else if cfg!(target_os = "macos") {
        RootDiscoveryPlatform::MacOs
    } else {
        RootDiscoveryPlatform::Other
    }
}

/// 根据用户选择返回快速优先目录或全部已确认本地卷。
///
/// 快速扫描（`UserPriority`）的搜索根仅为平台优先相对目录，不包含用户主目录本身，
/// 因此不会遍历 Desktop、Downloads 等优先列表之外的路径。
pub(super) fn discovery_scope(scope: RootDiscoveryScope) -> LocalVolumeRoots {
    if scope == RootDiscoveryScope::FullLocalVolumes {
        return enumerate_local_volume_roots();
    }
    local_priority_scope(platform_priority_roots())
}

/// 在读取任何目录项前，把用户优先根限制为已确认本地、非链接的现存目录。
/// 缺失的可选优先目录是普通状态；网络、未知卷或链接则计入保守跳过。
pub(super) fn local_priority_scope(roots: Vec<PathBuf>) -> LocalVolumeRoots {
    let mut result = LocalVolumeRoots::default();
    for root in roots {
        match classify_local_path(&root) {
            LocalPathStatus::ConfirmedLocal => match validate_local_plain_directory(&root) {
                Ok(()) => result.search_roots.push(root),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    result.other_skipped_count = result.other_skipped_count.saturating_add(1);
                    result.excluded_roots.push(root);
                }
            },
            LocalPathStatus::RejectedNetwork => {
                result.network_skipped_count = result.network_skipped_count.saturating_add(1);
                result.excluded_roots.push(root);
            }
            LocalPathStatus::Missing => {}
            LocalPathStatus::Indeterminate => {
                result.other_skipped_count = result.other_skipped_count.saturating_add(1);
                result.excluded_roots.push(root);
            }
        }
    }
    result
}

/// 返回相对当前用户根的平台优先目录；快速扫描只以这些路径为搜索根。
pub(super) fn platform_priority_roots() -> Vec<PathBuf> {
    current_user_home()
        .map(platform_default_roots)
        .unwrap_or_default()
}

/// 返回相对 OS 用户根的平台固定优先目录。
fn platform_default_roots(root: PathBuf) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        [
            "AppData/Roaming/AIManagerData",
            ".claude",
            ".codex",
            "AppData/Roaming",
            "AppData/Local",
        ]
        .into_iter()
        .map(|relative| root.join(relative))
        .collect()
    }
    #[cfg(target_os = "macos")]
    {
        [
            "Library/Application Support/AIManagerData",
            ".claude",
            ".codex",
            "Library/Application Support",
            "Library",
        ]
        .into_iter()
        .map(|relative| root.join(relative))
        .collect()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    Vec::new()
}

/// 优先目录预先标记为已入队，后续宽遍历不会重复扫描它们。
pub(super) fn priority_queue(root: &Path, priority_roots: &[PathBuf]) -> VecDeque<PathBuf> {
    priority_roots
        .iter()
        .filter(|path| path.starts_with(root))
        .filter(|path| path.as_path() != root)
        .filter(|path| {
            fs::symlink_metadata(path)
                .is_ok_and(|metadata| metadata.is_dir() && !metadata_is_link_like(&metadata))
        })
        .cloned()
        .chain(std::iter::once(root.to_path_buf()))
        .collect()
}
