//! 通过隔离 JSONL 与真实 SQLite 闭环验证 Codex fork 归属段入库语义。

use std::fs;

use loki_metis_core::{ProviderKind, SourceClientKind};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    append_rollout, codex_aggregate, create_root, discover_registered, scan, token_line,
    write_rollout,
};
use crate::backend::local_index::{CancellationToken, LocalIndex};

/// 在隔离 app-data 中按当前 Codex parser generation 打开索引。
fn open_index(app_data: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(
        app_data,
        SourceClientKind::Codex.parser_version(),
    ))
    .expect("isolated index opens")
}

/// 构造尚未出现自身边界的 fork 前缀，其中 500 Token 只属于父级复制历史。
fn fork_prefix() -> &'static str {
    concat!(
        r#"{"timestamp":"2026-08-12T08:00:00Z","type":"session_meta","payload":{"id":"fork-owner","agent_path":["parent","child"]}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:00.100Z","type":"session_meta","payload":{"id":"copied-parent"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:00.200Z","type":"event_msg","payload":{"type":"task_started","started_at":"2026-08-12T07:00:00Z"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T08:00:00.300Z","type":"event_msg","payload":{"type":"token_count","call_id":"copied-prefix-call","info":{"last_token_usage":{"input_tokens":400,"cached_input_tokens":300,"output_tokens":100,"reasoning_output_tokens":20,"total_tokens":500},"total_token_usage":{"input_tokens":400,"cached_input_tokens":300,"output_tokens":100,"reasoning_output_tokens":20,"total_tokens":500}}}}"#,
        "\n"
    )
}

/// 构造完整 fork 文件：复制前缀必须排除，边界后的 20 Token 属于自身。
fn fork_rollout() -> String {
    format!(
        "{}{}",
        fork_prefix(),
        concat!(
            r#"{"timestamp":"2026-08-12T08:00:01Z","type":"event_msg","payload":{"type":"task_started","started_at":"2026-08-12T08:00:00.500Z"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-12T08:00:02Z","type":"event_msg","payload":{"type":"token_count","call_id":"fork-owned-call-1","info":{"last_token_usage":{"input_tokens":12,"cached_input_tokens":4,"output_tokens":8,"reasoning_output_tokens":3,"total_tokens":20},"total_token_usage":{"input_tokens":412,"cached_input_tokens":304,"output_tokens":108,"reasoning_output_tokens":23,"total_tokens":520}}}}"#,
            "\n"
        )
    )
}

/// 构造 fork 已进入自身区段后的增量追加调用。
fn fork_append() -> &'static str {
    concat!(
        r#"{"timestamp":"2026-08-12T08:00:03Z","type":"event_msg","payload":{"type":"token_count","call_id":"fork-owned-call-2","info":{"last_token_usage":{"input_tokens":6,"cached_input_tokens":2,"output_tokens":4,"reasoning_output_tokens":1,"total_tokens":10},"total_token_usage":{"input_tokens":418,"cached_input_tokens":306,"output_tokens":112,"reasoning_output_tokens":24,"total_tokens":530}}}}"#,
        "\n"
    )
}

/// 首扫与重开续扫都必须只把根调用和 fork 自身调用写入当前 generation。
#[test]
fn sqlite_index_excludes_fork_prefix_and_restores_owned_checkpoint() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root = create_root(source_temp.path(), "root");
    let root_rollout = root.join("sessions/rollout-root.jsonl");
    let fork_path = root.join("sessions/rollout-fork.jsonl");
    write_rollout(
        &root_rollout,
        "root-owner",
        &[token_line("2026-08-12T07:30:00Z", "root-call", 100, 40)],
    );
    fs::write(&fork_path, fork_rollout()).expect("fork rollout is written");
    let roots = discover_registered(std::slice::from_ref(&root));
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    let first_aggregate = codex_aggregate(&mut index);
    assert_eq!(first.call_count, 2);
    assert_eq!(first_aggregate.tokens.total_tokens, 130);
    assert_eq!(first.calls_added, 2);
    let mut first_totals = block_on(index.canonical_calls())
        .expect("current calls load")
        .calls
        .into_iter()
        .map(|call| call.usage.total_tokens)
        .collect::<Vec<_>>();
    first_totals.sort_unstable();
    assert_eq!(first_totals, vec![20, 110]);

    drop(index);
    append_rollout(&fork_path, fork_append());
    let mut reopened = open_index(app_temp.path());
    let second = scan(&mut reopened, &roots, &CancellationToken::new());
    let second_aggregate = codex_aggregate(&mut reopened);
    assert_eq!(second.calls_added, 1);
    assert_eq!(second.call_count, 3);
    assert_eq!(second_aggregate.tokens.total_tokens, 140);
    let persisted = block_on(reopened.aggregate_for_provider(ProviderKind::RolloutJsonl))
        .expect("persisted provider aggregate loads");
    assert_eq!(persisted.call_count, 3);
    assert_eq!(persisted.tokens.total_tokens, 140);
}

/// 未解析的 fork 所有权警告在文件未变的后续扫描中也必须继续可见。
#[test]
fn unchanged_unresolved_fork_remains_partial() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root = create_root(source_temp.path(), "root");
    fs::write(
        root.join("sessions/rollout-unresolved.jsonl"),
        fork_prefix(),
    )
    .expect("unresolved fork is written");
    let roots = discover_registered(std::slice::from_ref(&root));
    let mut index = open_index(app_temp.path());

    let first = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(first.call_count, 0);
    assert_eq!(first.coverage.warning_count, 1);
    assert_eq!(
        first.coverage.state,
        loki_metis_core::CoverageState::Partial
    );

    let unchanged = scan(&mut index, &roots, &CancellationToken::new());
    assert_eq!(unchanged.unchanged_files, 1);
    assert_eq!(unchanged.call_count, 0);
    assert_eq!(unchanged.coverage.warning_count, 1);
    assert_eq!(
        unchanged.coverage.state,
        loki_metis_core::CoverageState::Partial
    );
}
