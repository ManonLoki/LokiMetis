//! 把发现/索引两阶段任务换算为可轮询的单调进度状态。

use std::sync::Arc;

use crate::dto::{ScanScopeCodeDto, ScanScopeProgressDto};
use loki_metis_core::LocalScanProgress;
use loki_metis_core::ScanProgressScopeCode;

/// 按回调顺序同步写入纯内存快照，避免等基点事件乱序或越过任务终态。
pub(super) fn publish_scan_progress(
    scan: &Arc<crate::scan_state::ScanCoordinator>,
    scan_id: &str,
    progress: LocalScanProgress,
) {
    let view = progress.to_task_progress();
    scan.update_progress(
        scan_id,
        view.files_visited,
        view.calls_indexed,
        view.progress_basis_points,
        match view.current_scope_code {
            ScanProgressScopeCode::DiscoveringVolumes => ScanScopeCodeDto::DiscoveringVolumes,
            ScanProgressScopeCode::DiscoveryFinished => ScanScopeCodeDto::DiscoveryFinished,
            ScanProgressScopeCode::IndexingRoots => ScanScopeCodeDto::IndexingRoots,
        },
        Some(ScanScopeProgressDto {
            current_root_id: view.current_root_id,
            directories_scanned: view.scope_progress.directories_scanned,
            roots_discovered: view.scope_progress.roots_discovered,
            roots_completed: view.scope_progress.roots_completed,
            roots_total: view.scope_progress.roots_total,
        }),
        view.current_scope_label,
    );
}

#[cfg(test)]
mod tests;
