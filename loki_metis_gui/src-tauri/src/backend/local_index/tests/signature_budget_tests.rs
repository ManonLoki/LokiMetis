use std::fs;

use loki_metis_core::{CoverageState, LocalIndexState};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    append_rollout, create_root, discover_registered, scan, session_line, token_line, write_rollout,
};
use crate::backend::local_index::{
    CancellationToken, DiscoveryInputs, LocalIndex, PARSER_VERSION, ScanConfig, discover_quick,
    scan_discovered_roots,
};

// 本文件名为 signature_budget，覆盖两类边界：一类是 file_source.rs 里
// “首记录签名探测”本身的正确性（只有真正以 session_meta 开头的文件才
// 会被当作 rollout 数据源），另一类是探测预算耗尽/文件在探测中途消失
// 等资源受限场景下的保守回退。

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 验证有效根内只有通过首记录签名的同名文件会进入来源与调用索引。
#[test]
fn indexes_only_signature_matched_rollout_files_inside_a_valid_root() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let valid_rollout = root_path.join("sessions/rollout-valid.jsonl");
    let ordinary_rollout = root_path.join("sessions/rollout-ordinary.jsonl");
    write_rollout(
        &valid_rollout,
        "session-valid",
        &[token_line("2026-07-30T10:01:00Z", "call-valid", 10, 2)],
    );
    // “ordinary” 这份文件虽然文件名同样满足 `rollout-*.jsonl` 命名约定，
    // 但故意只写了一行 token_line、没有以 session_meta 开头——它伪装成
    // 一个“看起来像”rollout 的普通 JSONL，用来验证仅凭文件名无法蒙混
    // 过关，必须首记录结构签名也匹配才会被采信为数据源。
    let ordinary_bytes =
        token_line("2026-07-30T10:02:00Z", "call-ordinary", 9_999, 8_888).into_bytes();
    fs::write(&ordinary_rollout, &ordinary_bytes).expect("ordinary JSONL bait is written");
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let summary = scan(&mut index, &roots, &CancellationToken::new());
    let sources = block_on(index.list_sources()).expect("source roots load");
    let root_record = &sources[0];

    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(root_record.source_file_count, 1);
    assert_eq!(root_record.call_observation_count, 1);
    assert_eq!(summary.coverage.state, CoverageState::Partial);
    assert_eq!(
        fs::read(ordinary_rollout).expect("ordinary bait remains readable"),
        ordinary_bytes
    );
}

/// 验证已索引来源确认失去 Codex 首记录签名后只清理派生来源与调用。
#[test]
fn confirmed_rejected_rollout_removes_only_its_derived_records() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let anchor = root_path.join("sessions/rollout-anchor.jsonl");
    let replaced = root_path.join("sessions/rollout-replaced.jsonl");
    write_rollout(
        &anchor,
        "session-anchor",
        &[token_line("2026-07-30T10:01:00Z", "call-anchor", 10, 2)],
    );
    write_rollout(
        &replaced,
        "session-replaced",
        &[token_line("2026-07-30T10:02:00Z", "call-replaced", 20, 4)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let mut index = open_index(app_temp.path());
    assert_eq!(
        scan(&mut index, &roots, &CancellationToken::new())
            .aggregate
            .call_count,
        2
    );

    let rejected_bytes =
        token_line("2026-07-30T10:03:00Z", "call-rejected", 7_777, 6_666).into_bytes();
    fs::write(&replaced, &rejected_bytes).expect("source is replaced by ordinary JSONL");
    let rediscovered = discover_registered(&[root_path]);
    let summary = scan(&mut index, &rediscovered, &CancellationToken::new());
    let sources = block_on(index.list_sources()).expect("source roots load");
    let root_record = &sources[0];

    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(root_record.source_file_count, 1);
    assert_eq!(root_record.call_observation_count, 1);
    assert_eq!(
        fs::read(replaced).expect("rejected source remains readable"),
        rejected_bytes
    );
}

/// 验证签名预算不足时保留既有 checkpoint 与调用，并把覆盖降为部分。
#[test]
fn signature_budget_exhaustion_preserves_existing_index_records() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-budget.jsonl");
    write_rollout(
        &rollout,
        "session-budget",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    append_rollout(
        &rollout,
        &token_line("2026-07-30T10:02:00Z", "call-b", 20, 4),
    );

    let summary = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            max_signature_files: 0,
            started_at_epoch_ms: Some(2_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("budget-limited scan returns a partial result");

    assert_eq!(summary.coverage.state, CoverageState::Partial);
    assert_eq!(summary.coverage.roots_scanned, 0);
    assert_eq!(summary.files_scanned, 0);
    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(
        block_on(index.list_sources()).expect("source roots load")[0].source_file_count,
        1
    );
}

/// 验证首条记录尚未写完时属于不确定状态，不会移除历史根或 checkpoint。
#[test]
fn incomplete_signature_line_preserves_existing_root_and_source() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-incomplete.jsonl");
    write_rollout(
        &rollout,
        "session-complete",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let root_id = roots[0].root_id.clone();
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());

    let incomplete_bytes = session_line("session-being-written")
        .trim_end_matches('\n')
        .as_bytes()
        .to_vec();
    fs::write(&rollout, &incomplete_bytes).expect("partial first line is written");
    let validation = discover_quick(
        &DiscoveryInputs {
            home_dir: None,
            codex_home: None,
            registered_roots: block_on(index.all_roots()).expect("remembered roots load"),
        },
        &CancellationToken::new(),
    );

    assert!(validation.roots.is_empty());
    assert!(validation.confirmed_invalid_root_ids.is_empty());
    assert_eq!(validation.unconfirmed_root_ids, [root_id]);

    let summary = scan(&mut index, &roots, &CancellationToken::new());
    let sources = block_on(index.list_sources()).expect("source roots load");
    let root_record = &sources[0];
    assert_eq!(summary.coverage.state, CoverageState::Partial);
    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(root_record.source_file_count, 1);
    assert_eq!(root_record.call_observation_count, 1);
    assert_eq!(
        fs::read(rollout).expect("partial source remains readable"),
        incomplete_bytes
    );
}

/// 验证首次索引的目录项预算耗尽不会把零来源误报为已确认零调用。
#[test]
fn entry_budget_exhaustion_keeps_fresh_scope_not_scanned() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    write_rollout(
        &root_path.join("sessions/rollout-entry-budget.jsonl"),
        "session-budget",
        &[],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let summary = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            max_entries: 0,
            started_at_epoch_ms: Some(3_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("entry-budget scan returns a partial result");

    assert_eq!(summary.coverage.state, CoverageState::Partial);
    assert_eq!(summary.coverage.roots_scanned, 0);
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("partial snapshot loads")
            .index_state,
        LocalIndexState::NotScanned
    );
}

/// 验证目录项数量恰好等于预算时仍属于完整枚举，不会产生多余部分覆盖。
#[test]
fn exact_entry_budget_completes_scan() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    write_rollout(
        &root_path.join("sessions/rollout-exact-entry-budget.jsonl"),
        "session-budget",
        &[],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());

    let summary = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            max_entries: 1,
            started_at_epoch_ms: Some(4_000),
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("exact entry-budget scan completes");

    assert_eq!(summary.coverage.state, CoverageState::Complete);
    assert_eq!(summary.coverage.roots_scanned, 1);
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("completed snapshot loads")
            .index_state,
        LocalIndexState::ReadyNoCalls
    );
}

/// 验证显式重验确认历史根失效后按精确 ID 级联移除本产品记录，外部文件不变。
#[test]
fn explicit_revalidation_removes_confirmed_invalid_root_by_exact_id() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-invalid-root.jsonl");
    write_rollout(
        &rollout,
        "session-invalid-root",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(std::slice::from_ref(&root_path));
    let root_id = roots[0].root_id.clone();
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    let invalid_bytes = b"{\"type\":\"ordinary\"}\n".to_vec();
    fs::write(&rollout, &invalid_bytes).expect("root signature is deliberately invalidated");

    let validation = discover_quick(
        &DiscoveryInputs {
            home_dir: None,
            codex_home: None,
            registered_roots: block_on(index.all_roots()).expect("remembered roots load"),
        },
        &CancellationToken::new(),
    );
    assert_eq!(validation.confirmed_invalid_root_ids, [root_id]);
    assert_eq!(
        block_on(index.remove_roots(&validation.confirmed_invalid_root_ids))
            .expect("confirmed invalid root is removed"),
        1
    );

    assert!(
        block_on(index.list_sources())
            .expect("source roots load")
            .is_empty()
    );
    assert_eq!(
        block_on(index.aggregate())
            .expect("aggregate reloads")
            .call_count,
        0
    );
    assert_eq!(
        block_on(index.usage_snapshot())
            .expect("snapshot reloads")
            .index_state,
        LocalIndexState::NotScanned
    );
    assert_eq!(
        fs::read(rollout).expect("invalidated source remains readable"),
        invalid_bytes
    );
}
