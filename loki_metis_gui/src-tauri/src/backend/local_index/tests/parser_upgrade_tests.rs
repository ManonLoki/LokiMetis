use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use loki_metis_core::{CoverageState, LocalIndexState};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    append_rollout, create_root, discover_registered, execute_fixture_sql, reopen_for_fixture,
    scan, token_line, write_rollout,
};
use crate::backend::local_index::{
    CancellationToken, LocalIndex, PARSER_VERSION, ScanConfig, ScanMode, scan_discovered_roots,
};

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 验证升级后旧 parser generation 在用户重扫前不可读，防止历史不安全标签展示。
// 测试手法：先正常扫描写入一条记录，再绕开正常写入路径，直接用裸
// SQL 把这条记录的 parser_version 强行改成一个更旧的版本号，同时把
// model/reasoning_effort 字段改写成看起来像绝对路径的字符串——这样
// 一旦查询代码有 bug、错误地把“旧 generation”的记录也读出来展示，
// 断言就能立刻通过内容检测到路径泄露；如果查询代码正确地按
// `parser_version = 当前版本` 过滤，这条被人为改旧的记录应该完全不出现
// 在任何当前读取结果里。
#[test]
fn excludes_older_parser_generations_from_all_current_reads() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root-old-parser");
    write_rollout(
        &root_path.join("sessions/rollout-old.jsonl"),
        "session-old",
        &[token_line("2026-07-30T10:01:00Z", "call-old", 100, 40)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    let database_path = index.database_path().to_path_buf();
    {
        let connection = reopen_for_fixture(&database_path);
        execute_fixture_sql(
            &connection,
            &format!(
                "UPDATE source_files SET parser_version = {}",
                PARSER_VERSION.saturating_sub(1)
            ),
        );
        execute_fixture_sql(
            &connection,
            "UPDATE usage_calls
             SET model = '/Users/alice/secret',
                 reasoning_effort = 'C:/Users/alice/secret'",
        );
    }

    let canonical = block_on(index.canonical_calls()).expect("current calls load");
    let snapshot = block_on(index.usage_snapshot()).expect("current snapshot loads");
    let root_records = block_on(index.list_sources()).expect("current roots load");

    assert!(canonical.calls.is_empty());
    assert!(snapshot.canonical.calls.is_empty());
    assert_eq!(snapshot.index_state, LocalIndexState::NeedsRescan);
    assert_eq!(root_records[0].source_file_count, 0);
    assert_eq!(root_records[0].call_observation_count, 0);

    block_on(index.set_root_enabled(&roots[0].root_id, false)).expect("stale root is disabled");
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("disabled stale snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );
    block_on(index.set_root_enabled(&roots[0].root_id, true)).expect("stale root is re-enabled");
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("restored stale snapshot loads")
            .index_state,
        LocalIndexState::NeedsRescan
    );
}

/// 验证强制重建时，修改时间与 checkpoint 不一致的窗口外旧文件会被重新读取，
/// 其保留窗口外的调用由摄入下界丢弃，且不得继续用 leftover 旧 generation 挡住当前事实。
#[test]
fn force_rebuild_with_mtime_skip_clears_needs_rescan_without_mixing_old_generation() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root-mtime-skip");
    let stale_rollout = root_path.join("sessions/rollout-stale-window.jsonl");
    let current_rollout = root_path.join("sessions/rollout-current-window.jsonl");
    write_rollout(
        &stale_rollout,
        "session-stale",
        &[token_line("2020-01-02T10:01:00Z", "call-stale", 700, 70)],
    );
    write_rollout(
        &current_rollout,
        "session-current",
        &[token_line("2026-07-30T10:01:00Z", "call-current", 100, 40)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    {
        let connection = reopen_for_fixture(index.database_path());
        execute_fixture_sql(
            &connection,
            &format!(
                "UPDATE source_files SET parser_version = {}",
                PARSER_VERSION.saturating_sub(1)
            ),
        );
        execute_fixture_sql(
            &connection,
            "UPDATE usage_calls SET model = '/Users/alice/secret'",
        );
    }
    let stale_file = fs::File::options()
        .write(true)
        .open(&stale_rollout)
        .expect("stale rollout opens for mtime");
    stale_file
        .set_modified(UNIX_EPOCH + Duration::from_secs(86_400))
        .expect("stale rollout mtime is older than the retention window");
    drop(stale_file);
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("mixed parser snapshot before rebuild loads")
            .index_state,
        LocalIndexState::NeedsRescan
    );

    let rebuilt = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            mode: ScanMode::Quick,
            scan_since_epoch_ms: 1_700_000_000_000,
            ingest_since_epoch_ms: 1_700_000_000_000,
            force_rebuild: true,
            started_at_epoch_ms: Some(2_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("forced rebuild with retention skip succeeds");
    let snapshot = block_on(index.usage_snapshot()).expect("rebuilt snapshot loads");

    assert_eq!(
        rebuilt.rebuilt_files, 2,
        "the backdated source changed against its checkpoint and must be re-read"
    );
    assert_eq!(snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(snapshot.canonical.calls.len(), 1);
    assert_eq!(snapshot.canonical.calls[0].usage.input_tokens, 100);
    assert!(
        !format!("{:?}", snapshot.canonical.calls).contains("/Users/alice/secret"),
        "old-generation labels must stay out of the current snapshot"
    );
}

/// 验证完整重扫会清理已删除的旧 parser 来源；同一根上已有当前 generation 时 leftover 旧行不得挡住概览。
#[test]
fn complete_rescan_prunes_missing_stale_sources() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root-prune");
    let retained_rollout = root_path.join("sessions/rollout-retained.jsonl");
    let removed_rollout = root_path.join("sessions/rollout-removed.jsonl");
    write_rollout(
        &retained_rollout,
        "session-retained",
        &[token_line("2026-07-30T10:01:00Z", "call-retained", 100, 40)],
    );
    write_rollout(
        &removed_rollout,
        "session-removed",
        &[token_line("2026-07-30T10:02:00Z", "call-removed", 200, 80)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    {
        let connection = reopen_for_fixture(index.database_path());
        execute_fixture_sql(
            &connection,
            &format!(
                "UPDATE source_files SET parser_version = {}
                 WHERE source_id = (
                   SELECT source_id FROM usage_calls WHERE input_tokens = 200 LIMIT 1
                 )",
                PARSER_VERSION.saturating_sub(1)
            ),
        );
    }
    fs::remove_file(&removed_rollout).expect("isolated stale rollout is removed");
    append_rollout(
        &retained_rollout,
        "{\"timestamp\":\"2026-07-30T10:03:00Z\",\"type\":\"event_msg\",\
         \"payload\":{\"type\":\"future_event\"}}\n",
    );
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("mixed parser snapshot loads")
            .index_state,
        LocalIndexState::Ready
    );

    let rescan = scan(&mut index, &roots, &CancellationToken::new());
    let snapshot = block_on(index.usage_snapshot()).expect("rescanned snapshot loads");

    assert_eq!(rescan.coverage.state, CoverageState::Partial);
    assert!(rescan.coverage.warning_count > 0);
    assert_eq!(snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(snapshot.canonical.calls.len(), 1);
    assert_eq!(snapshot.canonical.calls[0].usage.input_tokens, 100);
    assert_eq!(snapshot.roots[0].source_file_count, 1);
}

/// 验证无法确认是否可读的区域不会被完整根的其他区域误清理。
#[test]
fn uncertain_area_preserves_existing_stale_sources() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root-uncertain");
    write_rollout(
        &root_path.join("sessions/rollout-uncertain.jsonl"),
        "session-uncertain",
        &[token_line(
            "2026-07-30T10:01:00Z",
            "call-uncertain",
            100,
            40,
        )],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    let mut uncertain_root = roots[0].clone();
    uncertain_root.has_sessions = false;
    uncertain_root.sessions_inspection_complete = false;
    let incomplete = scan(
        &mut index,
        std::slice::from_ref(&uncertain_root),
        &CancellationToken::new(),
    );
    let current_snapshot =
        block_on(index.usage_snapshot()).expect("current uncertain snapshot loads");
    assert_eq!(incomplete.coverage.state, CoverageState::Partial);
    assert!(incomplete.coverage.skipped_count > 0);
    assert_eq!(current_snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(
        current_snapshot.roots[0].last_coverage,
        Some(CoverageState::Partial)
    );

    {
        let connection = reopen_for_fixture(index.database_path());
        execute_fixture_sql(
            &connection,
            &format!(
                "UPDATE source_files SET parser_version = {}",
                PARSER_VERSION.saturating_sub(1)
            ),
        );
    }

    scan(&mut index, &[uncertain_root], &CancellationToken::new());
    let snapshot = block_on(index.usage_snapshot()).expect("uncertain snapshot loads");

    assert_eq!(snapshot.index_state, LocalIndexState::NeedsRescan);
}
