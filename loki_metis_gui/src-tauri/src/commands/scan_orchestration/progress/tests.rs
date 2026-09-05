use loki_metis_core::LocalScanProgress;
use loki_metis_core::ScanKind;

/// 验证全设备发现先占用前四分之一进度，随后索引阶段保持单调推进。
#[test]
fn full_device_progress_reports_discovery_before_indexing() {
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
        current_root_id: Some("root-progress".to_owned()),
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
    assert_eq!(indexing.current_root_id.as_deref(), Some("root-progress"));
}
