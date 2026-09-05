use std::fs::{self, OpenOptions};
use std::io::Write;

use tauri::async_runtime::block_on;
use tempfile::tempdir;

use loki_metis_core::SourceClientKind;

use super::*;
use crate::backend::local_index::claude::discovery::{
    ClaudeDiscoveryInputs, discover_claude_quick,
};
use crate::backend::local_index::claude::index::IndexClaudeTranscript;
use crate::backend::local_index::{
    CLAUDE_PARSER_VERSION, PARSER_VERSION, RegisteredRoot, ScanMode,
};

/// writer 写入的 generation 必须等于 core 打开索引、概览和 Collect 使用的值。
#[test]
fn claude_writer_generation_matches_core_reader() {
    assert_eq!(
        CLAUDE_PARSER_VERSION,
        SourceClientKind::ClaudeCode.parser_version()
    );
}

/// 在隔离 app-data 内按指定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path, parser_version: u32) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, parser_version)).expect("index opens")
}

// 本文件是 root_lifecycle/incremental_scan 等 Codex 测试组针对 Claude
// Code 客户端的对应版本，额外覆盖了 Claude 专属的“单调 upsert 拒绝
// 数值倒退”规则（call_store.rs 的 insert_claude_usage_batch）。
/// 全部夹具共用的固定合法 session UUID。
const SESSION: &str = "00000000-0000-0000-0000-000000000001";

/// 验证增量扫描只处理新增内容，且拒绝追加写入导致的用量数值倒退。
#[test]
fn scan_is_incremental_and_rejects_appended_usage_regression() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-a", 10, 20, 30, 1, false);
    let roots = discover_roots(root_path);
    // app-data 路径显式带上 `clients/claude-code` 子目录，并用
    // CLAUDE_PARSER_VERSION（而不是 Codex 的 PARSER_VERSION）打开索引——
    // 对应 agent_client.rs 里 client_app_data_dir 的物理隔离规则：
    // 两个客户端的 SQLite 文件和 parser generation 编号空间完全独立，
    // 不会互相覆盖或混淆。
    let app_data = temp.path().join("app-data/clients/claude-code");
    let mut index = open_index(&app_data, CLAUDE_PARSER_VERSION);
    let config = ScanConfig {
        mode: ScanMode::Quick,
        started_at_epoch_ms: Some(1_000),
        ..ScanConfig::default()
    };

    let mut first_progress = Vec::new();
    let first = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |progress| first_progress.push(progress),
    ))
    .expect("first scan succeeds");
    assert_eq!(first.files_scanned, 1);
    assert_eq!(first.calls_added, 1, "first summary: {first:?}");
    assert_eq!(first.aggregate.call_count, 1);
    assert_eq!(first.aggregate.tokens.input_tokens, 60);
    assert_eq!(first.aggregate.tokens.output_tokens, 1);
    assert!(
        first_progress
            .iter()
            .all(|progress| progress.current_root_id == roots[0].root_id)
    );
    assert!(!first_progress.is_empty());

    let unchanged = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("unchanged scan succeeds");
    assert_eq!(unchanged.unchanged_files, 1);
    assert_eq!(unchanged.calls_added, 0);

    write_observation(&transcript, "msg-a", 10, 20, 30, 3, true);
    let appended = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("append scan succeeds");
    assert_eq!(appended.calls_added, 0);
    assert_eq!(appended.aggregate.tokens.output_tokens, 3);

    write_observation(&transcript, "msg-a", 9, 20, 31, 4, true);
    let raw_input_regressed = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("raw input regression scan finishes with warning");
    assert!(raw_input_regressed.coverage.warning_count >= 1);
    assert_eq!(raw_input_regressed.aggregate.tokens.output_tokens, 3);

    write_observation(&transcript, "msg-a", 10, 20, 30, 2, true);
    let regressed = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("regression scan finishes with warning");
    assert!(regressed.coverage.warning_count >= 1);
    assert_eq!(regressed.aggregate.tokens.output_tokens, 3);
}

/// 验证替换整份 transcript 会重建 generation，且不保留旧的调用记录。
#[test]
fn replacement_rebuilds_generation_without_retaining_old_calls() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-old", 1, 0, 0, 1, false);
    let roots = discover_roots(root_path);
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    let config = ScanConfig::default();
    block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("first scan succeeds");

    write_observation(&transcript, "msg-new", 8, 1, 1, 2, false);
    let rebuilt = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        config,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("replacement scan succeeds");
    assert_eq!(rebuilt.rebuilt_files, 1);
    assert_eq!(rebuilt.aggregate.call_count, 1);
    assert_eq!(rebuilt.aggregate.tokens.input_tokens, 10);
    assert_eq!(rebuilt.aggregate.tokens.output_tokens, 2);
}

/// 已索引 transcript 在扫描窗口外被追加时必须重新解析，追加的调用不得因窗口而丢失；
/// 追加内容入库后，再次以同一窗口扫描时该文件不再被打开。
#[test]
fn appended_transcript_outside_the_scan_window_is_still_indexed() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-a", 10, 0, 0, 1, false);
    let roots = discover_roots(root_path);
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("first scan succeeds");

    write_observation(&transcript, "msg-b", 5, 0, 0, 2, true);
    let stale_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    fs::File::options()
        .write(true)
        .open(&transcript)
        .expect("transcript reopens")
        .set_modified(stale_mtime)
        .expect("transcript mtime is backdated");
    let windowed = ScanConfig {
        scan_since_epoch_ms: 3_000_000_000_000,
        ..ScanConfig::default()
    };

    let appended = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        windowed,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("windowed scan succeeds");
    assert_eq!(appended.files_scanned, 1, "changed source must be re-read");
    assert_eq!(appended.calls_added, 1);
    assert_eq!(appended.aggregate.call_count, 2);
    assert_eq!(appended.aggregate.tokens.output_tokens, 3);

    let settled = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        windowed,
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("settled scan succeeds");
    assert_eq!(
        settled.files_scanned, 0,
        "unchanged source outside the window stays closed"
    );
    assert_eq!(
        settled.aggregate.call_count, 2,
        "retained source keeps its calls"
    );
}

/// 摄入下界只丢弃保留窗口之外的调用；解析后的来源仍保持就绪。
#[test]
fn transcript_index_filters_calls_older_than_the_scan_window() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-old", 3, 2, 1, 4, false);
    let root = discover_roots(root_path)
        .into_iter()
        .next()
        .expect("Claude root is discovered");
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    block_on(index.register_claude_root(&root)).expect("root registers");

    let result = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        2_000_000_000_000,
        false,
        &CancellationToken::new(),
    ))
    .expect("windowed transcript scan succeeds");

    assert_eq!(result.added_calls, 0);
    assert_eq!(
        block_on(index.aggregate())
            .expect("aggregate loads")
            .call_count,
        0
    );
    assert!(
        block_on(index.stored_source_file(&result.source_id))
            .expect("source state loads")
            .is_some(),
        "the parsed source remains ready even when all calls are outside the window"
    );
}

/// 验证 Claude 扫描只写入 Claude 专属数据库，不会碰到 Codex 数据库。
#[test]
fn claude_scan_cannot_write_into_the_codex_database() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-claude", 3, 2, 1, 4, false);
    let roots = discover_roots(root_path);
    let app_data = temp.path().join("app-data");
    let codex_index = open_index(&app_data, PARSER_VERSION);
    let codex_database_path = codex_index.database_path().to_path_buf();
    assert_eq!(
        block_on(codex_index.aggregate())
            .expect("Codex aggregate loads")
            .call_count,
        0
    );

    let claude_app_data = app_data.join("clients/claude-code");
    let mut claude_index = open_index(&claude_app_data, CLAUDE_PARSER_VERSION);
    let claude_database_path = claude_index.database_path().to_path_buf();
    let summary = block_on(scan_claude_discovered_roots(
        &mut claude_index,
        &roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("Claude scan succeeds");

    assert_ne!(PARSER_VERSION, CLAUDE_PARSER_VERSION);
    assert_ne!(codex_database_path, claude_database_path);
    assert_eq!(summary.aggregate.call_count, 1);
    assert_eq!(
        block_on(codex_index.aggregate())
            .expect("Codex aggregate reloads")
            .call_count,
        0
    );
}

/// 验证主 session 与每个官方 subagent 各自被识别为独立线程。
#[test]
fn main_session_and_each_official_subagent_are_distinct_threads() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let main = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    let subagents = root_path.join(format!("projects/project-a/{SESSION}/subagents"));
    write_observation(&main, "msg-shared", 1, 0, 0, 1, false);
    write_observation(
        &subagents.join("agent-one.jsonl"),
        "msg-shared",
        2,
        0,
        0,
        1,
        false,
    );
    write_observation(
        &subagents.join("agent-two.jsonl"),
        "msg-shared",
        3,
        0,
        0,
        1,
        false,
    );
    write_observation(
        &subagents.join("unapproved.jsonl"),
        "msg-bait",
        99,
        0,
        0,
        1,
        false,
    );

    let roots = discover_roots(root_path);
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    let summary = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("three approved transcripts scan");

    assert_eq!(summary.files_scanned, 3);
    assert_eq!(summary.aggregate.call_count, 3);
    assert_eq!(summary.aggregate.thread_count, 3);
    assert_eq!(summary.aggregate.tokens.input_tokens, 6);
}

/// 验证 subagent 枚举未完整完成时，保留上一次已就绪的 generation。
#[test]
fn incomplete_subagent_enumeration_preserves_the_previous_ready_generation() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let main = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    let subagent = root_path.join(format!(
        "projects/project-a/{SESSION}/subagents/agent-one.jsonl"
    ));
    write_observation(&main, "msg-main", 1, 0, 0, 1, false);
    write_observation(&subagent, "msg-subagent", 2, 0, 0, 1, false);
    let roots = discover_roots(root_path);
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);

    let first = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("initial complete scan succeeds");
    assert_eq!(first.aggregate.call_count, 2);

    let partial = block_on(scan_claude_discovered_roots(
        &mut index,
        &roots,
        ScanConfig {
            max_directories: 2,
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    ))
    .expect("bounded rescan remains usable");

    assert_eq!(partial.coverage.state, CoverageState::Partial);
    assert_eq!(partial.aggregate.call_count, 2);
    assert_eq!(
        block_on(index.aggregate())
            .expect("old subagent remains")
            .call_count,
        2
    );
}

/// 验证只有瞬时错误类别才会被判定为“应保留旧来源”。
#[test]
fn only_transient_source_errors_preserve_previous_claude_sources() {
    assert!(is_transient_source_error(LocalErrorKind::PermissionDenied));
    assert!(is_transient_source_error(LocalErrorKind::SourceUnavailable));
    assert!(!is_transient_source_error(LocalErrorKind::InvalidPath));
    assert!(!is_transient_source_error(LocalErrorKind::InvalidUsage));
}

/// 验证扫描被取消后保存的检查点，下次能从断点续扫而不是被判定为无变化。
#[test]
fn cancelled_claude_checkpoint_resumes_instead_of_becoming_unchanged() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-first", 1, 0, 0, 1, false);
    let root = discover_roots(root_path)
        .into_iter()
        .next()
        .expect("Claude root is discovered");
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    block_on(index.register_claude_root(&root)).expect("root registers");

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let first = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &cancelled,
    ))
    .expect("pre-cancelled first scan keeps a resumable checkpoint");
    assert!(first.cancelled);
    assert!(!first.unchanged);
    let first_checkpoint = block_on(index.stored_source_file(&first.source_id))
        .expect("checkpoint loads")
        .expect("checkpoint exists");
    assert_eq!(first_checkpoint.observed_size, 0);

    let resumed = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &CancellationToken::new(),
    ))
    .expect("first scan resumes");
    assert!(!resumed.unchanged);
    assert_eq!(
        block_on(index.aggregate())
            .expect("first call is indexed")
            .call_count,
        1
    );

    write_observation(&transcript, "msg-second", 2, 0, 0, 1, true);
    let append_cancelled = CancellationToken::new();
    append_cancelled.cancel();
    let partial_append = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &append_cancelled,
    ))
    .expect("cancelled append preserves its prior ready generation");
    assert!(partial_append.cancelled);
    assert_eq!(
        block_on(index.aggregate())
            .expect("old call remains")
            .call_count,
        1
    );

    let append_resumed = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &CancellationToken::new(),
    ))
    .expect("append resumes");
    assert!(!append_resumed.unchanged);
    assert_eq!(
        block_on(index.aggregate())
            .expect("both calls are indexed")
            .call_count,
        2
    );
}

/// 测试辅助：把单个路径作为已登记根跑一次快速发现，返回确认结果。
fn discover_roots(path: std::path::PathBuf) -> Vec<ClaudeDiscoveredRoot> {
    discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: None,
                path,
                alias: "Claude 测试根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    )
    .roots
}

/// 测试辅助：写入一条（或追加一条）合成 assistant 用量观察记录。
fn write_observation(
    path: &std::path::Path,
    message_id: &str,
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    append: bool,
) {
    fs::create_dir_all(path.parent().expect("transcript has parent"))
        .expect("transcript parent is created");
    let line = format!(
        "{}\n",
        serde_json::json!({
            "type": "assistant",
            "sessionId": SESSION,
            "timestamp": "2026-07-31T08:00:00Z",
            "message": {
                "id": message_id,
                "model": "claude-test",
                "content": "privacy bait",
                "usage": {
                    "input_tokens": input,
                    "cache_read_input_tokens": cached,
                    "cache_creation_input_tokens": cache_write,
                    "output_tokens": output,
                }
            }
        })
    );
    if append {
        OpenOptions::new()
            .append(true)
            .open(path)
            .expect("transcript opens for append")
            .write_all(line.as_bytes())
            .expect("observation appends");
    } else {
        fs::write(path, line).expect("transcript is written");
    }
}
