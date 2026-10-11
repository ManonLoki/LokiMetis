//! 回归手动子树深搜的有限预算、固定 deadline 和迟到写隔离。

use super::*;

/// 手动子树不得沿用 FullDiscoveryOptions 的无限生产默认值。
#[test]
fn manual_subtree_uses_explicit_finite_traversal_and_signature_budgets() {
    let selected = PathBuf::from("/selected");
    let options = manual_subtree_discovery_options(selected.clone());

    assert_eq!(options.search_roots, vec![selected]);
    assert_eq!(options.max_directories, MANUAL_SUBTREE_DIRECTORY_LIMIT);
    assert_eq!(options.max_entries, MANUAL_SUBTREE_ENTRY_LIMIT);
    assert_eq!(options.max_traversal_batches, MANUAL_SUBTREE_BATCH_LIMIT);
    assert_eq!(
        options.max_signature_directories,
        MANUAL_SUBTREE_SIGNATURE_DIRECTORY_LIMIT
    );
    assert_eq!(
        options.max_signature_entries,
        MANUAL_SUBTREE_SIGNATURE_ENTRY_LIMIT
    );
    assert_eq!(
        options.max_signature_files,
        MANUAL_SUBTREE_SIGNATURE_FILE_LIMIT
    );
    assert_eq!(
        options.max_signature_bytes,
        MANUAL_SUBTREE_SIGNATURE_BYTE_LIMIT
    );
    assert!(options.max_directories < u64::MAX);
    assert!(options.max_entries < u64::MAX);
    assert!(options.max_signature_directories < u64::MAX);
    assert!(options.max_signature_entries < u64::MAX);
    assert!(options.max_signature_files < u64::MAX);
    assert!(options.max_signature_bytes < u64::MAX);
}

/// 固定 deadline 关闭交付门后，迟到进度、候选和完成都不得覆盖失败终态。
#[tokio::test]
async fn manual_subtree_deadline_rejects_all_late_worker_writes() {
    let coordinator = Arc::new(RootDiscoveryCoordinator::default());
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::ManualSubtree,
        1,
    ));
    let cancellation = CancellationToken::new();
    let control = ManualSubtreeRunControl::new(Arc::clone(&coordinator), cancellation.clone());
    let deadline = Instant::now();

    supervise_manual_subtree_boundaries(control.clone(), deadline, || false).await;
    assert!(cancellation.is_cancelled());
    let timed_out = coordinator.snapshot();
    assert_eq!(timed_out.lifecycle, RootDiscoveryLifecycle::Failed);
    assert_eq!(
        timed_out.error_code.as_deref(),
        Some(MANUAL_SUBTREE_DEADLINE_ERROR)
    );

    assert!(!control.publish_progress(
        DiscoveryProgress {
            directories_scanned: 99,
            roots_discovered: 1,
        },
        deadline,
    ));
    assert!(
        control
            .prepare_candidate_delivery(
                SourceClientKind::Codex,
                RootCandidateEvidence::CodexRollout,
                Path::new("/late"),
                deadline,
            )
            .is_none()
    );
    assert!(!control.complete(
        &CoverageReport {
            state: CoverageState::Complete,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        },
        deadline,
    ));

    assert!(coordinator.candidates().is_empty());
    assert_eq!(coordinator.snapshot(), timed_out);
}

/// gate 等待跨过 deadline 时必须在获锁后重新采样，不得使用排队前时刻。
#[test]
fn manual_subtree_rechecks_deadline_after_waiting_for_delivery_gate() {
    let coordinator = Arc::new(RootDiscoveryCoordinator::default());
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::ManualSubtree,
        1,
    ));
    let control = ManualSubtreeRunControl::new(Arc::clone(&coordinator), CancellationToken::new());
    let polling_control = control.clone();
    let held_gate = control.lock_delivery();
    let deadline = Instant::now() + Duration::from_millis(1);
    let polling = std::thread::spawn(move || polling_control.poll(deadline));

    while Instant::now() < deadline {
        std::thread::yield_now();
    }
    drop(held_gate);

    assert!(
        !polling
            .join()
            .expect("deadline poll reaches terminal state")
    );
    let status = coordinator.snapshot();
    assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Failed);
    assert_eq!(
        status.error_code.as_deref(),
        Some(MANUAL_SUBTREE_DEADLINE_ERROR)
    );
}

/// 事件回调阻塞时不得占有交付锁，且事件所指候选必须已可查询。
#[test]
fn blocked_candidate_emit_keeps_backend_candidate_and_does_not_block_deadline() {
    let coordinator = Arc::new(RootDiscoveryCoordinator::default());
    assert!(coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        RootDiscoveryPlatform::Other,
        RootDiscoveryScope::ManualSubtree,
        1,
    ));
    let control = ManualSubtreeRunControl::new(Arc::clone(&coordinator), CancellationToken::new());
    let deadline = Instant::now() + Duration::from_secs(60);
    let delivery_control = control.clone();
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let delivery = std::thread::spawn(move || {
        deliver_discovered_candidates(
            &delivery_control,
            SourceClientKind::Codex,
            [(
                PathBuf::from("/blocked"),
                RootCandidateEvidence::CodexRollout,
            )],
            deadline,
            |candidate| {
                entered_sender
                    .send(candidate.id)
                    .expect("emit entry is observed");
                release_receiver.recv().expect("blocked emit is released");
            },
        );
    });
    let emitted_candidate_id = entered_receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("candidate emit starts");
    let selectable = coordinator
        .select_candidates(std::slice::from_ref(&emitted_candidate_id))
        .expect("an emitted candidate is already a backend fact");
    assert_eq!(selectable.len(), 1);

    let expiry_control = control.clone();
    let (expired_sender, expired_receiver) = std::sync::mpsc::channel();
    let expiry = std::thread::spawn(move || {
        let still_accepting = expiry_control.poll_at(deadline, deadline);
        expired_sender
            .send(still_accepting)
            .expect("deadline result is observed");
    });
    let expired_while_emit_blocked = expired_receiver.recv_timeout(Duration::from_secs(1));
    release_sender.send(()).expect("release blocked emit");
    delivery
        .join()
        .expect("delivery worker reaches terminal state");
    expiry
        .join()
        .expect("deadline worker reaches terminal state");

    assert_eq!(expired_while_emit_blocked, Ok(false));
    assert_eq!(
        coordinator.snapshot().lifecycle,
        RootDiscoveryLifecycle::Failed
    );
    assert_eq!(coordinator.candidates().len(), 1);
    assert!(
        coordinator
            .select_candidates(std::slice::from_ref(&emitted_candidate_id))
            .is_ok()
    );
}
