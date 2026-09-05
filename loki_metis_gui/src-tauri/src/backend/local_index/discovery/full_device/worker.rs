//! 目录预检与枚举 worker；只返回结果，不修改共享协调器状态。

use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::is_discovery_path_excluded;

use super::super::inspection::{metadata_is_link_like, reject_symlink_components};
use super::super::path_key;
use super::types::{
    DirectoryCursor, EnumerationOutcome, PendingTask, PreflightOutcome, TraversalItem,
    WorkerOutcome, WorkerResult, WorkerTask,
};
use crate::backend::local_index::CancellationToken;

/// 按任务类型分派到预检或枚举处理，返回带原始顺序号的结果。
pub(super) fn execute_worker_task(
    task: WorkerTask,
    cancellation: &CancellationToken,
) -> WorkerResult {
    let order = task.order;
    let outcome = match task.pending {
        PendingTask::Preflight(item) => {
            WorkerOutcome::Preflight(preflight_directory(&item, cancellation))
        }
        PendingTask::Enumerate(cursor) => WorkerOutcome::Enumerated(Box::new(
            enumerate_directory_chunk(*cursor, task.entry_quota, cancellation),
        )),
    };
    WorkerResult { order, outcome }
}

/// 验证候选仍是非链接普通目录并取得规范化路径；不消费签名或修改覆盖事实。
fn preflight_directory(item: &TraversalItem, cancellation: &CancellationToken) -> PreflightOutcome {
    if cancellation.is_cancelled() {
        return PreflightOutcome::Cancelled;
    }
    if item.is_traversal_root {
        match reject_symlink_components(&item.path) {
            Ok(true) => return PreflightOutcome::Symlink,
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return PreflightOutcome::PermissionDenied;
            }
            Err(_) => return PreflightOutcome::Skipped,
        }
    }
    let metadata = match fs::symlink_metadata(&item.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return PreflightOutcome::PermissionDenied;
        }
        Err(_) => return PreflightOutcome::Skipped,
    };
    if metadata_is_link_like(&metadata) {
        return PreflightOutcome::Symlink;
    }
    if !metadata.is_dir() {
        return PreflightOutcome::NotDirectory;
    }
    if cancellation.is_cancelled() {
        return PreflightOutcome::Cancelled;
    }
    PreflightOutcome::Ready {
        normalized: fs::canonicalize(&item.path).unwrap_or_else(|_| item.path.clone()),
        traversal_root: item.traversal_root.clone(),
        policy_path: item.policy_path.clone(),
    }
}

/// 在固定目录项配额内继续枚举一个目录，逐项复查链接并随时响应同一取消信号。
fn enumerate_directory_chunk(
    mut cursor: DirectoryCursor,
    entry_quota: u64,
    cancellation: &CancellationToken,
) -> EnumerationOutcome {
    let mut children = std::mem::take(&mut cursor.children);
    let mut permission_denied_count = 0_u64;
    let mut skipped_count = 0_u64;
    let mut symlink_skipped_count = 0_u64;
    let mut entries_consumed = 0_u64;
    let mut entries = match cursor.entries.take() {
        Some(entries) => entries,
        None => match fs::read_dir(&cursor.path) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    permission_denied_count = 1;
                } else {
                    skipped_count = 1;
                }
                return EnumerationOutcome {
                    entries_consumed,
                    children,
                    continuation: None,
                    permission_denied_count,
                    skipped_count,
                    symlink_skipped_count,
                    cancelled: false,
                };
            }
        },
    };
    let mut complete = false;
    let mut cancelled = false;
    while entries_consumed < entry_quota {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let entry = match cursor.buffered_entry.take() {
            Some(entry) => Some(entry),
            None => entries.next(),
        };
        let Some(entry) = entry else {
            complete = true;
            break;
        };
        entries_consumed = entries_consumed.saturating_add(1);
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
                continue;
            }
            Err(_) => {
                skipped_count = skipped_count.saturating_add(1);
                continue;
            }
        };
        let entry_path = entry.path();
        let policy_path = cursor.policy_path.join(entry.file_name());
        match fs::symlink_metadata(&entry_path) {
            Ok(metadata) if metadata_is_link_like(&metadata) => {
                symlink_skipped_count = symlink_skipped_count.saturating_add(1);
                skipped_count = skipped_count.saturating_add(1);
            }
            Ok(metadata) if metadata.is_dir() => children.push(TraversalItem {
                path: entry_path,
                policy_path,
                traversal_root: cursor.traversal_root.clone(),
                is_traversal_root: false,
            }),
            Ok(_) => {}
            Err(_) => skipped_count = skipped_count.saturating_add(1),
        }
    }
    if !complete && !cancelled && entries_consumed == entry_quota {
        if cancellation.is_cancelled() {
            cancelled = true;
        } else {
            match entries.next() {
                Some(entry) => cursor.buffered_entry = Some(entry),
                None => complete = true,
            }
        }
    }
    let continuation = (!complete && !cancelled).then(|| {
        cursor.entries = Some(entries);
        cursor.children = std::mem::take(&mut children);
        cursor
    });
    if complete {
        children.sort_by_key(|child| path_key(&child.path));
    } else {
        children.clear();
    }
    EnumerationOutcome {
        entries_consumed,
        children,
        continuation,
        permission_denied_count,
        skipped_count,
        symlink_skipped_count,
        cancelled,
    }
}

/// 首目录立即发布，之后每 128 个目录发布一次，避免为大范围遍历制造无界任务。
pub(super) const fn should_publish_progress(directories_scanned: u64) -> bool {
    directories_scanned == 1 || directories_scanned.is_multiple_of(128)
}

/// 判断主动发现路径是否位于排除范围，同时允许单独排队的本地卷穿过挂载容器。
pub(super) fn is_excluded(path: &Path, traversal_root: &Path, excluded_roots: &[PathBuf]) -> bool {
    is_discovery_path_excluded(path, traversal_root, excluded_roots)
}

/// 主卷遍历遇到另一个已单独排队的本地挂载点时停止跨卷进入。
pub(super) fn crosses_another_search_root(
    path: &Path,
    traversal_root: &Path,
    search_roots: &[PathBuf],
) -> bool {
    search_roots.iter().any(|other| {
        other != traversal_root && other.starts_with(traversal_root) && path.starts_with(other)
    })
}
