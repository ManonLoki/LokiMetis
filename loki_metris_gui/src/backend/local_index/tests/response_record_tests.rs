//! 使用完全隔离的 SQLite 与 rollout 验证新旧用量格式的生命周期。

use loki_metris_core::{Confidence, CoverageState, LocalErrorKind};
use serde_json::json;
use std::path::Path;
use tempfile::TempDir;

use super::support::{
    append_rollout, create_root, cumulative_line, discover_registered, execute_fixture_sql,
    reopen_for_fixture, scan, token_line, write_rollout,
};
use crate::backend::local_index::test_runtime::block_on;
use crate::backend::local_index::{
    CancellationToken, DiscoveredRoot, LocalIndex, PARSER_VERSION, ScanConfig,
    file_source::{
        RolloutFileInspection, RolloutProbeBudget, ValidatedRolloutFile, inspect_rollout_file,
    },
    rollout_ingest::IndexRolloutFile,
    scan_discovered_roots,
};

/// 创建独立数据库，禁止使用真实 app-data。
fn open_index(path: &Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(path, PARSER_VERSION)).expect("isolated index opens")
}

/// 当前协议的单响应事实；累计值故意不同，不能被当作额外调用。
fn response(timestamp: &str, id: &str, input: u64) -> String {
    format!(
        "{}\n",
        json!({"type":"token_usage_record","timestamp":timestamp,
        "payload":{"thread_id":"owner","session_id":"root","response_id":id,
            "usage":{"input_tokens":input,"cached_input_tokens":0,"cache_write_input_tokens":0,
                "output_tokens":10,"reasoning_output_tokens":0,"total_tokens":input+10},
            "thread_token_usage":{"input_tokens":999999,"total_tokens":999999}}})
    )
}

/// 为直接摄入测试取得真实签名验证句柄；检查时不传已经取消的令牌。
fn validated(root: &DiscoveredRoot) -> ValidatedRolloutFile {
    match inspect_rollout_file(
        &root.path.join("sessions/rollout-response.jsonl"),
        &CancellationToken::new(),
        &mut RolloutProbeBudget::new(1, u64::MAX),
    ) {
        RolloutFileInspection::Matched(file) => file,
        _ => panic!("synthetic rollout signature matches"),
    }
}

/// 读取已公开的规范调用总数、总量，避免以内部实现状态代替实际账单断言。
fn totals(index: &mut LocalIndex) -> (usize, u64) {
    let calls = block_on(index.canonical_calls())
        .expect("canonical calls load")
        .calls;
    (
        calls.len(),
        calls.iter().map(|c| c.usage.total_tokens).sum(),
    )
}

/// fork 复制前缀的现代记录明确属于父线程；可选旧记录则缺少归属证据。
fn fork_prefix(ambiguous_legacy: bool) -> String {
    let owner = json!({"timestamp":"2026-07-30T10:00:00Z","type":"session_meta",
        "payload":{"id":"owner","session_id":"owner","forked_from_id":"parent"}});
    let mut foreign: serde_json::Value = serde_json::from_str(
        &response("2026-07-30T09:00:00Z", "parent-response", 500),
    ).unwrap();
    foreign["payload"]["thread_id"] = json!("parent");
    let legacy = if ambiguous_legacy {
        token_line("2026-07-30T10:00:10Z", "unowned-legacy", 50, 0)
    } else { String::new() };
    format!("{owner}\n{foreign}\n{legacy}")
}

/// 空 fork 追加明确本线程响应后成为完整来源，重启与重建不重复计入父前缀。
#[test]
fn modern_fork_identity_survives_append_restart_and_rebuild() {
    check_fork_lifecycle(false);
}

/// 跨重启后出现明确现代响应，仍不能把先前不确定旧用量的质量抹掉。
#[test]
fn modern_fork_keeps_legacy_ambiguity_through_restart_and_rebuild() {
    check_fork_lifecycle(true);
}

/// 使用真实 SQLite 检查空前缀和歧义前缀的实际账单及持久质量。
fn check_fork_lifecycle(ambiguous_legacy: bool) {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    std::fs::write(&path, fork_prefix(ambiguous_legacy)).unwrap();
    let roots = discover_registered(&[root]);
    let expected_state = if ambiguous_legacy { CoverageState::Partial } else { CoverageState::Complete };
    let warnings = u64::from(ambiguous_legacy);
    let mut index = open_index(app.path());
    let prefix = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!((prefix.call_count, prefix.coverage.state, prefix.coverage.warning_count),
        (0, expected_state, warnings));
    drop(index);
    let mut index = open_index(app.path());
    let own_response = response("2026-07-30T10:00:20Z", "own-response", 20);
    let split = own_response.len() / 2;
    append_rollout(&path, &own_response[..split]);
    let half_line = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!((half_line.call_count, half_line.coverage.warning_count), (0, warnings));
    drop(index);
    let mut index = open_index(app.path());
    append_rollout(&path, &own_response[split..]);
    append_rollout(&path, &token_line("2026-07-30T10:00:21Z", "shadow", 999, 0));
    let appended = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!((appended.call_count, appended.coverage.state, appended.coverage.warning_count),
        (1, expected_state, warnings));
    assert_eq!(totals(&mut index), (1, 30));
    drop(index);
    let mut index = open_index(app.path());
    let unchanged = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(unchanged.coverage.warning_count, warnings);
    let rebuilt = block_on(scan_discovered_roots(
        &mut index, &roots,
        ScanConfig { force_rebuild: true, ..ScanConfig::default() },
        &CancellationToken::new(), |_| {},
    )).unwrap();
    assert_eq!((rebuilt.call_count, rebuilt.coverage.state, rebuilt.coverage.warning_count),
        (1, expected_state, warnings));
    assert_eq!(totals(&mut index), (1, 30));
}

/// unchanged、重启、追加重放和强制重建得到同一账单；旧影子始终不新增调用。
#[test]
fn response_usage_is_stable_through_restart_append_replay_and_rebuild() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    let first = response("2026-07-30T10:01:00Z", "first", 10);
    write_rollout(&path, "owner", std::slice::from_ref(&first));
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    let scanned = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(
        (
            scanned.call_count,
            scanned.coverage.state,
            scanned.coverage.warning_count
        ),
        (1, CoverageState::Complete, 0)
    );
    assert_eq!(
        scan(&mut index, &roots, &CancellationToken::new()).unchanged_files,
        1
    );
    drop(index);
    let mut index = open_index(app.path());
    append_rollout(&path, &token_line("2026-07-30T10:02:00Z", "shadow", 999, 0));
    append_rollout(&path, &first);
    append_rollout(&path, &response("2026-07-30T10:03:00Z", "second", 20));
    let appended = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(
        (
            appended.calls_added,
            appended.call_count,
            appended.coverage.warning_count
        ),
        (1, 2, 0)
    );
    assert_eq!(totals(&mut index), (2, 50));
    let rebuilt = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            force_rebuild: true,
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(
        (
            rebuilt.rebuilt_files,
            rebuilt.coverage.state,
            rebuilt.coverage.warning_count
        ),
        (1, CoverageState::Complete, 0)
    );
    assert_eq!(totals(&mut index), (2, 50));
}

/// 旧累计回退和新响应确属不同调用，严格写入不能沿用旧 Exact 替换 Derived 的策略。
#[test]
fn modern_response_keeps_derived_legacy_prefix_in_real_sqlite() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    write_rollout(
        &root.join("sessions/rollout-response.jsonl"),
        "owner",
        &[
            cumulative_line("2026-07-30T10:01:00Z", 10),
            response("2026-07-30T10:02:00Z", "modern", 20),
            token_line("2026-07-30T10:03:00Z", "shadow", 100, 0),
        ],
    );
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    let result = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(
        (
            result.call_count,
            result.coverage.state,
            result.coverage.warning_count
        ),
        (2, CoverageState::Partial, 1)
    );
    assert_eq!(totals(&mut index), (2, 50));
    let calls = block_on(index.canonical_calls()).unwrap().calls;
    assert!(
        calls
            .iter()
            .any(|c| c.confidence == Confidence::Derived && c.usage.total_tokens == 20)
    );
    assert!(
        calls
            .iter()
            .any(|c| c.confidence == Confidence::Exact && c.usage.total_tokens == 30)
    );
}

/// 相同身份的矛盾追加在重启后仍失败，不能覆盖旧账单或错误恢复完整覆盖。
#[test]
fn conflicting_response_append_survives_restart_without_overwriting() {
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
    append_rollout(&path, &response("2026-07-30T10:02:00Z", "same", 20));
    let error = block_on(index.index_rollout_file(
        &roots[0],
        validated(&roots[0]),
        false,
        ScanConfig::default(),
        &CancellationToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.kind(), LocalErrorKind::InvalidUsage);
    for _ in 0..2 {
        let result = scan(&mut index, &roots, &CancellationToken::new());
        assert_eq!(
            (result.coverage.state, result.coverage.warning_count),
            (CoverageState::Partial, 1)
        );
        assert_eq!(totals(&mut index), (1, 20));
        assert!(!block_on(index.usage_snapshot()).unwrap().read_ready);
        drop(index);
        index = open_index(app.path());
    }
}

/// 重建也不能静默改写同一响应账单；旧已公开 generation 仍是权威事实。
#[test]
fn conflicting_response_rebuild_preserves_published_generation() {
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
        &[response("2026-07-30T10:01:00Z", "same", 20)],
    );
    let result = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            force_rebuild: true,
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(
        (result.coverage.state, result.coverage.warning_count),
        (CoverageState::Partial, 1)
    );
    assert_eq!(totals(&mut index), (1, 20));
    assert!(!block_on(index.usage_snapshot()).unwrap().read_ready);
}

/// 时间保留过滤不能丢掉协议阶段，重启后的新旧影子仍服从已观察的现代阶段。
#[test]
fn filtered_first_response_still_persists_authority() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    write_rollout(
        &path,
        "owner",
        &[response("2020-01-01T00:00:00Z", "old-modern", 10)],
    );
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    let config = ScanConfig {
        ingest_since_epoch_ms: 1_700_000_000_000,
        ..ScanConfig::default()
    };
    block_on(scan_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(totals(&mut index), (0, 0));
    drop(index);
    let mut index = open_index(app.path());
    append_rollout(&path, &token_line("2026-07-30T10:02:00Z", "shadow", 999, 0));
    append_rollout(&path, &response("2026-07-30T10:03:00Z", "new-modern", 20));
    let result = block_on(scan_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .unwrap();
    assert_eq!(result.coverage.warning_count, 0);
    assert_eq!(totals(&mut index), (1, 30));
}

/// 首条现代追加被取消后重新读取；取消重建则保留已经公开的现代检查点。
#[test]
fn cancelled_append_and_rebuild_restore_the_correct_authority() {
    let source = TempDir::new().unwrap();
    let app = TempDir::new().unwrap();
    let root = create_root(source.path(), "root");
    let path = root.join("sessions/rollout-response.jsonl");
    write_rollout(
        &path,
        "owner",
        &[token_line("2026-07-30T10:01:00Z", "legacy", 10, 0)],
    );
    let roots = discover_registered(&[root]);
    let mut index = open_index(app.path());
    scan(&mut index, &roots, &CancellationToken::new());
    append_rollout(&path, &response("2026-07-30T10:02:00Z", "first-modern", 20));
    append_rollout(&path, &token_line("2026-07-30T10:03:00Z", "shadow", 999, 0));
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let outcome = block_on(index.index_rollout_file(
        &roots[0],
        validated(&roots[0]),
        false,
        ScanConfig::default(),
        &cancelled,
    ))
    .unwrap();
    assert!(outcome.cancelled && !outcome.rebuilt);
    drop(index);
    let mut index = open_index(app.path());
    let resumed = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(
        (resumed.calls_added, resumed.coverage.warning_count),
        (1, 0)
    );
    assert_eq!(totals(&mut index), (2, 50));
    let before = block_on(index.load_parse_context(&outcome.source_id))
        .unwrap()
        .unwrap();
    let outcome = block_on(index.index_rollout_file(
        &roots[0],
        validated(&roots[0]),
        false,
        ScanConfig {
            force_rebuild: true,
            ..ScanConfig::default()
        },
        &cancelled,
    ))
    .unwrap();
    assert!(outcome.cancelled && outcome.rebuilt);
    let after = block_on(index.load_parse_context(&outcome.source_id))
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(totals(&mut index), (2, 50));
    append_rollout(
        &path,
        &token_line("2026-07-30T10:04:00Z", "late-shadow", 1234, 0),
    );
    assert_eq!(
        scan(&mut index, &roots, &CancellationToken::new()).calls_added,
        0
    );
    assert_eq!(totals(&mut index), (2, 50));
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - response_record_tests_rebuild.rs：旧 parser/schema 与旧警告只能通过重读原始文件解除，不能靠缺字段补零。；重建先按首次发生时间决定保留窗口，重放时间变化既不能丢账也不能延长保留期。
include!("response_record_tests_rebuild.rs");
