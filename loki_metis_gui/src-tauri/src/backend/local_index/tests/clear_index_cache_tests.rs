//! 审计「清空本产品索引」对派生 SQLite 表的真实效果，不重写 DELETE 语句。

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use tauri::async_runtime::block_on;
use tempfile::TempDir;

use super::support::{
    create_root, discover_registered, reopen_for_fixture, scan, token_line, write_rollout,
};
use crate::backend::local_index::{CancellationToken, LocalIndex, PARSER_VERSION};
use loki_metis_core::{Completeness, Confidence, Freshness, MetricFact, MetricScope, ProviderKind};

/// 在隔离 app-data 内按测试固定 parser 版本打开索引。
fn open_index(app_data_dir: &std::path::Path) -> LocalIndex {
    block_on(LocalIndex::open_in_app_data(app_data_dir, PARSER_VERSION)).expect("index opens")
}

/// 对已提交的派生表做 COUNT(*)；表名只来自本测试硬编码白名单。
fn table_count(connection: &DatabaseConnection, table: &str) -> i64 {
    assert!(
        matches!(
            table,
            "usage_calls" | "source_files" | "scan_runs" | "provider_snapshots" | "source_roots"
        ),
        "table name must be a known derived or registry table"
    );
    let row = block_on(connection.query_one(Statement::from_string(
        DbBackend::Sqlite,
        format!("SELECT COUNT(*) FROM {table}"),
    )))
    .expect("count query succeeds")
    .expect("count row exists");
    row.try_get_by_index(0).expect("count column is readable")
}

/// 读取全部登记根的 last_coverage_state，供清空前后对照。
fn last_coverage_states(connection: &DatabaseConnection) -> Vec<Option<String>> {
    let rows = block_on(connection.query_all(Statement::from_string(
        DbBackend::Sqlite,
        "SELECT last_coverage_state FROM source_roots ORDER BY root_id",
    )))
    .expect("coverage query succeeds");
    rows.into_iter()
        .map(|row| {
            row.try_get_by_index::<Option<String>>(0)
                .expect("coverage column is readable")
        })
        .collect()
}

/// 写入一条只含规范化本机字段的合成快照。
fn seed_provider_snapshot(index: &mut LocalIndex) {
    let fact = MetricFact::new(
        0_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        1,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        None,
    );
    let normalized_json = serde_json::to_string(&fact).expect("local fact serializes");
    block_on(index.save_normalized_snapshot(1, 2, &normalized_json))
        .expect("provider snapshot is seeded");
}

/// 真实扫描写入调用、checkpoint、scan_runs 与 provider 快照后，clear_index 必须清派生表并保留登记根。
#[test]
fn clear_index_deletes_derived_tables_and_nulls_coverage_but_keeps_registry() {
    let source_temp = TempDir::new().expect("source temp is available");
    let app_temp = TempDir::new().expect("app-data temp is available");
    let root_path = create_root(source_temp.path(), "root");
    let rollout = root_path.join("sessions/rollout-clear-cache.jsonl");
    write_rollout(
        &rollout,
        "session-clear-cache",
        &[token_line("2026-07-30T10:01:00Z", "call-a", 10, 2)],
    );
    let roots = discover_registered(&[root_path]);
    let mut index = open_index(app_temp.path());
    scan(&mut index, &roots, &CancellationToken::new());
    seed_provider_snapshot(&mut index);
    let database_path = index.database_path().to_path_buf();

    let before = reopen_for_fixture(&database_path);
    assert!(table_count(&before, "usage_calls") > 0);
    assert!(table_count(&before, "source_files") > 0);
    assert!(table_count(&before, "scan_runs") > 0);
    assert!(table_count(&before, "provider_snapshots") > 0);
    assert_eq!(table_count(&before, "source_roots"), 1);
    assert!(
        last_coverage_states(&before)
            .iter()
            .all(|state| state.is_some()),
        "scan must persist last_coverage_state so the NULL update is observable"
    );
    drop(before);

    block_on(index.clear_index()).expect("product index is cleared");
    drop(index);

    let after = reopen_for_fixture(&database_path);
    assert_eq!(table_count(&after, "usage_calls"), 0);
    assert_eq!(table_count(&after, "source_files"), 0);
    assert_eq!(table_count(&after, "scan_runs"), 0);
    assert_eq!(table_count(&after, "provider_snapshots"), 0);
    assert_eq!(table_count(&after, "source_roots"), 1);
    assert!(
        last_coverage_states(&after)
            .iter()
            .all(|state| state.is_none()),
        "clear_index must NULL source_roots.last_coverage_state"
    );
    assert!(
        rollout.exists(),
        "original Codex rollout must remain untouched"
    );
}
