//! 使用真实 SQLite 连续扫描验证质量事实不会被 unchanged、重启或合法追加清零。

use loki_metris_core::CoverageState;
use std::time::{Duration, UNIX_EPOCH};
use tempfile::TempDir;

use super::support::{
    append_rollout, create_root, discover_registered, execute_fixture_sql, reopen_for_fixture,
    scan, token_line, write_rollout,
};
use crate::local_index::test_runtime::block_on;
use crate::local_index::{
    CancellationToken, LocalIndex, PARSER_VERSION, ScanConfig, scan_discovered_roots,
    file_source::{RolloutFileInspection, RolloutProbeBudget, inspect_rollout_file},
    rollout_ingest::IndexRolloutFile,
};

/// 打开完全隔离的索引，避免访问用户数据。
fn open_index(path: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(path, PARSER_VERSION)).expect("isolated index opens")
}

/// 已越过 offset 的坏行仍存在于当前 generation，必须持续抑制完整覆盖资格。
#[test]
fn consumed_bad_line_remains_partial_after_unchanged_restart_append_and_rebuild() {
    let source = TempDir::new().expect("source temp");
    let app = TempDir::new().expect("app temp");
    let root = create_root(source.path(), "root");
    let rollout = root.join("sessions/rollout-quality-checkpoint.jsonl");
    write_rollout(&rollout, "quality", &[token_line("2026-07-30T10:01:00Z", "a", 10, 2)]);
    append_rollout(&rollout, "{\"type\":\n");
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    let first = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(first.coverage.state, CoverageState::Partial);
    assert_eq!(first.coverage.warning_count, 1);
    let unchanged = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(unchanged.unchanged_files, 1);
    assert_eq!(unchanged.coverage.state, CoverageState::Partial);
    assert_eq!(unchanged.coverage.warning_count, 1);

    drop(index);
    let mut index = open_index(app.path());
    let reopened = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(reopened.coverage.state, CoverageState::Partial);
    assert_eq!(reopened.coverage.warning_count, 1);
    append_rollout(&rollout, &token_line("2026-07-30T10:02:00Z", "b", 20, 4));
    let appended = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(appended.calls_added, 1);
    assert_eq!(appended.coverage.state, CoverageState::Partial);
    assert_eq!(appended.coverage.warning_count, 1);
    let rebuilt = block_on(scan_discovered_roots(
        &mut index, &roots,
        ScanConfig { force_rebuild: true, ..ScanConfig::default() },
        &CancellationToken::new(), |_| {},
    )).expect("forced rebuild scans existing bad line");
    assert_eq!(rebuilt.rebuilt_files, 1);
    assert_eq!(rebuilt.coverage.state, CoverageState::Partial);
    assert_eq!(rebuilt.coverage.warning_count, 1);

    write_rollout(&rollout, "fixed", &[token_line("2026-07-30T10:03:00Z", "c", 7, 1)]);
    let fixed = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(fixed.rebuilt_files, 1);
    assert_eq!(fixed.coverage.state, CoverageState::Complete);
    assert_eq!(fixed.coverage.warning_count, 0);
    assert_eq!(fixed.call_count, 1);
}

/// 未知旧质量不能通过缺字段补零假装完整；必须从原始文件重新解析。
#[test]
fn missing_persisted_quality_forces_rebuild_instead_of_assuming_clean() {
    for state in [
        r#"{"schemaVersion":1,"ownership":{"state":"owned"}}"#,
        r#"{"schemaVersion":2,"ownership":{"state":"owned"}}"#,
    ] {
        let source = TempDir::new().expect("source temp");
        let app = TempDir::new().expect("app temp");
        let root = create_root(source.path(), "root");
        let rollout = root.join("sessions/rollout-quality-migration.jsonl");
        write_rollout(&rollout, "migration", &[token_line("2026-07-30T10:01:00Z", "a", 10, 2)]);
        append_rollout(&rollout, "{\"type\":\n");
        let roots = discover_registered(&[root]);
        let mut index = open_index(app.path());
        let first = scan(&mut index, &roots, &CancellationToken::new());
        assert_eq!(first.coverage.state, CoverageState::Partial);
        let connection = reopen_for_fixture(index.database_path());
        execute_fixture_sql(&connection, &format!("UPDATE source_files SET adapter_state = '{state}'"));
        drop(connection);
        let migrated = scan(&mut index, &roots, &CancellationToken::new());
        assert_eq!(migrated.rebuilt_files, 1);
        assert_eq!(migrated.coverage.state, CoverageState::Partial);
        assert_eq!(migrated.coverage.warning_count, 1);
        assert_eq!(migrated.call_count, 1);
    }
}

/// 文件不进入时间窗口时仍保留已验证的警告，显式重建则必须重新访问。
#[test]
fn retained_window_excluded_source_rechecks_identity_and_preserves_quality() {
    let source = TempDir::new().expect("source temp");
    let app = TempDir::new().expect("app temp");
    let root = create_root(source.path(), "root");
    let rollout = root.join("sessions/rollout-quality-retained.jsonl");
    write_rollout(&rollout, "retained", &[token_line("2026-07-30T10:01:00Z", "a", 10, 2)]);
    append_rollout(&rollout, "{\"type\":\n");
    std::fs::File::options().write(true).open(&rollout).expect("fixture opens")
        .set_modified(UNIX_EPOCH + Duration::from_secs(86_400)).expect("mtime predates window");
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    let first = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(first.coverage.warning_count, 1);
    for _ in 0..2 {
        let retained = block_on(scan_discovered_roots(&mut index, &roots,
            ScanConfig { scan_since_epoch_ms: 1_700_000_000_000, ..ScanConfig::default() },
            &CancellationToken::new(), |_| {})).expect("window scan retains known quality");
        assert_eq!(retained.files_scanned, 1, "known source identity must be checked even outside the window");
        assert_eq!(retained.unchanged_files, 1);
        assert_eq!(retained.call_count, 1);
        assert_eq!(retained.coverage.state, CoverageState::Partial);
        assert_eq!(retained.coverage.warning_count, 1);
    }
    let rebuilt = block_on(scan_discovered_roots(&mut index, &roots,
        ScanConfig { scan_since_epoch_ms: 1_700_000_000_000, force_rebuild: true, ..ScanConfig::default() },
        &CancellationToken::new(), |_| {})).expect("forced rebuild bypasses window");
    assert_eq!(rebuilt.rebuilt_files, 1);
    assert_eq!(rebuilt.files_scanned, 1);
    assert_eq!(rebuilt.coverage.state, CoverageState::Partial);
    assert_eq!(rebuilt.coverage.warning_count, 1);
}

/// 同根另有新版来源也不能掩盖窗口外旧来源；预算不足不解除重扫资格。
#[test]
fn mixed_parser_root_stays_stale_until_window_excluded_source_is_rebuilt() {
    use loki_metris_core::LocalIndexState;
    let source = TempDir::new().expect("source temp");
    let app = TempDir::new().expect("app temp");
    let root = create_root(source.path(), "root");
    let rollout = root.join("sessions/rollout-old-quality.jsonl");
    write_rollout(&rollout, "old", &[token_line("2026-07-30T10:01:00Z", "a", 10, 2)]);
    std::fs::File::options().write(true).open(&rollout).expect("fixture opens")
        .set_modified(UNIX_EPOCH + Duration::from_secs(86_400)).expect("mtime predates window");
    write_rollout(&root.join("sessions/rollout-current-quality.jsonl"), "current",
        &[token_line("2026-07-30T10:02:00Z", "b", 20, 4)]);
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    assert_eq!(scan(&mut index, &roots, &CancellationToken::new()).call_count, 2);
    let connection = reopen_for_fixture(index.database_path());
    execute_fixture_sql(&connection, &format!("UPDATE source_files SET parser_version = {} WHERE source_id =
        (SELECT source_id FROM usage_calls WHERE input_tokens = 10 LIMIT 1)", PARSER_VERSION - 1));
    drop(connection);
    assert_eq!(block_on(index.index_state()).expect("mixed state"), LocalIndexState::NeedsRescan);
    let limited = block_on(scan_discovered_roots(&mut index, &roots,
        ScanConfig { scan_since_epoch_ms: 1_700_000_000_000, max_signature_files: 0, ..ScanConfig::default() },
        &CancellationToken::new(), |_| {})).expect("budget exhaustion is reported");
    assert_eq!(limited.coverage.state, CoverageState::Partial);
    assert_eq!(block_on(index.index_state()).expect("stale state remains"), LocalIndexState::NeedsRescan);
    let rebuilt = block_on(scan_discovered_roots(&mut index, &roots,
        ScanConfig { scan_since_epoch_ms: 1_700_000_000_000, ..ScanConfig::default() },
        &CancellationToken::new(), |_| {})).expect("ordinary scan repairs stale source");
    assert_eq!(rebuilt.rebuilt_files, 1);
    assert_eq!(rebuilt.call_count, 2);
    assert_eq!(rebuilt.coverage.state, CoverageState::Complete);
    assert_eq!(block_on(index.index_state()).expect("all sources verified"), LocalIndexState::Ready);
}

/// 已观察却未读的追加后缀在重启后仍须继续读取，不能被窗口或 unchanged 跳过。
#[test]
fn cancelled_append_resumes_unread_suffix_with_unchanged_size_and_mtime() {
    let source = TempDir::new().expect("source temp");
    let app = TempDir::new().expect("app temp");
    let root = create_root(source.path(), "root");
    let rollout = root.join("sessions/rollout-append-cancel.jsonl");
    write_rollout(&rollout, "append-cancel", &[token_line("2026-07-30T10:01:00Z", "a", 10, 2)]);
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    assert_eq!(scan(&mut index, &roots, &CancellationToken::new()).call_count, 1);
    append_rollout(&rollout, &token_line("2026-07-30T10:02:00Z", "b", 20, 4));
    append_rollout(&rollout, "{\"type\":\n");
    append_rollout(&rollout, &token_line("2026-07-30T10:03:00Z", "c", 30, 6));
    std::fs::File::options().write(true).open(&rollout).expect("fixture opens")
        .set_modified(UNIX_EPOCH + Duration::from_secs(86_400)).expect("mtime predates window");
    let indexed_rollout = roots[0].path.join("sessions/rollout-append-cancel.jsonl");
    let validated = match inspect_rollout_file(&indexed_rollout, &CancellationToken::new(),
        &mut RolloutProbeBudget::new(1, u64::MAX)) {
        RolloutFileInspection::Matched(source) => source,
        _ => panic!("fixture signature matches"),
    };
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let outcome = block_on(index.index_rollout_file(&roots[0], validated, false,
        ScanConfig::default(), &cancellation)).expect("cancelled append saves its safe prefix");
    assert!(outcome.cancelled && !outcome.rebuilt);
    assert_eq!(outcome.added_calls, 0);
    let checkpoint = block_on(index.stored_source_file(&outcome.source_id)).unwrap().unwrap();
    assert!(checkpoint.parsed_offset > 0 && checkpoint.parsed_offset < checkpoint.observed_size);
    assert_eq!(checkpoint.trailing_bytes, 0);
    assert!(!block_on(index.usage_snapshot()).unwrap().read_ready);
    drop(index);

    let mut index = open_index(app.path());
    let config = ScanConfig { scan_since_epoch_ms: 1_700_000_000_000, ..ScanConfig::default() };
    let resumed = block_on(scan_discovered_roots(&mut index, &roots, config,
        &CancellationToken::new(), |_| {})).expect("unread suffix resumes outside window");
    assert_eq!((resumed.files_scanned, resumed.unchanged_files, resumed.calls_added, resumed.call_count), (1, 0, 2, 3));
    assert_eq!((resumed.coverage.state, resumed.coverage.warning_count), (CoverageState::Partial, 1));
    let checkpoint = block_on(index.stored_source_file(&outcome.source_id)).unwrap().unwrap();
    assert_eq!(checkpoint.generation, 1);
    assert_eq!(checkpoint.parsed_offset, checkpoint.observed_size);
    let retained = block_on(scan_discovered_roots(&mut index, &roots, config,
        &CancellationToken::new(), |_| {})).expect("fully read source retains quality");
    assert_eq!((retained.files_scanned, retained.unchanged_files, retained.coverage.warning_count), (1, 1, 1));
    let unchanged = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!((unchanged.unchanged_files, unchanged.call_count, unchanged.coverage.warning_count), (1, 3, 1));
}
