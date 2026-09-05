//! 遍历已签名 Claude 根：目录枚举按根级 `spawn_blocking` 岛执行，
//! 索引生命周期与写库在 async 侧串行 await。

mod enumerate;

use loki_metis_core::{CoverageReport, CoverageState};
use tauri::async_runtime::spawn_blocking;

use super::discovery::ClaudeDiscoveredRoot;
use super::index::IndexClaudeTranscript;
use crate::backend::local_index::RegisterDiscoveredRoot;
use crate::backend::local_index::discovery::path_key;
use crate::backend::local_index::scan::{ScanProgress, ScanSummary};
use crate::backend::local_index::{
    CancellationToken, LocalError, LocalErrorKind, LocalIndex, ScanConfig, current_epoch_ms,
};
use enumerate::{EnumerateWindow, RootSourceLabels, ScanCounters, enumerate_root_transcripts};

/// 只进入 `projects/<project>/<session>.jsonl` 与其 `subagents/*.jsonl`。
pub async fn scan_claude_discovered_roots<F>(
    index: &mut LocalIndex,
    roots: &[ClaudeDiscoveredRoot],
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
    let mut counters = ScanCounters {
        remaining_directories: config.max_directories,
        remaining_entries: config.max_entries,
        ..ScanCounters::default()
    };
    let mut roots_completed = 0_u64;
    let mut cancelled = false;

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
            files_scanned: counters.files_scanned,
            calls_added: counters.calls_added,
            warning_count: counters.warning_count,
        });
        index.register_claude_root(root).await?;
        let before_warnings = counters.warning_count;
        let root_clone = root.clone();
        let cancel = cancellation.clone();
        let counters_in = counters.clone();
        let window = EnumerateWindow {
            scan_since_epoch_ms: config.scan_since_epoch_ms,
            known_sources: index
                .source_file_observations_for_root(&root.root_id, false)
                .await?,
        };
        let outcome = spawn_blocking(move || {
            enumerate_root_transcripts(&root_clone, counters_in, &window, &cancel)
        })
        .await
        .map_err(|_| {
            LocalError::new(
                LocalErrorKind::SourceUnavailable,
                "Claude enumerate worker lost",
            )
        })?;
        counters = outcome.counters;
        let mut labels = outcome.labels;
        let mut enumeration_complete = outcome.enumeration_complete;
        if outcome.cancelled {
            cancelled = true;
            enumeration_complete = false;
        }
        for path in outcome.paths {
            if cancellation.is_cancelled() {
                cancelled = true;
                enumeration_complete = false;
                break;
            }
            index_one(
                index,
                root,
                &path,
                &config,
                cancellation,
                &mut labels,
                &mut counters,
            )
            .await?;
            on_progress(ScanProgress {
                scan_id: scan_id.clone(),
                current_root_id: root.root_id.clone(),
                roots_completed,
                roots_total,
                files_scanned: counters.files_scanned,
                calls_added: counters.calls_added,
                warning_count: counters.warning_count,
            });
            if cancellation.is_cancelled() {
                cancelled = true;
                enumeration_complete = false;
                break;
            }
        }
        index
            .reconcile_root_sources(
                &root.root_id,
                false,
                &labels.retained,
                &labels.rejected,
                enumeration_complete && !cancelled,
            )
            .await?;
        if !cancelled && !counters.budget_exhausted {
            roots_completed = roots_completed.saturating_add(1);
        }
        let root_state = if cancelled {
            CoverageState::Cancelled
        } else if counters.budget_exhausted || counters.warning_count > before_warnings {
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
            files_scanned: counters.files_scanned,
            calls_added: counters.calls_added,
            warning_count: counters.warning_count,
        });
        if cancelled || counters.budget_exhausted {
            break;
        }
    }

    let state = if cancelled {
        CoverageState::Cancelled
    } else if counters.budget_exhausted
        || counters.permission_denied_count > 0
        || counters.skipped_count > 0
        || counters.warning_count > 0
    {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    let coverage = CoverageReport {
        state,
        roots_scanned: roots_completed,
        roots_discovered: roots_total,
        permission_denied_count: counters.permission_denied_count,
        skipped_count: counters.skipped_count,
        warning_count: counters.warning_count,
    };
    index
        .finish_scan(
            &scan_id,
            current_epoch_ms(),
            if cancelled {
                "cancelled"
            } else if counters.budget_exhausted {
                "partial"
            } else {
                "completed"
            },
            cancelled,
            counters.files_scanned,
            counters.calls_added,
            counters.warning_count,
        )
        .await?;
    let aggregate = index
        .aggregate_for_provider(loki_metis_core::ProviderKind::ClaudeTranscriptJsonl)
        .await?;
    Ok(ScanSummary {
        scan_id,
        coverage,
        files_scanned: counters.files_scanned,
        unchanged_files: counters.unchanged_files,
        rebuilt_files: counters.rebuilt_files,
        calls_added: counters.calls_added,
        aggregate,
    })
}

/// 索引单个 transcript 文件（主 session 或 subagent），更新计数与来源标签。
async fn index_one(
    index: &mut LocalIndex,
    root: &ClaudeDiscoveredRoot,
    path: &std::path::Path,
    config: &ScanConfig,
    cancellation: &CancellationToken,
    labels: &mut RootSourceLabels,
    counters: &mut ScanCounters,
) -> Result<(), LocalError> {
    let relative = path
        .strip_prefix(&root.path)
        .map(path_key)
        .unwrap_or_default();
    match index
        .index_claude_transcript(
            root,
            path,
            config.max_line_bytes,
            config.ingest_since_epoch_ms,
            config.force_rebuild,
            cancellation,
        )
        .await
    {
        Ok(outcome) => {
            labels.retained.insert(relative);
            counters.files_scanned = counters.files_scanned.saturating_add(1);
            counters.unchanged_files = counters
                .unchanged_files
                .saturating_add(u64::from(outcome.unchanged));
            counters.rebuilt_files = counters
                .rebuilt_files
                .saturating_add(u64::from(outcome.rebuilt));
            counters.calls_added = counters.calls_added.saturating_add(outcome.added_calls);
            counters.warning_count = counters
                .warning_count
                .saturating_add(outcome.warnings.total());
            let _ = (&outcome.source_id, outcome.trailing_bytes);
            if outcome.cancelled {
                cancellation.cancel();
            }
        }
        Err(error) if is_transient_source_error(error.kind()) => {
            labels.retained.insert(relative);
            if error.kind() == LocalErrorKind::PermissionDenied {
                counters.permission_denied_count =
                    counters.permission_denied_count.saturating_add(1);
            } else {
                counters.skipped_count = counters.skipped_count.saturating_add(1);
            }
            counters.warning_count = counters.warning_count.saturating_add(1);
        }
        Err(error)
            if matches!(
                error.kind(),
                LocalErrorKind::InvalidPath | LocalErrorKind::InvalidUsage
            ) =>
        {
            labels.rejected.insert(relative);
            counters.warning_count = counters.warning_count.saturating_add(1);
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

/// 临时权限或竞态不可用只降低覆盖，不能删除上次已就绪的来源 generation。
const fn is_transient_source_error(kind: LocalErrorKind) -> bool {
    matches!(
        kind,
        LocalErrorKind::PermissionDenied | LocalErrorKind::SourceUnavailable
    )
}

#[cfg(test)]
mod tests;
