//! macOS Claude Desktop 内嵌 Claude Code 数据根的固定布局发现。

use std::path::{Path, PathBuf};

use super::{CancellationToken, Candidate, DiscoveryMethod};

/// 内嵌根枚举结果；不携带 transcript 内容。
#[derive(Default)]
pub(super) struct EmbeddedCandidates {
    pub(super) candidates: Vec<Candidate>,
    pub(super) quality: EnumerationQuality,
}

/// 固定容器枚举中影响覆盖结论的计数。
#[derive(Default)]
pub(super) struct EnumerationQuality {
    pub(super) directories_scanned: u64,
    pub(super) permission_denied_count: u64,
    pub(super) skipped_count: u64,
    pub(super) symlink_skipped_count: u64,
    pub(super) network_skipped_count: u64,
    pub(super) budget_exhausted: bool,
    pub(super) cancelled: bool,
}

/// 非 macOS 平台没有此固定宿主容器。
#[cfg(not(target_os = "macos"))]
pub(super) fn candidates(
    _home_dir: Option<&Path>,
    _cancellation: &CancellationToken,
) -> EmbeddedCandidates {
    EmbeddedCandidates::default()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::fs;

    use super::*;
    use crate::backend::local_index::claude::path_rules::is_uuid;
    use crate::backend::local_index::discovery::{
        metadata_is_link_like, reject_symlink_components,
    };
    use crate::backend::local_index::{
        LocalPathStatus, classify_local_path, is_obviously_network_path,
    };

    /// 三层动态目录的总目录项、已打开目录和最终候选数上限。
    const MAX_ENTRIES: u64 = 4_096;
    const MAX_DIRECTORIES: u64 = 512;
    const MAX_CANDIDATES: usize = 256;

    /// 只枚举 `local-agent-mode-sessions/<UUID>/<UUID>/local_<UUID>/.claude`。
    pub(super) fn candidates(
        home_dir: Option<&Path>,
        cancellation: &CancellationToken,
    ) -> EmbeddedCandidates {
        candidates_with_classifier(home_dir, cancellation, &classify_local_path)
    }

    /// 分类器可由测试注入，确保每层挂载边界在下钻之前受检。
    fn candidates_with_classifier(
        home_dir: Option<&Path>,
        cancellation: &CancellationToken,
        classify: &impl Fn(&Path) -> LocalPathStatus,
    ) -> EmbeddedCandidates {
        let mut result = EmbeddedCandidates::default();
        let Some(home_dir) = home_dir else {
            return result;
        };
        let container = home_dir
            .join("Library")
            .join("Application Support")
            .join("Claude")
            .join("local-agent-mode-sessions");
        if is_obviously_network_path(&container) {
            result.quality.network_skipped_count = 1;
            result.quality.skipped_count = 1;
            return result;
        }
        match reject_symlink_components(&container) {
            Ok(true) => {
                result.quality.symlink_skipped_count = 1;
                result.quality.skipped_count = 1;
                return result;
            }
            Ok(false) => {}
            Err(error) => {
                record_error(&mut result.quality, &error);
                return result;
            }
        }
        match classify(&container) {
            LocalPathStatus::Missing => return result,
            LocalPathStatus::ConfirmedLocal => {}
            LocalPathStatus::RejectedNetwork => {
                result.quality.network_skipped_count = 1;
                result.quality.skipped_count = 1;
                return result;
            }
            LocalPathStatus::Indeterminate => {
                result.quality.skipped_count = 1;
                return result;
            }
        }
        let mut remaining_entries = MAX_ENTRIES;
        visit_level(
            &container,
            0,
            cancellation,
            &mut remaining_entries,
            &mut result,
            classify,
        );
        result
    }

    /// 只下钻三层符合官方容器名称的普通目录，绝不搜索任意后代。
    fn visit_level(
        path: &Path,
        level: u8,
        cancellation: &CancellationToken,
        remaining_entries: &mut u64,
        result: &mut EmbeddedCandidates,
        classify: &impl Fn(&Path) -> LocalPathStatus,
    ) {
        if cancellation.is_cancelled() {
            result.quality.cancelled = true;
            return;
        }
        if result.quality.directories_scanned >= MAX_DIRECTORIES {
            result.quality.budget_exhausted = true;
            return;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                record_error(&mut result.quality, &error);
                return;
            }
        };
        result.quality.directories_scanned += 1;
        for entry in entries {
            if cancellation.is_cancelled() {
                result.quality.cancelled = true;
                return;
            }
            if *remaining_entries == 0 {
                result.quality.budget_exhausted = true;
                return;
            }
            *remaining_entries -= 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    record_error(&mut result.quality, &error);
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let expected = if level == 2 {
                name.strip_prefix("local_").is_some_and(is_uuid)
            } else {
                is_uuid(name)
            };
            if !expected {
                continue;
            }
            let child = entry.path();
            let metadata = match fs::symlink_metadata(&child) {
                Ok(metadata) => metadata,
                Err(error) => {
                    record_error(&mut result.quality, &error);
                    continue;
                }
            };
            if metadata_is_link_like(&metadata) {
                result.quality.symlink_skipped_count += 1;
                result.quality.skipped_count += 1;
                continue;
            }
            if !metadata.is_dir() {
                continue;
            }
            match classify(&child) {
                LocalPathStatus::ConfirmedLocal => {}
                LocalPathStatus::RejectedNetwork => {
                    result.quality.network_skipped_count += 1;
                    result.quality.skipped_count += 1;
                    continue;
                }
                LocalPathStatus::Missing | LocalPathStatus::Indeterminate => {
                    result.quality.skipped_count += 1;
                    continue;
                }
            }
            if level == 2 {
                inspect_inner_root(child.join(".claude"), result);
            } else {
                visit_level(
                    &child,
                    level + 1,
                    cancellation,
                    remaining_entries,
                    result,
                    classify,
                );
            }
            if result.quality.budget_exhausted || result.quality.cancelled {
                return;
            }
        }
    }

    /// 只把普通 `.claude` 目录交给现有 transcript 签名探测。
    fn inspect_inner_root(path: PathBuf, result: &mut EmbeddedCandidates) {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                record_error(&mut result.quality, &error);
                return;
            }
        };
        if metadata_is_link_like(&metadata) {
            result.quality.symlink_skipped_count += 1;
            result.quality.skipped_count += 1;
            return;
        }
        if !metadata.is_dir() {
            return;
        }
        if result.candidates.len() >= MAX_CANDIDATES {
            result.quality.budget_exhausted = true;
            return;
        }
        result.candidates.push(Candidate {
            existing_root_id: None,
            path,
            alias: "Claude Desktop".to_owned(),
            method: DiscoveryMethod::DefaultHome,
        });
    }

    /// 目录读取失败仅发布计数，不把任何路径或文件名写入日志。
    fn record_error(quality: &mut EnumerationQuality, error: &std::io::Error) {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            quality.permission_denied_count += 1;
        } else {
            quality.skipped_count += 1;
        }
    }

    #[cfg(test)]
    mod tests {
        use tempfile::tempdir;

        use super::*;

        /// 任一动态层变为网络挂载时，都不得打开其目录或后续 `.claude`。
        #[test]
        fn network_mount_at_each_dynamic_level_is_skipped_before_descent() {
            let temp = tempdir().expect("temporary home is available");
            let container = temp
                .path()
                .join("Library/Application Support/Claude/local-agent-mode-sessions");
            let first = container.join("11111111-1111-1111-1111-111111111111");
            let second = first.join("22222222-2222-2222-2222-222222222222");
            let third = second.join("local_33333333-3333-3333-3333-333333333333");
            fs::create_dir_all(third.join(".claude"))
                .expect("fixed embedded layout is available");

            for (blocked, directories_scanned) in [(&first, 1), (&second, 2), (&third, 3)] {
                let result = candidates_with_classifier(
                    Some(temp.path()),
                    &CancellationToken::new(),
                    &|path| {
                        if path == blocked.as_path() {
                            LocalPathStatus::RejectedNetwork
                        } else {
                            LocalPathStatus::ConfirmedLocal
                        }
                    },
                );
                assert!(result.candidates.is_empty());
                assert_eq!(result.quality.directories_scanned, directories_scanned);
                assert_eq!(result.quality.network_skipped_count, 1);
                assert_eq!(result.quality.skipped_count, 1);
                assert!(!result.quality.budget_exhausted);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) fn candidates(
    home_dir: Option<&Path>,
    cancellation: &CancellationToken,
) -> EmbeddedCandidates {
    macos::candidates(home_dir, cancellation)
}
