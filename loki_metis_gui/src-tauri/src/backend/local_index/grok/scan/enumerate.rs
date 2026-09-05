//! Grok 根内 `sessions/**/updates.jsonl` 的同步枚举岛。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::{SourceFileObservation, source_file_needs_visit};

use crate::backend::local_index::discovery::metadata_is_link_like;
use crate::backend::local_index::discovery::path_key;
use crate::backend::local_index::file_source::modified_epoch_ms;
use crate::backend::local_index::scan::CancellationToken;

use super::super::discovery::GrokDiscoveredRoot;
use super::super::path_rules::is_grok_updates_path;

/// 汇总一次扫描内跨目录、跨文件累积的可变计数与剩余预算。
#[derive(Default, Clone)]
pub(super) struct ScanCounters {
    pub(super) remaining_directories: u64,
    pub(super) remaining_entries: u64,
    pub(super) files_scanned: u64,
    pub(super) unchanged_files: u64,
    pub(super) rebuilt_files: u64,
    pub(super) calls_added: u64,
    pub(super) permission_denied_count: u64,
    pub(super) skipped_count: u64,
    pub(super) warning_count: u64,
    pub(super) budget_exhausted: bool,
}

/// 一个根内按相对路径累积的保留与拒绝来源标签。
#[derive(Default)]
pub(super) struct RootSourceLabels {
    pub(super) retained: std::collections::HashSet<String>,
    pub(super) rejected: std::collections::HashSet<String>,
}

/// 枚举结果：更新后的计数、标签、完整性与待索引路径。
pub(super) struct EnumerateOutcome {
    pub(super) counters: ScanCounters,
    pub(super) labels: RootSourceLabels,
    pub(super) enumeration_complete: bool,
    pub(super) cancelled: bool,
    pub(super) paths: Vec<PathBuf>,
}

/// 枚举一个根所需的窗口下界与上次 checkpoint 观察值。
pub(super) struct EnumerateWindow {
    /// 从未索引过的文件只在修改时间不早于该下界时进入解析。
    pub(super) scan_since_epoch_ms: i64,
    /// 该根下已索引来源上次记录的大小与修改时间，按相对标签索引。
    pub(super) known_sources: HashMap<String, SourceFileObservation>,
}

/// 在 blocking 岛内枚举一个 Grok 根下全部可索引 updates.jsonl。
pub(super) fn enumerate_root_updates(
    root: &GrokDiscoveredRoot,
    mut counters: ScanCounters,
    window: &EnumerateWindow,
    cancellation: &CancellationToken,
) -> EnumerateOutcome {
    let mut labels = RootSourceLabels::default();
    let mut enumeration_complete = true;
    let mut cancelled = false;
    let mut paths = Vec::new();
    walk(
        &root.path,
        &root.path.join("sessions"),
        0,
        &mut counters,
        &mut labels,
        &mut enumeration_complete,
        &mut cancelled,
        &mut paths,
        window,
        cancellation,
    );
    EnumerateOutcome {
        counters,
        labels,
        enumeration_complete,
        cancelled,
        paths,
    }
}

#[allow(clippy::too_many_arguments)]
/// 在严格根与预算内递归枚举 Grok `updates.jsonl` 文件。
fn walk(
    root: &Path,
    directory: &Path,
    depth: u32,
    counters: &mut ScanCounters,
    labels: &mut RootSourceLabels,
    enumeration_complete: &mut bool,
    cancelled: &mut bool,
    paths: &mut Vec<PathBuf>,
    window: &EnumerateWindow,
    cancellation: &CancellationToken,
) {
    if cancellation.is_cancelled() {
        *cancelled = true;
        *enumeration_complete = false;
        return;
    }
    if counters.remaining_directories == 0 {
        counters.budget_exhausted = true;
        *enumeration_complete = false;
        return;
    }
    counters.remaining_directories -= 1;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            counters.permission_denied_count = counters.permission_denied_count.saturating_add(1);
            *enumeration_complete = false;
            return;
        }
        Err(_) => {
            counters.skipped_count = counters.skipped_count.saturating_add(1);
            *enumeration_complete = false;
            return;
        }
    };
    for entry in entries {
        if cancellation.is_cancelled() {
            *cancelled = true;
            *enumeration_complete = false;
            return;
        }
        if counters.remaining_entries == 0 {
            counters.budget_exhausted = true;
            *enumeration_complete = false;
            return;
        }
        counters.remaining_entries -= 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                counters.skipped_count = counters.skipped_count.saturating_add(1);
                *enumeration_complete = false;
                continue;
            }
        };
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                counters.skipped_count = counters.skipped_count.saturating_add(1);
                *enumeration_complete = false;
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            if depth < 3 {
                walk(
                    root,
                    &entry.path(),
                    depth + 1,
                    counters,
                    labels,
                    enumeration_complete,
                    cancelled,
                    paths,
                    window,
                    cancellation,
                );
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let entry_path = entry.path();
        let Ok(relative) = entry_path.strip_prefix(root) else {
            continue;
        };
        if is_grok_updates_path(relative) {
            let relative_label = path_key(relative);
            // 窗口外但被追加/替换过的已索引文件必须重新解析，否则新增字节会永久丢失。
            if source_file_needs_visit(
                modified_epoch_ms(&metadata),
                metadata.len(),
                window.scan_since_epoch_ms,
                window.known_sources.get(&relative_label).copied(),
            ) {
                paths.push(entry.path());
            } else {
                labels.retained.insert(relative_label);
            }
        } else {
            labels
                .rejected
                .insert(relative.to_string_lossy().into_owned());
        }
    }
}
