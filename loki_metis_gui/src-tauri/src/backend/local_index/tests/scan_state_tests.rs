use std::fs;

use loki_metis_core::{Confidence, CoverageReport, CoverageState, LocalIndexState};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    codex_aggregate, create_root, discover_registered, scan, token_line, write_rollout,
};
use crate::backend::local_index::{
    CancellationToken, LocalIndex, PARSER_VERSION, RegisterDiscoveredRoot,
};

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 验证冷库与完成空扫描使用不同索引状态，不能把“从未扫描”冒充已确认零用量。
#[test]
fn distinguishes_not_scanned_from_ready_without_calls() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "empty-root");
    write_rollout(
        &root_path.join("sessions/rollout-no-calls.jsonl"),
        "session-no-calls",
        &[],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("cold snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );

    scan(&mut index, &roots, &CancellationToken::new());

    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("empty scanned snapshot loads")
            .index_state,
        LocalIndexState::ReadyNoCalls
    );
}

/// 验证历史扫描不能让当前唯一启用但从未扫描的数据根冒充“已扫描空结果”。
#[test]
fn newly_enabled_unscanned_scope_remains_not_scanned() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let first_path = create_root(source_temp.path(), "first-empty-root");
    let second_path = create_root(source_temp.path(), "second-empty-root");
    write_rollout(
        &first_path.join("sessions/rollout-first-empty.jsonl"),
        "session-first-empty",
        &[],
    );
    write_rollout(
        &second_path.join("sessions/rollout-second-empty.jsonl"),
        "session-second-empty",
        &[],
    );
    let first = discover_registered(&[first_path]);
    let second = discover_registered(&[second_path]);
    let mut index = open_index(app_temp.path());

    scan(&mut index, &first, &CancellationToken::new());
    block_on(index.set_root_enabled(&first[0].root_id, false))
        .expect("previously scanned root is disabled");
    block_on(index.register_root(&second[0])).expect("new unscanned root is registered");

    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("new scope snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );

    scan(&mut index, &second, &CancellationToken::new());
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("scanned new scope snapshot loads")
            .index_state,
        LocalIndexState::ReadyNoCalls
    );
}

/// 验证失败或在任何来源完成前取消的首次扫描不能把冷库变成可信零值。
#[test]
fn failed_and_empty_cancelled_scans_remain_not_scanned() {
    let app_temp = TempDir::new().expect("app-data temp is available");
    let mut index = open_index(app_temp.path());
    for (status, cancelled) in [("failed", false), ("cancelled", true)] {
        let scan_id = block_on(index.begin_scan("quick", 1000)).expect("scan fixture starts");
        block_on(index.finish_scan(&scan_id, 1001, status, cancelled, 0, 0, 1))
            .expect("terminal scan fixture is recorded");
    }

    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("failed scan snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );
}

/// 验证真实 `sessions/YYYY/MM/DD` 目录布局会递归进入当前 parser 索引。
#[test]
fn indexes_rollout_in_nested_calendar_directories() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "nested-root");
    let rollout = root_path.join("sessions/2026/07/31/rollout-nested.jsonl");
    fs::create_dir_all(
        rollout
            .parent()
            .expect("nested rollout has a parent directory"),
    )
    .expect("nested calendar directories are created");
    write_rollout(
        &rollout,
        "session-nested",
        &[token_line("2026-07-31T08:00:00Z", "call-nested", 42, 21)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let summary = scan(&mut index, &roots, &CancellationToken::new());
    let snapshot = block_on(index.usage_snapshot()).expect("nested snapshot loads");

    assert_eq!(summary.files_scanned, 1);
    assert_eq!(summary.call_count, 1);
    assert_eq!(snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(snapshot.canonical.calls[0].usage.input_tokens, 42);
}

/// 验证多根、活动/归档与复制文件只计一次，并完整保留 provenance。
#[test]
fn deduplicates_multiple_roots_and_active_archived_copies() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let first_root = create_root(source_temp.path(), "root-a");
    let second_root = create_root(source_temp.path(), "root-b");
    let call = token_line("2026-07-30T10:01:00Z", "logical-a", 100, 40);
    write_rollout(
        &first_root.join("sessions/rollout-active.jsonl"),
        "session-shared",
        std::slice::from_ref(&call),
    );
    write_rollout(
        &first_root.join("archived_sessions/rollout-archived.jsonl"),
        "session-shared",
        std::slice::from_ref(&call),
    );
    write_rollout(
        &second_root.join("sessions/rollout-copy.jsonl"),
        "session-shared",
        &[call],
    );
    let roots = discover_registered(&[first_root, second_root]);
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    let aggregate = codex_aggregate(&mut index);
    let canonical = block_on(index.canonical_calls()).expect("canonical calls load");
    let snapshot = block_on(index.usage_snapshot()).expect("consistent snapshot loads");

    assert_eq!(first.call_count, 1);
    assert_eq!(aggregate.duplicate_source_count, 2);
    assert_eq!(aggregate.cross_root_duplicate_source_count, 1);
    assert_eq!(aggregate.root_count, 2);
    assert_eq!(aggregate.source_count, 3);
    assert_eq!(canonical.calls[0].provenance.len(), 3);
    assert_eq!(canonical.calls[0].usage.input_tokens, 100);
    assert_eq!(canonical.calls[0].confidence, Confidence::Exact);
    assert_eq!(snapshot.canonical, canonical);
    assert_eq!(snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(snapshot.roots.len(), 2);
    assert_eq!(snapshot.roots[0].alias, "测试根 0");
    assert_eq!(snapshot.roots[1].alias, "测试根 1");

    let database = fs::read(index.database_path()).expect("index bytes are readable in test");
    let database_text = String::from_utf8_lossy(&database);
    assert!(!database_text.contains("privacy-bait-answer"));
    assert!(!database_text.contains("sk-privacy-bait-secret"));
    assert!(!database_text.contains("privacy-bait@example.com"));
    assert!(!database_text.contains("/private/privacy-bait-project"));
}

/// 验证启停根在重开索引后仍同时约束概览、统计与调用，重新启用可恢复共享事实。
#[test]
fn enabled_roots_define_one_persistent_view_for_overview_statistics_and_calls() {
    use crate::calls_view::load_usage_calls;
    use crate::dto::{UsageCallsQueryDto, UsageWindow};
    use crate::local_view::load_local_windows;
    use crate::statistics_view::load_usage_statistics;
    use loki_metis_core::UsageDimension;

    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let first_root = create_root(source_temp.path(), "root-view-a");
    let second_root = create_root(source_temp.path(), "root-view-b");
    let shared = token_line("2026-07-30T10:01:00Z", "shared", 100, 40);
    write_rollout(
        &first_root.join("sessions/rollout-first.jsonl"),
        "session-shared",
        &[
            shared.clone(),
            token_line("2026-07-30T10:02:00Z", "only-a", 50, 0),
        ],
    );
    write_rollout(
        &second_root.join("sessions/rollout-second.jsonl"),
        "session-shared",
        &[shared, token_line("2026-07-30T10:03:00Z", "only-b", 70, 10)],
    );
    let roots = discover_registered(&[first_root, second_root]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    drop(index);

    let observed_at_epoch_ms = "2026-07-31T12:00:00Z"
        .parse::<jiff::Timestamp>()
        .expect("fixture observation time is valid")
        .as_millisecond();
    let coverage = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 2,
        roots_discovered: 2,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    };
    let read_counts = || {
        let overview = block_on(load_local_windows(
            app_temp.path(),
            &coverage,
            observed_at_epoch_ms,
        ))
        .expect("overview windows load");
        let overview_count = overview
            .windows
            .iter()
            .find(|window| window.window == UsageWindow::ThisWeek)
            .expect("this week overview exists")
            .fact
            .value
            .call_count;
        let statistics = block_on(load_usage_statistics(
            app_temp.path(),
            &coverage,
            UsageWindow::ThisWeek,
            UsageDimension::Model,
            observed_at_epoch_ms,
        ))
        .expect("statistics load");
        let calls = block_on(load_usage_calls(
            app_temp.path(),
            &UsageCallsQueryDto::default(),
            observed_at_epoch_ms,
        ))
        .expect("calls load");
        (
            overview_count,
            statistics.fact.value.call_count,
            calls.total_count,
            overview
                .windows
                .iter()
                .find(|window| window.window == UsageWindow::ThisWeek)
                .expect("this week overview exists")
                .fact
                .value
                .root_count,
        )
    };

    assert_eq!(read_counts(), (3, 3, 3, 2));

    let mut index = open_index(app_temp.path());
    block_on(index.set_root_enabled(&roots[1].root_id, false)).expect("second root is disabled");
    drop(index);
    assert_eq!(read_counts(), (2, 2, 2, 1));

    let mut index = open_index(app_temp.path());
    block_on(index.set_root_enabled(&roots[1].root_id, true)).expect("second root is re-enabled");
    drop(index);
    assert_eq!(read_counts(), (3, 3, 3, 2));
}
