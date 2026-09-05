//! 单个目录的同步枚举与签名探测岛：一次 `spawn_blocking` 处理整目录，
//! 绝不按 DirEntry 拆分 blocking 任务。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use loki_metis_core::{SourceFileObservation, source_file_needs_visit};

use crate::backend::local_index::discovery::{metadata_is_link_like, path_key};
use crate::backend::local_index::file_source::{
    RolloutFileInspection, RolloutProbeBudget, ValidatedRolloutFile, inspect_rollout_file,
    is_rollout_jsonl, modified_epoch_ms,
};
use crate::backend::local_index::scan::CancellationToken;
use crate::backend::local_index::{LocalError, LocalErrorKind};

/// 目录访问后需要由 async 扫描壳继续处理的动作。
pub(super) enum EntryAction {
    /// 继续深入子目录。
    PushDirectory(PathBuf),
    /// 仅保留相对标签，不索引。
    RetainLabel(String),
    /// 确认拒绝的相对标签。
    RejectLabel(String),
    /// 已通过签名探测，交给 async 侧串行索引。
    IndexFile {
        relative_label: String,
        source: ValidatedRolloutFile,
    },
}

/// 一次目录级 blocking 访问的汇总结果。
pub(super) struct DirectoryVisitOutcome {
    /// 访问后剩余允许处理的目录项数预算。
    pub(super) remaining_entries: u64,
    /// 访问后剩余的签名探测预算。
    pub(super) probe_budget: RolloutProbeBudget,
    /// 权限拒绝计数。
    pub(super) permission_denied: u64,
    /// 因链接、非本地卷等原因跳过的计数。
    pub(super) skipped: u64,
    /// 格式与数据质量告警计数。
    pub(super) warnings: u64,
    /// 需要由 async 侧继续处理的动作列表。
    pub(super) actions: Vec<EntryAction>,
    /// 标识本目录已完整枚举，未因预算或错误提前中止。
    pub(super) enumeration_complete: bool,
    /// 标识本次访问因取消信号提前终止。
    pub(super) cancelled: bool,
    /// 标识预算已耗尽，本次访问未必完整。
    pub(super) budget_exhausted: bool,
}

/// 一个根区域内枚举所需的窗口下界与上次 checkpoint 观察值；跨目录岛共享。
pub(super) struct VisitWindow {
    /// 从未索引过的文件只在修改时间不早于该下界时进入解析。
    pub(super) scan_since_epoch_ms: i64,
    /// 该根区域下已索引来源上次记录的大小与修改时间，按相对标签索引。
    pub(super) known_sources: Arc<HashMap<String, SourceFileObservation>>,
}

/// 读取并预检一个目录；预算与取消在岛内边界检查。
pub(super) fn visit_directory(
    directory: PathBuf,
    root_path: PathBuf,
    mut remaining_entries: u64,
    window: &VisitWindow,
    cancellation: &CancellationToken,
    mut probe_budget: RolloutProbeBudget,
) -> Result<DirectoryVisitOutcome, LocalError> {
    let mut permission_denied = 0_u64;
    let mut skipped = 0_u64;
    let mut warnings = 0_u64;
    let mut actions = Vec::new();
    let mut enumeration_complete = true;
    let mut cancelled = false;
    let mut budget_exhausted = false;
    let mut entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(if error.kind() == std::io::ErrorKind::PermissionDenied {
                LocalError::new(LocalErrorKind::PermissionDenied, "directory unreadable")
            } else {
                LocalError::new(LocalErrorKind::SourceUnavailable, "directory unreadable")
            });
        }
    };
    loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            enumeration_complete = false;
            break;
        }
        let Some(entry) = entries.next() else {
            break;
        };
        if remaining_entries == 0 {
            enumeration_complete = false;
            budget_exhausted = true;
            skipped = skipped.saturating_add(1);
            warnings = warnings.saturating_add(1);
            break;
        }
        remaining_entries -= 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                enumeration_complete = false;
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    permission_denied = permission_denied.saturating_add(1);
                } else {
                    skipped = skipped.saturating_add(1);
                }
                warnings = warnings.saturating_add(1);
                continue;
            }
        };
        let path = entry.path();
        let relative_label = is_rollout_jsonl(&path)
            .then(|| path.strip_prefix(&root_path).ok())
            .flatten()
            .map(path_key);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                if let Some(relative_label) = relative_label {
                    actions.push(EntryAction::RetainLabel(relative_label));
                }
                enumeration_complete = false;
                skipped = skipped.saturating_add(1);
                warnings = warnings.saturating_add(1);
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            if let Some(relative_label) = relative_label {
                actions.push(EntryAction::RejectLabel(relative_label));
            } else {
                skipped = skipped.saturating_add(1);
                warnings = warnings.saturating_add(1);
            }
            continue;
        }
        if metadata.is_dir() {
            if let Some(relative_label) = relative_label {
                actions.push(EntryAction::RejectLabel(relative_label));
            }
            actions.push(EntryAction::PushDirectory(path));
            continue;
        }
        let Some(relative_label) = relative_label else {
            continue;
        };
        if !metadata.is_file() {
            actions.push(EntryAction::RejectLabel(relative_label));
            continue;
        }
        // 窗口外但被追加/替换过的已索引文件必须重新解析，否则新增字节会永久丢失。
        if !source_file_needs_visit(
            modified_epoch_ms(&metadata),
            metadata.len(),
            window.scan_since_epoch_ms,
            window.known_sources.get(&relative_label).copied(),
        ) {
            actions.push(EntryAction::RetainLabel(relative_label));
            continue;
        }
        match inspect_rollout_file(&path, cancellation, &mut probe_budget) {
            RolloutFileInspection::Matched(source) => {
                actions.push(EntryAction::IndexFile {
                    relative_label,
                    source,
                });
            }
            RolloutFileInspection::Rejected => {
                actions.push(EntryAction::RejectLabel(relative_label));
            }
            RolloutFileInspection::Indeterminate => {
                actions.push(EntryAction::RetainLabel(relative_label));
                skipped = skipped.saturating_add(1);
                warnings = warnings.saturating_add(1);
            }
            RolloutFileInspection::Cancelled => {
                cancelled = true;
                enumeration_complete = false;
                break;
            }
            RolloutFileInspection::BudgetExhausted => {
                skipped = skipped.saturating_add(1);
                warnings = warnings.saturating_add(1);
                enumeration_complete = false;
                budget_exhausted = true;
                break;
            }
        }
    }
    Ok(DirectoryVisitOutcome {
        remaining_entries,
        probe_budget,
        permission_denied,
        skipped,
        warnings,
        actions,
        enumeration_complete,
        cancelled,
        budget_exhausted,
    })
}
