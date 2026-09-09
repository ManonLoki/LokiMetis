//! 覆盖系统索引与普通兜底在候选提交边界的取消和 deadline 合同。

use super::*;
use loki_metis_core::RootDiscoveryLifecycle;

/// 首个系统索引候选触发取消后，后续路径不能再验证或提交。
#[test]
fn indexed_path_candidates_stop_after_callback_requests_cancellation() {
    let temp = tempfile::tempdir().expect("temporary discovery scope");
    let first_root = temp.path().join("first");
    let second_root = temp.path().join("second");
    let first_path = first_root.join("sessions/rollout-first.jsonl");
    let second_path = second_root.join("sessions/rollout-second.jsonl");
    std::fs::create_dir_all(first_path.parent().expect("first parent")).unwrap();
    std::fs::create_dir_all(second_path.parent().expect("second parent")).unwrap();

    let coordinator = RootDiscoveryCoordinator::default();
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::UserPriority,
        1,
    ));
    let delivered = std::cell::Cell::new(0_usize);
    submit_indexed_path_candidates(
        &coordinator,
        &LocalVolumeRoots {
            search_roots: vec![temp.path().to_path_buf()],
            ..LocalVolumeRoots::default()
        },
        RootDiscoveryStrategy::MetadataTraversal,
        [first_path, second_path],
        &|_| {
            delivered.set(delivered.get() + 1);
            assert!(coordinator.request_cancel());
        },
    );

    assert_eq!(delivered.get(), 1);
    assert_eq!(coordinator.candidates().len(), 1);
}

/// 首个兜底候选回调触发取消后，后续 worker 结果不得再入队或向 UI 发送候选。
#[test]
fn fallback_stops_before_second_candidate_emit_after_cancellation() {
    let temp = tempfile::tempdir().expect("temporary discovery scope");
    for name in ["first", "second"] {
        let rollout = temp
            .path()
            .join(name)
            .join("sessions")
            .join(format!("rollout-{name}.jsonl"));
        fs::create_dir_all(rollout.parent().expect("rollout parent")).unwrap();
        fs::write(rollout, b"must not be opened").unwrap();
    }
    let coordinator = RootDiscoveryCoordinator::default();
    let delivered = std::cell::Cell::new(0_usize);

    discover_metadata_roots_in_with_callback(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![temp.path().to_path_buf()],
            ..LocalVolumeRoots::default()
        },
        Vec::new(),
        &|_| {
            delivered.set(delivered.get() + 1);
            assert!(coordinator.request_cancel());
        },
    );

    assert_eq!(delivered.get(), 1);
    assert_eq!(coordinator.candidates().len(), 1);
    assert_eq!(
        coordinator.snapshot().lifecycle,
        RootDiscoveryLifecycle::Cancelled
    );
}

/// 总 deadline 到达时必须以稳定失败码结束，不能把未遍历卷标成完成。
#[test]
fn fallback_total_deadline_is_visible_and_never_claims_complete() {
    let temp = tempfile::tempdir().expect("temporary discovery scope");
    let coordinator = RootDiscoveryCoordinator::default();

    discover_metadata_roots_in_with_callback_and_budget(
        &coordinator,
        LocalVolumeRoots {
            search_roots: vec![temp.path().to_path_buf()],
            ..LocalVolumeRoots::default()
        },
        Vec::new(),
        &|_| {},
        MetadataTraversalBudget::with_timeout(Duration::ZERO),
    );

    let status = coordinator.snapshot();
    assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Failed);
    assert_eq!(
        status.error_code.as_deref(),
        Some("metadata_discovery_deadline_exceeded")
    );
    assert_eq!(status.progress.volumes_completed, 0);
}
