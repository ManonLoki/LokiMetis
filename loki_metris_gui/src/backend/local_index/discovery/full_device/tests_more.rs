// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证发现真实 Codex 根后立即停止下钻，根内 logs 即使伪造会话结构也不会成为第二数据源。；验证主动发现会剪枝独立 logs 子树，即使其中伪造完整 rollout 签名也不能登记。；验证未知候选的会话区域是符号链接时不会穿越到目标 rollout。

/// 验证发现真实 Codex 根后立即停止下钻，根内 logs 即使伪造会话结构也不会成为第二数据源。
#[test]
fn full_discovery_stops_at_signed_root_and_never_registers_nested_logs() {
    let temp = tempdir().expect("synthetic volume is available");
    let codex_root = temp.path().join("profile/.codex");
    write_signature_rollout(&codex_root.join("sessions/rollout-primary.jsonl"));
    write_signature_rollout(
        &codex_root
            .join("logs")
            .join("sessions")
            .join("rollout-log-copy.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(codex_root).expect("signed root canonicalizes")
    );
    assert_ne!(result.roots[0].alias, "logs");
}

/// 验证主动发现会剪枝独立 logs 子树，即使其中伪造完整 rollout 签名也不能登记。
#[test]
fn full_discovery_prunes_standalone_logs_root_with_valid_rollout() {
    let temp = tempdir().expect("synthetic volume is available");
    let valid = temp.path().join("profile/.codex");
    let forbidden = temp.path().join("other/logs");
    write_signature_rollout(&valid.join("sessions/rollout-valid.jsonl"));
    write_signature_rollout(&forbidden.join("sessions/rollout-bait.jsonl"));

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.roots.len(), 1);
    assert_eq!(
        result.roots[0].path,
        fs::canonicalize(valid).expect("valid root canonicalizes")
    );
    assert!(result.roots.iter().all(|root| root.path != forbidden));
    assert!(result.coverage.skipped_count >= 1);
}

/// 验证未知候选的会话区域是符号链接时不会穿越到目标 rollout。
#[cfg(unix)]
#[test]
fn full_discovery_rejects_symlink_session_area() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("synthetic volume is available");
    let target = temp.path().join("linked-session-target");
    write_signature_rollout(&target.join("rollout-linked.jsonl"));
    let candidate = temp.path().join("candidate");
    fs::create_dir_all(&candidate).expect("candidate root is created");
    symlink(&target, candidate.join("sessions")).expect("session symlink is created");

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert!(result.roots.is_empty());
    assert!(result.symlink_skipped_count >= 1);
}

/// 验证主动发现收到取消后停止遍历并把覆盖标为取消。
#[test]
fn full_discovery_reports_cancellation() {
    let temp = tempdir().expect("temporary root is available");
    fs::create_dir_all(temp.path().join("nested")).expect("fixture tree is created");
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert_eq!(result.directories_scanned, 0);
    assert_eq!(result.coverage.roots_scanned, 0);
}

/// 验证进度回调触发取消后不会继续进入未知候选结构探测。
#[test]
fn progress_cancellation_stops_before_signature_probe() {
    let temp = tempdir().expect("temporary root is available");
    write_signature_rollout(&temp.path().join("sessions/rollout-cancelled.jsonl"));
    let cancellation = CancellationToken::new();

    let result = discover_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
        |_| cancellation.cancel(),
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert!(result.roots.is_empty());
    assert_eq!(result.directories_scanned, 1);
}

/// 验证已确认的数据根在随后取消时仍被保留并计入覆盖。
#[test]
fn cancellation_retains_roots_confirmed_before_the_signal() {
    let temp = tempdir().expect("temporary root is available");
    write_signature_rollout(
        &temp
            .path()
            .join("candidate/sessions/rollout-confirmed.jsonl"),
    );
    fs::create_dir_all(temp.path().join("other")).expect("second directory is created");
    let cancellation = CancellationToken::new();

    let result = discover_full_device_with_progress(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            ..FullDiscoveryOptions::default()
        },
        &cancellation,
        |progress| {
            if progress.roots_discovered == 1 {
                cancellation.cancel();
            }
        },
    );

    assert_eq!(result.coverage.state, CoverageState::Cancelled);
    assert_eq!(result.roots.len(), 1);
}

/// 验证主动遍历目录预算耗尽属于部分覆盖，而不是冒充用户取消。
#[test]
fn traversal_budget_exhaustion_is_partial() {
    let temp = tempdir().expect("temporary root is available");
    fs::create_dir_all(temp.path().join("nested")).expect("fixture tree is created");

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            max_directories: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert_eq!(result.coverage.roots_scanned, 1);
    assert_eq!(result.directories_scanned, 1);
}

/// 验证单批目录额度用尽后会继续下一批，并发现不同深度和名称的全部有效根。
#[test]
fn traversal_continues_across_bounded_batches_to_find_all_signed_roots() {
    let temp = tempdir().expect("batched traversal root is available");
    write_signature_rollout(
        &temp
            .path()
            .join("first/arbitrary/sessions/rollout-first.jsonl"),
    );
    write_signature_rollout(
        &temp
            .path()
            .join("second/deeper/location/archived_sessions/rollout-second.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![temp.path().to_path_buf()],
            max_directories: 2,
            max_entries: 3,
            max_traversal_batches: 8,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Complete);
    assert_eq!(result.roots.len(), 2);
    assert!(result.directories_scanned > 2);
}

/// 验证主动遍历目录项预算恰好用完仍完整，出现第 N+1 项才报告部分覆盖。
#[test]
fn traversal_entry_budget_distinguishes_exact_limit_from_overflow() {
    let exact = tempdir().expect("exact-budget root is available");
    File::create(exact.path().join("only-entry")).expect("single entry is created");
    let exact_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![exact.path().to_path_buf()],
            max_entries: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    let overflow = tempdir().expect("overflow root is available");
    File::create(overflow.path().join("first-entry")).expect("first entry is created");
    File::create(overflow.path().join("second-entry")).expect("second entry is created");
    let overflow_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![overflow.path().to_path_buf()],
            max_entries: 1,
            max_traversal_batches: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(exact_result.coverage.state, CoverageState::Complete);
    assert_eq!(exact_result.directories_scanned, 1);
    assert_eq!(overflow_result.coverage.state, CoverageState::Partial);
    assert_eq!(overflow_result.directories_scanned, 1);
}

/// 验证结构签名目录项预算恰好覆盖有效文件，而第 N+1 个候选触发部分覆盖。
#[test]
fn signature_entry_budget_distinguishes_exact_limit_from_overflow() {
    let exact = tempdir().expect("exact signature root is available");
    write_signature_rollout(&exact.path().join("sessions/rollout-valid.jsonl"));
    let exact_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![exact.path().to_path_buf()],
            max_signature_entries: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    let overflow = tempdir().expect("overflow signature root is available");
    fs::create_dir_all(overflow.path().join("sessions")).expect("signature area is created");
    fs::write(
        overflow.path().join("sessions/rollout-first.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("first invalid candidate is written");
    fs::write(
        overflow.path().join("sessions/rollout-second.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("second invalid candidate is written");
    let overflow_result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![overflow.path().to_path_buf()],
            max_signature_entries: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(exact_result.coverage.state, CoverageState::Complete);
    assert_eq!(exact_result.roots.len(), 1);
    assert_eq!(overflow_result.coverage.state, CoverageState::Partial);
    assert!(overflow_result.roots.is_empty());
}

/// 验证结构探测目录预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_directory_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("directory budget consumer is available");
    fs::create_dir_all(consuming_root.path().join("sessions"))
        .expect("empty sessions area is created");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-directory-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_directories: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 验证结构探测文件预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_file_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("file budget consumer is available");
    fs::create_dir_all(consuming_root.path().join("sessions")).expect("sessions area is created");
    fs::write(
        consuming_root.path().join("sessions/rollout-invalid.jsonl"),
        "{\"type\":\"ordinary\"}\n",
    )
    .expect("invalid rollout is written");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-file-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_files: 1,
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 验证结构探测字节预算由首候选消耗后，第二个有效候选不能获得新预算。
#[test]
fn signature_byte_budget_is_shared_across_candidates() {
    let consuming_root = tempdir().expect("byte budget consumer is available");
    let invalid_line = b"{\"type\":\"ordinary\"}\n";
    fs::create_dir_all(consuming_root.path().join("sessions")).expect("sessions area is created");
    fs::write(
        consuming_root.path().join("sessions/rollout-invalid.jsonl"),
        invalid_line,
    )
    .expect("invalid rollout is written");
    let valid_root = tempdir().expect("valid root is available");
    write_signature_rollout(
        &valid_root
            .path()
            .join("sessions/rollout-valid-after-byte-budget.jsonl"),
    );

    let result = discover_full_device(
        &FullDiscoveryOptions {
            search_roots: vec![
                valid_root.path().to_path_buf(),
                consuming_root.path().to_path_buf(),
            ],
            max_signature_bytes: u64::try_from(invalid_line.len()).expect("fixture is small"),
            ..FullDiscoveryOptions::default()
        },
        &CancellationToken::new(),
    );

    assert_eq!(result.coverage.state, CoverageState::Partial);
    assert!(result.roots.is_empty());
}

/// 写入只含最小允许字段的首条会话记录，不包含正文或用户路径。
fn write_signature_rollout(path: &Path) {
    fs::create_dir_all(path.parent().expect("fixture has a parent"))
        .expect("fixture parent is created");
    let mut file = File::create(path).expect("rollout fixture is created");
    writeln!(
        file,
        "{{\"timestamp\":\"2026-07-31T08:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"synthetic-session\"}}}}"
    )
    .expect("signature line is written");
}
