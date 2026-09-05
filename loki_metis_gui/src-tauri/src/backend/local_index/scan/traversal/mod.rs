//! 遍历已发现根的活动与归档 rollout：目录枚举/签名探测按目录级
//! `spawn_blocking` 岛执行；索引生命周期与写库在 async 侧串行 await。

mod directory_island;

use std::collections::HashSet;
use std::sync::Arc;

use loki_metis_core::{CoverageReport, CoverageState};
use tauri::async_runtime::spawn_blocking;

use super::{CancellationToken, ScanConfig, ScanProgress, ScanSummary};
use crate::backend::local_index::RegisterDiscoveredRoot;
use crate::backend::local_index::current_epoch_ms;
use crate::backend::local_index::file_source::RolloutProbeBudget;
use crate::backend::local_index::rollout_ingest::IndexRolloutFile;
use crate::backend::local_index::{DiscoveredRoot, LocalError, LocalErrorKind, LocalIndex};
use directory_island::{DirectoryVisitOutcome, EntryAction, VisitWindow, visit_directory};

/// 扫描已发现根的活动与归档 rollout，并用回调发布无路径进度。
pub async fn scan_discovered_roots<F>(
    index: &mut LocalIndex,
    roots: &[DiscoveredRoot],
    config: ScanConfig,
    cancellation: &CancellationToken,
    mut on_progress: F,
) -> Result<ScanSummary, LocalError>
where
    F: FnMut(ScanProgress),
{
    let started_at = config.started_at_epoch_ms.unwrap_or_else(current_epoch_ms);
    let scan_id = index.begin_scan(config.mode.as_str(), started_at).await?;
    let roots_total = u64::try_from(roots.len()).unwrap_or(u64::MAX);
    let mut files_scanned = 0_u64;
    let mut unchanged_files = 0_u64;
    let mut rebuilt_files = 0_u64;
    let mut calls_added = 0_u64;
    let mut warning_count = 0_u64;
    let mut permission_denied_count = 0_u64;
    let mut skipped_count = 0_u64;
    let mut roots_completed = 0_u64;
    let mut cancelled = false;
    let mut scan_budget_exhausted = false;
    let mut remaining_directories = config.max_directories;
    let mut remaining_entries = config.max_entries;
    let mut rollout_probe_budget =
        RolloutProbeBudget::new(config.max_signature_files, config.max_signature_bytes);

    for root in roots {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        on_progress(ScanProgress {
            scan_id: scan_id.clone(),
            current_root_id: root.root_id.clone(),
            roots_completed,
            roots_total,
            files_scanned,
            calls_added,
            warning_count,
        });
        index.register_root(root).await?;
        // 每个数据根只建一次共享路径，目录循环里改为引用计数而非复制整条路径。
        let root_path = Arc::new(root.path.clone());
        let before_warning_count = warning_count;
        for (area, archived, available, inspection_complete) in [
            (
                "sessions",
                false,
                root.has_sessions,
                root.sessions_inspection_complete,
            ),
            (
                "archived_sessions",
                true,
                root.has_archived_sessions,
                root.archived_sessions_inspection_complete,
            ),
        ] {
            if !available {
                if inspection_complete && !cancellation.is_cancelled() {
                    index
                        .reconcile_root_sources(
                            &root.root_id,
                            archived,
                            &HashSet::new(),
                            &HashSet::new(),
                            true,
                        )
                        .await?;
                } else if !inspection_complete {
                    skipped_count = skipped_count.saturating_add(1);
                    warning_count = warning_count.saturating_add(1);
                }
                continue;
            }
            let mut area_enumeration_complete = true;
            let mut retained_relative_labels = HashSet::new();
            let mut rejected_relative_labels = HashSet::new();
            let known_sources = Arc::new(
                index
                    .source_file_observations_for_root(&root.root_id, archived)
                    .await?,
            );
            let mut stack = vec![root.path.join(area)];
            while let Some(directory) = stack.pop() {
                if cancellation.is_cancelled() {
                    cancelled = true;
                    break;
                }
                if scan_budget_exhausted {
                    area_enumeration_complete = false;
                    break;
                }
                if remaining_directories == 0 {
                    area_enumeration_complete = false;
                    scan_budget_exhausted = true;
                    skipped_count = skipped_count.saturating_add(1);
                    warning_count = warning_count.saturating_add(1);
                    break;
                }
                remaining_directories -= 1;
                let root_path = Arc::clone(&root_path);
                let cancel = cancellation.clone();
                let budget = rollout_probe_budget;
                let window = VisitWindow {
                    scan_since_epoch_ms: config.scan_since_epoch_ms,
                    known_sources: Arc::clone(&known_sources),
                };
                let visit = spawn_blocking(move || {
                    visit_directory(
                        directory,
                        root_path,
                        remaining_entries,
                        &window,
                        &cancel,
                        budget,
                    )
                })
                .await
                .map_err(|_| {
                    LocalError::new(
                        LocalErrorKind::SourceUnavailable,
                        "directory visit worker lost",
                    )
                })?;
                let DirectoryVisitOutcome {
                    remaining_entries: next_entries,
                    probe_budget,
                    permission_denied,
                    skipped,
                    warnings,
                    actions,
                    enumeration_complete,
                    cancelled: visit_cancelled,
                    budget_exhausted,
                } = match visit {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        area_enumeration_complete = false;
                        if error.kind() == LocalErrorKind::PermissionDenied {
                            permission_denied_count = permission_denied_count.saturating_add(1);
                        } else {
                            skipped_count = skipped_count.saturating_add(1);
                        }
                        warning_count = warning_count.saturating_add(1);
                        continue;
                    }
                };
                remaining_entries = next_entries;
                rollout_probe_budget = probe_budget;
                permission_denied_count = permission_denied_count.saturating_add(permission_denied);
                skipped_count = skipped_count.saturating_add(skipped);
                warning_count = warning_count.saturating_add(warnings);
                if !enumeration_complete {
                    area_enumeration_complete = false;
                }
                if visit_cancelled {
                    cancelled = true;
                }
                if budget_exhausted {
                    scan_budget_exhausted = true;
                    area_enumeration_complete = false;
                }
                for action in actions {
                    match action {
                        EntryAction::PushDirectory(path) => stack.push(path),
                        EntryAction::RetainLabel(label) => {
                            retained_relative_labels.insert(label);
                        }
                        EntryAction::RejectLabel(label) => {
                            rejected_relative_labels.insert(label);
                            skipped_count = skipped_count.saturating_add(1);
                            warning_count = warning_count.saturating_add(1);
                        }
                        EntryAction::IndexFile {
                            relative_label,
                            source,
                        } => {
                            retained_relative_labels.insert(relative_label);
                            match index
                                .index_rollout_file(root, source, archived, config, cancellation)
                                .await
                            {
                                Ok(outcome) => {
                                    files_scanned = files_scanned.saturating_add(1);
                                    unchanged_files = unchanged_files
                                        .saturating_add(u64::from(outcome.unchanged));
                                    rebuilt_files =
                                        rebuilt_files.saturating_add(u64::from(outcome.rebuilt));
                                    calls_added = calls_added.saturating_add(outcome.added_calls);
                                    warning_count =
                                        warning_count.saturating_add(outcome.warnings.total());
                                    if outcome.cancelled {
                                        cancelled = true;
                                    }
                                }
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        LocalErrorKind::PermissionDenied
                                            | LocalErrorKind::SourceUnavailable
                                            | LocalErrorKind::InvalidPath
                                            | LocalErrorKind::InvalidUsage
                                    ) =>
                                {
                                    skipped_count = skipped_count.saturating_add(1);
                                    warning_count = warning_count.saturating_add(1);
                                }
                                Err(error) => {
                                    index
                                        .finish_scan(
                                            &scan_id,
                                            current_epoch_ms(),
                                            "failed",
                                            false,
                                            files_scanned,
                                            calls_added,
                                            warning_count,
                                        )
                                        .await?;
                                    return Err(error);
                                }
                            }
                            on_progress(ScanProgress {
                                scan_id: scan_id.clone(),
                                current_root_id: root.root_id.clone(),
                                roots_completed,
                                roots_total,
                                files_scanned,
                                calls_added,
                                warning_count,
                            });
                            if cancelled {
                                break;
                            }
                        }
                    }
                }
                if cancelled || scan_budget_exhausted {
                    break;
                }
            }
            index
                .reconcile_root_sources(
                    &root.root_id,
                    archived,
                    &retained_relative_labels,
                    &rejected_relative_labels,
                    area_enumeration_complete && !cancelled,
                )
                .await?;
            if cancelled || scan_budget_exhausted {
                break;
            }
        }

        if !cancelled && !scan_budget_exhausted {
            roots_completed = roots_completed.saturating_add(1);
        }
        let root_state = if cancelled {
            CoverageState::Cancelled
        } else if scan_budget_exhausted || warning_count > before_warning_count {
            CoverageState::Partial
        } else {
            CoverageState::Complete
        };
        index
            .record_root_coverage(&root.root_id, root_state)
            .await?;
        on_progress(ScanProgress {
            scan_id: scan_id.clone(),
            current_root_id: root.root_id.clone(),
            roots_completed,
            roots_total,
            files_scanned,
            calls_added,
            warning_count,
        });
        if cancelled || scan_budget_exhausted {
            break;
        }
    }

    let state = if cancelled {
        CoverageState::Cancelled
    } else if scan_budget_exhausted
        || permission_denied_count > 0
        || skipped_count > 0
        || warning_count > 0
    {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    let coverage = CoverageReport {
        state,
        roots_scanned: roots_completed,
        roots_discovered: roots_total,
        permission_denied_count,
        skipped_count,
        warning_count,
    };
    index
        .finish_scan(
            &scan_id,
            current_epoch_ms(),
            if cancelled {
                "cancelled"
            } else if scan_budget_exhausted {
                "partial"
            } else {
                "completed"
            },
            cancelled,
            files_scanned,
            calls_added,
            warning_count,
        )
        .await?;
    let aggregate = index
        .aggregate_for_provider(loki_metis_core::ProviderKind::RolloutJsonl)
        .await?;
    Ok(ScanSummary {
        scan_id,
        coverage,
        files_scanned,
        unchanged_files,
        rebuilt_files,
        calls_added,
        aggregate,
    })
}
