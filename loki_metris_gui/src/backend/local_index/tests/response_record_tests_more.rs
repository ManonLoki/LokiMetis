// 本文件是 response_record_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：旧 parser/schema 与旧警告只能通过重读原始文件解除，不能靠缺字段补零。；重建先按首次发生时间决定保留窗口，重放时间变化既不能丢账也不能延长保留期。

/// 旧 parser/schema 与旧警告只能通过重读原始文件解除，不能靠缺字段补零。
#[test]
fn parser_upgrade_rebuilds_old_authority_and_quality() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    write_rollout(
        &path,
        "owner",
        &[
            response("2026-07-30T10:01:00Z", "first", 10),
            token_line("2026-07-30T10:02:00Z", "shadow", 999, 0),
        ],
    );
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    scan(&mut index, &roots, &CancellationToken::new());
    let connection = reopen_for_fixture(index.database_path());
    execute_fixture_sql(
        &connection,
        &format!(
            "UPDATE source_files SET parser_version = {},
        adapter_state = '{{\"schemaVersion\":2,\"ownership\":{{\"state\":\"owned\"}}}}'",
            PARSER_VERSION - 1
        ),
    );
    drop(connection);
    drop(index);
    let mut index = open_index(app.path());
    let rebuilt = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(
        (
            rebuilt.rebuilt_files,
            rebuilt.coverage.state,
            rebuilt.coverage.warning_count
        ),
        (1, CoverageState::Complete, 0)
    );
    assert_eq!(totals(&mut index), (1, 20));
}

/// 重建先按首次发生时间决定保留窗口，重放时间变化既不能丢账也不能延长保留期。
#[test]
fn response_rebuild_retention_uses_first_observed_time() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    write_rollout(
        &path,
        "owner",
        &[response("2026-07-30T10:01:00Z", "same", 10)],
    );
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    scan(&mut index, &roots, &CancellationToken::new());
    write_rollout(
        &path,
        "owner",
        &[response("2020-01-01T00:00:00Z", "same", 10)],
    );
    let config = ScanConfig {
        force_rebuild: true,
        ingest_since_epoch_ms: 1_700_000_000_000,
        ..ScanConfig::default()
    };
    let result = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(result.coverage.warning_count, 0);
    assert_eq!(
        totals(&mut index),
        (1, 20),
        "raw replay time cannot remove a retained original"
    );
    write_rollout(
        &path,
        "owner",
        &[response("2040-01-01T00:00:00Z", "same", 10)],
    );
    let config = ScanConfig {
        ingest_since_epoch_ms: 1_900_000_000_000,
        ..config
    };
    let result = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(result.coverage.warning_count, 0);
    assert_eq!(
        totals(&mut index),
        (0, 0),
        "late replay cannot extend the original retention"
    );
}
