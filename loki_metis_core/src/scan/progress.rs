//! 扫描进度事件与可轮询视图的纯业务映射（GUI/CLI/MCP 可复用）。

use crate::{ScanKind, client_ports::LocalScanProgress};

/// 扫描可见阶段编码（UI 文本通过对应 label 决定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProgressScopeCode {
    /// 正在发现本地卷。
    DiscoveringVolumes,
    /// 已完成本轮发现统计。
    DiscoveryFinished,
    /// 正在索引已知根。
    IndexingRoots,
}

/// 表示阶段内可展示的脱敏计数快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanProgressScope {
    /// 已检查的目录数（仅发现阶段填充）。
    pub directories_scanned: u64,
    /// 已发现的数据根数（仅发现阶段填充）。
    pub roots_discovered: u64,
    /// 已完成的根数（仅索引阶段填充）。
    pub roots_completed: u64,
    /// 目标根数（仅索引阶段填充）。
    pub roots_total: u64,
}

/// 输入给扫描进度映射的统一事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanProgressEvent {
    /// Quick/FullDevice 发现阶段阶段快照。
    Discovering {
        /// 已遍历目录数。
        directories_scanned: u64,
        /// 已确认根数。
        roots_discovered: u64,
        /// 本次发现允许遍历的目录上限。
        max_directories: u64,
    },
    /// 当前批次发现已结束。
    DiscoveryFinished {
        /// 发现阶段总计遍历的目录数。
        directories_scanned: u64,
        /// 发现阶段总计确认的根数。
        roots_discovered: u64,
    },
    /// 索引阶段持续进度。
    Indexing {
        /// 当前扫描模式。
        kind: ScanKind,
        /// 当前正在索引的数据根稳定 ID；不包含绝对路径。
        current_root_id: Option<String>,
        /// 已完成索引的根数。
        roots_completed: u64,
        /// 待索引根总数。
        roots_total: u64,
        /// 已扫描文件数。
        files_scanned: u64,
        /// 本轮新增 canonical 调用数。
        calls_added: u64,
    },
}

/// 表示扫描进度可轮询快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanTaskProgress {
    /// 本轮已访问文件数（如来源于索引阶段）。
    pub files_visited: u64,
    /// 本轮新增调用数（如来源于索引阶段）。
    pub calls_indexed: u64,
    /// 0–10_000 基点。
    pub progress_basis_points: u16,
    /// 当前正在索引的数据根稳定 ID；发现阶段为空。
    pub current_root_id: Option<String>,
    /// 当前扫描阶段编码。
    pub current_scope_code: ScanProgressScopeCode,
    /// 当前阶段可展示的计数快照。
    pub scope_progress: ScanProgressScope,
    /// 当前阶段脱敏标签。
    pub current_scope_label: String,
}

/// 生成 “正在发现本地卷”阶段标签。
pub fn scan_discovering_scope_label(directories_scanned: u64, roots_discovered: u64) -> String {
    format!("正在发现本地卷：已检查 {directories_scanned} 个目录，发现 {roots_discovered} 个数据根")
}

/// 生成 “发现完成”阶段标签。
pub fn scan_discovery_finished_label(directories_scanned: u64, roots_discovered: u64) -> String {
    format!("发现完成：已检查 {directories_scanned} 个目录，发现 {roots_discovered} 个数据根")
}

/// 生成 “已完成 N / M 个数据根”阶段标签。
pub fn scan_indexing_scope_label(roots_completed: u64, roots_total: u64) -> String {
    format!("已完成 {roots_completed} / {roots_total} 个数据根")
}

/// 把扫描事件映射为统一、可共享的进度可见模型。
pub fn scan_task_progress_view(progress: ScanProgressEvent) -> ScanTaskProgress {
    match progress {
        ScanProgressEvent::Discovering {
            directories_scanned,
            roots_discovered,
            max_directories,
        } => {
            let basis_points = if max_directories == 0 || directories_scanned == 0 {
                0
            } else {
                directories_scanned
                    .saturating_mul(2_500)
                    .checked_div(max_directories)
                    .unwrap_or_default()
                    .clamp(1, 2_499)
            };
            ScanTaskProgress {
                files_visited: 0,
                calls_indexed: 0,
                progress_basis_points: u16::try_from(basis_points).unwrap_or(2_499),
                current_root_id: None,
                current_scope_code: ScanProgressScopeCode::DiscoveringVolumes,
                scope_progress: ScanProgressScope {
                    directories_scanned,
                    roots_discovered,
                    ..ScanProgressScope::default()
                },
                current_scope_label: scan_discovering_scope_label(
                    directories_scanned,
                    roots_discovered,
                ),
            }
        }
        ScanProgressEvent::DiscoveryFinished {
            directories_scanned,
            roots_discovered,
        } => ScanTaskProgress {
            files_visited: 0,
            calls_indexed: 0,
            progress_basis_points: 2_500,
            current_root_id: None,
            current_scope_code: ScanProgressScopeCode::DiscoveryFinished,
            scope_progress: ScanProgressScope {
                directories_scanned,
                roots_discovered,
                ..ScanProgressScope::default()
            },
            current_scope_label: scan_discovery_finished_label(
                directories_scanned,
                roots_discovered,
            ),
        },
        ScanProgressEvent::Indexing {
            kind,
            current_root_id,
            roots_completed,
            roots_total,
            files_scanned,
            calls_added,
        } => {
            let root_basis_points = if roots_total == 0 {
                0
            } else {
                roots_completed
                    .saturating_mul(10_000)
                    .checked_div(roots_total)
                    .unwrap_or_default()
                    .min(10_000)
            };
            let basis_points = match kind {
                ScanKind::Quick => root_basis_points,
                ScanKind::FullDevice => {
                    2_500_u64.saturating_add(root_basis_points.saturating_mul(7_500) / 10_000)
                }
            };

            ScanTaskProgress {
                files_visited: files_scanned,
                calls_indexed: calls_added,
                progress_basis_points: u16::try_from(basis_points).unwrap_or(10_000),
                current_root_id,
                current_scope_code: ScanProgressScopeCode::IndexingRoots,
                scope_progress: ScanProgressScope {
                    roots_completed,
                    roots_total,
                    ..ScanProgressScope::default()
                },
                current_scope_label: scan_indexing_scope_label(roots_completed, roots_total),
            }
        }
    }
}

impl LocalScanProgress {
    /// 把本地扫描阶段事件转换为统一进度快照。
    pub fn to_task_progress(self) -> ScanTaskProgress {
        scan_task_progress_view(match self {
            LocalScanProgress::Discovering {
                directories_scanned,
                roots_discovered,
                max_directories,
            } => ScanProgressEvent::Discovering {
                directories_scanned,
                roots_discovered,
                max_directories,
            },
            LocalScanProgress::DiscoveryFinished {
                directories_scanned,
                roots_discovered,
            } => ScanProgressEvent::DiscoveryFinished {
                directories_scanned,
                roots_discovered,
            },
            LocalScanProgress::Indexing {
                kind,
                current_root_id,
                roots_completed,
                roots_total,
                files_scanned,
                calls_added,
            } => ScanProgressEvent::Indexing {
                kind,
                current_root_id,
                roots_completed,
                roots_total,
                files_scanned,
                calls_added,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证完全扫描的发现与索引阶段映射到统一进度模型。
    fn maps_full_device_discovery_and_indexing_to_shared_progress() {
        let discovering = scan_task_progress_view(ScanProgressEvent::Discovering {
            directories_scanned: 1,
            roots_discovered: 0,
            max_directories: 200_000,
        });
        let discovery_finished = scan_task_progress_view(ScanProgressEvent::DiscoveryFinished {
            directories_scanned: 20,
            roots_discovered: 2,
        });
        let indexing = scan_task_progress_view(ScanProgressEvent::Indexing {
            kind: ScanKind::FullDevice,
            current_root_id: Some("root-a".to_owned()),
            roots_completed: 1,
            roots_total: 2,
            files_scanned: 4,
            calls_added: 7,
        });
        assert_eq!(discovering.progress_basis_points, 1);
        assert!(discovering.current_scope_label.contains("已检查 1 个目录"),);
        assert_eq!(discovery_finished.progress_basis_points, 2_500);
        assert_eq!(indexing.progress_basis_points, 6_250);
        assert_eq!(indexing.files_visited, 4);
        assert_eq!(indexing.calls_indexed, 7);
        assert_eq!(indexing.current_root_id.as_deref(), Some("root-a"));
    }

    #[test]
    /// 验证快速扫描没有发现配额时仍正确报告索引进度。
    fn maps_quick_indexing_without_discovery_quota() {
        let quick = scan_task_progress_view(ScanProgressEvent::Indexing {
            kind: ScanKind::Quick,
            current_root_id: Some("root-quick".to_owned()),
            roots_completed: 1,
            roots_total: 4,
            files_scanned: 2,
            calls_added: 3,
        });
        assert_eq!(
            quick.current_scope_code,
            ScanProgressScopeCode::IndexingRoots
        );
        assert_eq!(quick.progress_basis_points, 2_500);
        assert_eq!(quick.current_root_id.as_deref(), Some("root-quick"));
    }

    #[test]
    /// 验证各类本机扫描进度事件映射为稳定任务进度。
    fn maps_local_scan_progress_variants_to_task_progress() {
        let discovering = LocalScanProgress::Discovering {
            directories_scanned: 1,
            roots_discovered: 0,
            max_directories: 200_000,
        }
        .to_task_progress();
        let discovered = LocalScanProgress::DiscoveryFinished {
            directories_scanned: 20,
            roots_discovered: 2,
        }
        .to_task_progress();
        let indexing = LocalScanProgress::Indexing {
            kind: ScanKind::FullDevice,
            current_root_id: Some("root-local".to_owned()),
            roots_completed: 1,
            roots_total: 2,
            files_scanned: 4,
            calls_added: 7,
        }
        .to_task_progress();

        assert_eq!(discovering.progress_basis_points, 1);
        assert!(discovering.current_scope_label.contains("已检查 1 个目录"));
        assert_eq!(discovered.progress_basis_points, 2_500);
        assert_eq!(indexing.progress_basis_points, 6_250);
        assert_eq!(indexing.files_visited, 4);
        assert_eq!(indexing.calls_indexed, 7);
        assert_eq!(indexing.current_root_id.as_deref(), Some("root-local"));
    }
}
