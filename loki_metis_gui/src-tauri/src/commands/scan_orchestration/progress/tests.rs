use std::sync::Arc;

use loki_metis_core::{LocalScanProgress, ScanCancellation, ScanKind};

use super::publish_scan_progress;
use crate::dto::{ScanKindDto, ScanStateDto};
use crate::scan_state::ScanCoordinator;

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

/// 验证发布在回调返回前有序生效，且旧 scan_id 不能污染下一轮扫描。
#[tokio::test]
async fn published_progress_is_immediate_ordered_and_scan_scoped() {
    let coordinator = Arc::new(ScanCoordinator::default());
    let first = coordinator
        .start(ScanKindDto::Quick, 1, ScanCancellation::new())
        .await
        .expect("first scan starts");

    for (files_scanned, calls_added) in [(1, 2), (2, 3)] {
        publish_scan_progress(
            &coordinator,
            &first.scan_id,
            LocalScanProgress::Indexing {
                kind: ScanKind::Quick,
                current_root_id: Some("root-progress".to_owned()),
                roots_completed: 0,
                roots_total: 1,
                files_scanned,
                calls_added,
            },
        );
    }
    let first_status = coordinator.snapshot().await;
    assert_eq!(first_status.files_visited, 2);
    assert_eq!(first_status.calls_indexed, 3);

    assert!(coordinator.finish_completed(&first.scan_id, 2).await);
    let second = coordinator
        .start(ScanKindDto::FullDevice, 1, ScanCancellation::new())
        .await
        .expect("replacement scan starts");
    publish_scan_progress(
        &coordinator,
        &first.scan_id,
        LocalScanProgress::Indexing {
            kind: ScanKind::Quick,
            current_root_id: Some("stale-root".to_owned()),
            roots_completed: 1,
            roots_total: 1,
            files_scanned: 99,
            calls_added: 99,
        },
    );

    let second_status = coordinator.snapshot().await;
    assert_eq!(
        second_status.scan_id.as_deref(),
        Some(second.scan_id.as_str())
    );
    assert_eq!(second_status.state, ScanStateDto::Running);
    assert_eq!(second_status.files_visited, 0);
    assert_eq!(second_status.calls_indexed, 0);
}
