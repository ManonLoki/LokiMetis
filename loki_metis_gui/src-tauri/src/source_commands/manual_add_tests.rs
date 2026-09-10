//! 回归手动添加预约、候选取消与 Claude subagent 证据保留。

use std::cell::Cell;

use super::*;

/// 原子预约必须拒绝并发流程，并在 RAII guard 销毁后立即恢复可用。
#[test]
fn manual_add_reservation_is_non_blocking_and_released_on_drop() {
    MANUAL_ADD_BUSY.store(false, Ordering::Release);
    let reservation = ManualAddReservation::acquire().expect("first flow reserves the picker");
    assert_eq!(
        ManualAddReservation::acquire().err(),
        Some("manual add is already in progress")
    );

    drop(reservation);

    ManualAddReservation::acquire().expect("reservation is released on every exit path");
}

/// 首个候选事件触发取消后，已发事件的候选保留，第二个结果不得提交。
#[test]
fn manual_subtree_stops_result_delivery_after_callback_cancellation() {
    let coordinator = Arc::new(RootDiscoveryCoordinator::default());
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::ManualSubtree,
        1,
    ));
    let control = ManualSubtreeRunControl::new(Arc::clone(&coordinator), CancellationToken::new());
    let deadline = Instant::now() + Duration::from_secs(1);
    let delivered = Cell::new(0_usize);
    deliver_discovered_candidates(
        &control,
        SourceClientKind::Codex,
        [
            (PathBuf::from("/first"), RootCandidateEvidence::CodexRollout),
            (
                PathBuf::from("/second"),
                RootCandidateEvidence::CodexRollout,
            ),
        ],
        deadline,
        |_| {
            delivered.set(delivered.get().saturating_add(1));
            assert!(coordinator.request_cancel());
        },
    );

    assert_eq!(delivered.get(), 1);
    assert_eq!(coordinator.candidates().len(), 1);
    let status = coordinator.snapshot();
    assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Cancelled);
    assert_eq!(status.progress.volumes_completed, 0);
}

/// 验证子树深搜命中 subagent transcript 时保留其专属证据类型。
#[test]
fn manual_subtree_preserves_subagent_evidence() {
    let temp = tempfile::tempdir().expect("temporary Claude root is available");
    let root = temp.path().join("custom-claude");
    let session_id = "00000000-0000-0000-0000-000000000001";
    let transcript = root.join(format!(
        "projects/project-a/{session_id}/subagents/agent-synthetic.jsonl"
    ));
    std::fs::create_dir_all(transcript.parent().expect("transcript has a parent"))
        .expect("subagent directory is created");
    std::fs::write(
        transcript,
        format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"id\":\"synthetic-subagent\",\"model\":\"claude-synthetic\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
        ),
    )
    .expect("synthetic transcript is written");
    let options = FullDiscoveryOptions {
        search_roots: vec![root],
        ..FullDiscoveryOptions::default()
    };
    let discovered =
        discover_claude_full_device_with_progress(&options, &CancellationToken::new(), |_| {});

    let candidates = claude_manual_candidates(discovered.roots);

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].1, RootCandidateEvidence::ClaudeSubagent);
}
