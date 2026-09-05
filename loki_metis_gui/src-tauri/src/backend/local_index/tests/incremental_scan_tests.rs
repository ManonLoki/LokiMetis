// 本文件验证的正是 index/rollout_ingest.rs 里 `index_rollout_file` 的
// “unchanged / append / rebuild”三选一判定（详细算法说明见那个函数的
// 注释）：多数用例都遵循“扫描一次 -> 不改文件再扫一次 -> 追加内容后
// 再扫一次”的固定节奏，分别验证首次入库、重复扫描不产生冗余、以及
// 追加内容只增量处理新行这三种关键行为。
use loki_metis_core::{Confidence, CoverageState};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    append_rollout, create_root, cumulative_line, discover_registered, scan, session_line,
    token_line, token_snapshot_line, write_rollout,
};
use crate::backend::local_index::{
    CancellationToken, LocalIndex, PARSER_VERSION, ScanConfig, ScanMode, scan_discovered_roots,
};
use crate::backend::local_index::{
    file_source::{RolloutFileInspection, RolloutProbeBudget, inspect_rollout_file},
    rollout_ingest::IndexRolloutFile,
};

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 验证无变化二次扫描不新增调用，随后 append 只增加一个调用。
#[test]
fn skips_unchanged_files_and_indexes_only_appended_call() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-append.jsonl");
    write_rollout(
        &rollout,
        "session-append",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    let unchanged = scan(&mut index, &roots, &CancellationToken::new());
    append_rollout(
        &rollout,
        &token_line("2026-07-30T10:02:00Z", "call-b", 20, 5),
    );
    let appended = scan(&mut index, &roots, &CancellationToken::new());

    assert_eq!(first.calls_added, 1);
    assert_eq!(unchanged.calls_added, 0);
    assert_eq!(unchanged.unchanged_files, 1);
    assert_eq!(appended.calls_added, 1);
    assert_eq!(appended.aggregate.call_count, 2);
    assert_eq!(appended.aggregate.tokens.input_tokens, 30);
}

/// 验证显式强制重建会为未变化来源生成新 generation，且 canonical 聚合不重复。
#[test]
fn force_rebuild_reindexes_unchanged_file_without_duplicate_calls() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-force-rebuild.jsonl");
    write_rollout(
        &rollout,
        "session-force-rebuild",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    let rebuilt = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            mode: ScanMode::Quick,
            force_rebuild: true,
            started_at_epoch_ms: Some(2_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("forced rebuild succeeds");

    assert_eq!(first.aggregate.call_count, 1);
    assert_eq!(rebuilt.unchanged_files, 0);
    assert_eq!(rebuilt.rebuilt_files, 1);
    assert_eq!(rebuilt.aggregate.call_count, 1);
    assert_eq!(rebuilt.aggregate.tokens.input_tokens, 10);
    assert_eq!(rebuilt.aggregate.tokens.output_tokens, 10);
}

/// 验证强制重建在解析取消时丢弃未完成 generation，并继续提供旧 canonical 结果。
#[test]
fn cancelled_force_rebuild_preserves_the_previous_ready_generation() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-force-cancel.jsonl");
    write_rollout(
        &rollout,
        "session-force-cancel",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    let initial = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(initial.aggregate.call_count, 1);

    let canonical_rollout = roots[0].path.join("sessions/rollout-force-cancel.jsonl");
    let mut budget = RolloutProbeBudget::new(1, u64::MAX);
    let source =
        match inspect_rollout_file(&canonical_rollout, &CancellationToken::new(), &mut budget) {
            RolloutFileInspection::Matched(source) => source,
            _ => panic!("rollout fixture must pass signature validation"),
        };
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let force_config = ScanConfig {
        max_line_bytes: 1024 * 1024,
        scan_since_epoch_ms: i64::MIN,
        force_rebuild: true,
        ..ScanConfig::default()
    };
    let cancelled =
        block_on(index.index_rollout_file(&roots[0], source, false, force_config, &cancellation))
            .expect("cancelled forced rebuild exits cleanly");

    assert!(cancelled.cancelled);
    assert!(cancelled.rebuilt);
    assert_eq!(cancelled.added_calls, 0);
    let checkpoint = block_on(index.stored_source_file(&cancelled.source_id))
        .expect("old checkpoint loads")
        .expect("old checkpoint remains ready");
    assert_eq!(checkpoint.generation, 1);
    assert_eq!(
        block_on(index.aggregate())
            .expect("old aggregate remains readable")
            .call_count,
        1
    );
}

/// 验证重开索引后仍能从持久检查点过滤重放和 compact 估算，只计真实新增调用。
#[test]
fn restores_cumulative_checkpoint_across_incremental_scans() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-checkpoint.jsonl");
    let first_snapshot =
        token_snapshot_line("2026-08-14T01:00:00Z", 100, 40, 20, 120, 100, 40, 20, 120);
    write_rollout(
        &rollout,
        "session-checkpoint",
        std::slice::from_ref(&first_snapshot),
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(first.calls_added, 1);
    drop(index);

    append_rollout(&rollout, &first_snapshot);
    append_rollout(
        &rollout,
        &token_snapshot_line("2026-08-14T01:00:02Z", 0, 0, 0, 50_000, 0, 0, 0, 50_000),
    );
    append_rollout(
        &rollout,
        &token_snapshot_line("2026-08-14T01:00:03Z", 10, 4, 2, 12, 110, 44, 22, 132),
    );
    let mut reopened = open_index(app_temp.path());
    let appended = scan(&mut reopened, &roots, &CancellationToken::new());

    assert_eq!(appended.calls_added, 1);
    assert_eq!(appended.aggregate.call_count, 2);
    assert_eq!(appended.aggregate.tokens.total_tokens, 132);
}

/// 验证连续累计回退只保留会话最新值，出现单次事实后移除累计占位。
// 对应 call_store.rs 里 `insert_usage_batch` 的置信度替换规则：先写入
// 两条 Derived（推算）事件应该合并成一条最新值，一旦后面出现 Exact
// （精确）事件，之前的 Derived 占位记录必须被清除，不能继续和精确值
// 并存误导统计。
#[test]
fn keeps_only_latest_cumulative_fallback_and_prefers_exact_calls() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-fallback.jsonl");
    write_rollout(
        &rollout,
        "session-fallback",
        &[
            cumulative_line("2026-07-30T10:01:00Z", 10),
            cumulative_line("2026-07-30T10:02:00Z", 20),
        ],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let cumulative = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(cumulative.aggregate.call_count, 1);
    assert_eq!(cumulative.aggregate.tokens.input_tokens, 20);
    assert_eq!(cumulative.aggregate.confidence, Confidence::Derived);

    append_rollout(
        &rollout,
        &token_line("2026-07-30T10:03:00Z", "exact-a", 7, 1),
    );
    let exact = scan(&mut index, &roots, &CancellationToken::new());

    assert_eq!(exact.aggregate.call_count, 1);
    assert_eq!(exact.aggregate.tokens.input_tokens, 7);
    assert_eq!(exact.aggregate.confidence, Confidence::Exact);
}

/// 验证半行 checkpoint 不保存正文，并在后续补齐换行后只索引一次。
#[test]
fn resumes_trailing_partial_line_from_complete_offset() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-partial.jsonl");
    let second = token_line("2026-07-30T10:02:00Z", "call-b", 20, 5);
    let split = second.len() / 2;
    let mut initial = session_line("session-partial");
    initial.push_str(&token_line("2026-07-30T10:01:00Z", "call-a", 10, 2));
    initial.push_str(&second[..split]);
    std::fs::write(&rollout, initial).expect("partial fixture is written");
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    append_rollout(&rollout, &second[split..]);
    let second_scan = scan(&mut index, &roots, &CancellationToken::new());

    assert_eq!(first.aggregate.call_count, 1);
    assert_eq!(second_scan.calls_added, 1);
    assert_eq!(second_scan.aggregate.call_count, 2);
}

/// 验证损坏和超大行只降低覆盖质量，合法调用仍能进入索引。
#[test]
fn counts_corrupt_and_oversized_lines_without_crashing() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-quality.jsonl");
    let content = format!(
        "{}not-json\n{{\"type\":\"unknown\",\"payload\":{{}}}}\n{}\n{}",
        session_line("session-quality"),
        "x".repeat(2_000),
        token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)
    );
    std::fs::write(&rollout, content).expect("quality fixture is written");
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let mut scan_progress = Vec::new();
    let summary = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            mode: ScanMode::Quick,
            max_line_bytes: 1_024,
            started_at_epoch_ms: Some(1_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |progress| scan_progress.push(progress),
    ))
    .expect("quality fixture is scanned");

    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(summary.coverage.state, CoverageState::Partial);
    assert!(summary.coverage.warning_count >= 3);
    assert!(
        scan_progress
            .iter()
            .all(|progress| progress.current_root_id == roots[0].root_id)
    );
    assert!(!scan_progress.is_empty());
}

/// 验证文件截断替换只重建该来源，并移除旧 generation 的调用。
#[test]
fn rebuilds_replaced_file_without_retaining_old_generation() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-replaced.jsonl");
    write_rollout(
        &rollout,
        "session-old-with-longer-id",
        &[
            token_line("2026-07-30T10:01:00Z", "old-a", 100, 20),
            token_line("2026-07-30T10:02:00Z", "old-b", 200, 40),
        ],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    let initial = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(initial.aggregate.call_count, 2);

    write_rollout(
        &rollout,
        "new",
        &[token_line("2026-07-30T11:00:00Z", "new-a", 7, 1)],
    );
    let replaced = scan(&mut index, &roots, &CancellationToken::new());

    assert_eq!(replaced.rebuilt_files, 1);
    assert_eq!(replaced.aggregate.call_count, 1);
    assert_eq!(replaced.aggregate.tokens.input_tokens, 7);
}

/// 验证预取消扫描不读取 fixture，并把覆盖与 scan_runs 状态标为取消。
#[test]
fn scan_honors_preexisting_cancellation() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    write_rollout(
        &root_path.join("sessions/rollout-cancel.jsonl"),
        "session-cancel",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let summary = scan(&mut index, &roots, &cancellation);

    assert_eq!(summary.coverage.state, CoverageState::Cancelled);
    assert_eq!(summary.files_scanned, 0);
    assert_eq!(summary.aggregate.call_count, 0);
}
