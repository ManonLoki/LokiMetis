//! Claude 根内 transcript 路径的同步枚举岛。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::{SourceFileObservation, source_file_needs_visit};

use crate::backend::local_index::discovery::metadata_is_link_like;
use crate::backend::local_index::discovery::path_key;
use crate::backend::local_index::file_source::modified_epoch_ms;
use crate::backend::local_index::scan::CancellationToken;

use super::super::discovery::ClaudeDiscoveredRoot;
use super::super::path_rules::{is_subagent_jsonl_name, is_uuid, is_uuid_jsonl_name};

/// 汇总一次扫描内跨目录、跨文件累积的可变计数与剩余预算。
#[derive(Default, Clone)]
pub(super) struct ScanCounters {
    /// 剩余允许遍历的目录数预算。
    pub(super) remaining_directories: u64,
    /// 剩余允许处理的目录项数预算。
    pub(super) remaining_entries: u64,
    /// 本次扫描已处理的文件数。
    pub(super) files_scanned: u64,
    /// 未发生变化、跳过重新索引的文件数。
    pub(super) unchanged_files: u64,
    /// 因截断/替换等原因整份重建的文件数。
    pub(super) rebuilt_files: u64,
    /// 本次扫描新增写入的调用条数。
    pub(super) calls_added: u64,
    /// 权限拒绝计数。
    pub(super) permission_denied_count: u64,
    /// 因链接、非本地卷等原因跳过的计数。
    pub(super) skipped_count: u64,
    /// 格式与数据质量告警计数。
    pub(super) warning_count: u64,
    /// 标识预算已耗尽，本次枚举未必完整。
    pub(super) budget_exhausted: bool,
}

/// 一个根内按相对路径累积的保留与拒绝来源标签。
#[derive(Default)]
pub(super) struct RootSourceLabels {
    /// 已确认应保留索引的来源相对标签集合。
    pub(super) retained: std::collections::HashSet<String>,
    /// 已确认应拒绝索引的来源相对标签集合。
    pub(super) rejected: std::collections::HashSet<String>,
}

/// 枚举结果：更新后的计数、标签、完整性与待索引路径。
pub(super) struct EnumerateOutcome {
    /// 更新后的扫描计数。
    pub(super) counters: ScanCounters,
    /// 更新后的保留/拒绝来源标签。
    pub(super) labels: RootSourceLabels,
    /// 标识本次枚举已完整覆盖该根，未因预算或错误提前中止。
    pub(super) enumeration_complete: bool,
    /// 标识本次枚举因取消信号提前终止。
    pub(super) cancelled: bool,
    /// 本次枚举确认的待索引文件路径。
    pub(super) paths: Vec<PathBuf>,
}

/// 枚举一个根所需的窗口下界与上次 checkpoint 观察值。
pub(super) struct EnumerateWindow {
    /// 从未索引过的文件只在修改时间不早于该下界时进入解析。
    pub(super) scan_since_epoch_ms: i64,
    /// 该根下已索引来源上次记录的大小与修改时间，按相对标签索引。
    pub(super) known_sources: HashMap<String, SourceFileObservation>,
}

/// 在 blocking 岛内枚举一个 Claude 根下全部可索引 transcript 路径。
pub(super) fn enumerate_root_transcripts(
    root: &ClaudeDiscoveredRoot,
    mut counters: ScanCounters,
    window: &EnumerateWindow,
    cancellation: &CancellationToken,
) -> EnumerateOutcome {
    let mut labels = RootSourceLabels::default();
    let mut enumeration_complete = true;
    let mut cancelled = false;
    let mut paths = Vec::new();
    let projects = root.path.join("projects");
    let Some(project_entries) = read_directory(&projects, &mut counters, &mut enumeration_complete)
    else {
        return EnumerateOutcome {
            counters,
            labels,
            enumeration_complete: false,
            cancelled,
            paths,
        };
    };
    for project in project_entries {
        if cancellation.is_cancelled() {
            cancelled = true;
            enumeration_complete = false;
            break;
        }
        let metadata = match fs::symlink_metadata(project.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                enumeration_complete = false;
                counters.skipped_count = counters.skipped_count.saturating_add(1);
                counters.warning_count = counters.warning_count.saturating_add(1);
                continue;
            }
        };
        if metadata_is_link_like(&metadata) || !metadata.is_dir() {
            continue;
        }
        let Some(entries) =
            read_directory(&project.path(), &mut counters, &mut enumeration_complete)
        else {
            enumeration_complete = false;
            continue;
        };
        for entry in entries {
            if cancellation.is_cancelled() {
                cancelled = true;
                enumeration_complete = false;
                break;
            }
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    enumeration_complete = false;
                    counters.skipped_count = counters.skipped_count.saturating_add(1);
                    counters.warning_count = counters.warning_count.saturating_add(1);
                    continue;
                }
            };
            if metadata_is_link_like(&metadata) {
                counters.skipped_count = counters.skipped_count.saturating_add(1);
                counters.warning_count = counters.warning_count.saturating_add(1);
                continue;
            }
            let name = path.file_name().and_then(|value| value.to_str());
            if metadata.is_file() && name.is_some_and(is_uuid_jsonl_name) {
                retain_or_queue(root, path, &metadata, window, &mut labels, &mut paths);
            } else if metadata.is_dir() && name.is_some_and(is_uuid) {
                let subagents = path.join("subagents");
                if let Some(subagent_entries) =
                    read_directory(&subagents, &mut counters, &mut enumeration_complete)
                {
                    for subagent in subagent_entries {
                        let subagent_path = subagent.path();
                        let metadata = match fs::symlink_metadata(&subagent_path) {
                            Ok(metadata) => metadata,
                            Err(_) => {
                                enumeration_complete = false;
                                counters.skipped_count = counters.skipped_count.saturating_add(1);
                                counters.warning_count = counters.warning_count.saturating_add(1);
                                continue;
                            }
                        };
                        if metadata_is_link_like(&metadata) || !metadata.is_file() {
                            continue;
                        }
                        if !subagent_path
                            .file_name()
                            .and_then(|value| value.to_str())
                            .is_some_and(is_subagent_jsonl_name)
                        {
                            continue;
                        }
                        retain_or_queue(
                            root,
                            subagent_path,
                            &metadata,
                            window,
                            &mut labels,
                            &mut paths,
                        );
                    }
                }
            }
        }
        if cancelled || counters.budget_exhausted {
            enumeration_complete = false;
            break;
        }
    }
    EnumerateOutcome {
        counters,
        labels,
        enumeration_complete,
        cancelled,
        paths,
    }
}

/// 窗口外且与 checkpoint 一致的文件保留既有索引，不进入本轮 parser；
/// 窗口外但被追加/替换过的已索引文件必须重新解析，否则新增字节会永久丢失。
fn retain_or_queue(
    root: &ClaudeDiscoveredRoot,
    path: PathBuf,
    metadata: &fs::Metadata,
    window: &EnumerateWindow,
    labels: &mut RootSourceLabels,
    paths: &mut Vec<PathBuf>,
) {
    let relative_label = path.strip_prefix(&root.path).ok().map(path_key);
    let checkpoint = relative_label
        .as_ref()
        .and_then(|label| window.known_sources.get(label).copied());
    if source_file_needs_visit(
        modified_epoch_ms(metadata),
        metadata.len(),
        window.scan_since_epoch_ms,
        checkpoint,
    ) {
        paths.push(path);
    } else if let Some(relative_label) = relative_label {
        labels.retained.insert(relative_label);
    }
}

/// 在预算允许时读取单个目录项列表；预算耗尽则标记未完整枚举并返回空。
fn read_directory(
    path: &Path,
    counters: &mut ScanCounters,
    enumeration_complete: &mut bool,
) -> Option<Vec<fs::DirEntry>> {
    if counters.remaining_directories == 0 {
        *enumeration_complete = false;
        counters.budget_exhausted = true;
        counters.skipped_count = counters.skipped_count.saturating_add(1);
        counters.warning_count = counters.warning_count.saturating_add(1);
        return None;
    }
    counters.remaining_directories -= 1;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(error) => {
            *enumeration_complete = false;
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                counters.permission_denied_count =
                    counters.permission_denied_count.saturating_add(1);
            } else {
                counters.skipped_count = counters.skipped_count.saturating_add(1);
            }
            counters.warning_count = counters.warning_count.saturating_add(1);
            return None;
        }
    };
    let mut result = Vec::new();
    for entry in entries {
        if counters.remaining_entries == 0 {
            *enumeration_complete = false;
            counters.budget_exhausted = true;
            counters.skipped_count = counters.skipped_count.saturating_add(1);
            counters.warning_count = counters.warning_count.saturating_add(1);
            break;
        }
        counters.remaining_entries -= 1;
        match entry {
            Ok(entry) => result.push(entry),
            Err(_) => {
                *enumeration_complete = false;
                counters.skipped_count = counters.skipped_count.saturating_add(1);
                counters.warning_count = counters.warning_count.saturating_add(1);
            }
        }
    }
    Some(result)
}
