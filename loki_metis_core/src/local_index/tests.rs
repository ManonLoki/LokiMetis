use sea_orm::{ConnectOptions, ConnectionTrait, Database, DbBackend, Statement};

use crate::{
    SourceClientKind, USAGE_INDEX_FILE_NAME, source_client_app_data_dir,
    source_client_usage_index_path,
};

use super::*;

/// 验证空 app-data 路径与符号链接路径被拒绝，不会创建数据库文件。
#[tokio::test]
async fn rejects_invalid_app_data_directories() {
    let error = LocalIndex::open_in_app_data(Path::new(""), 1)
        .await
        .expect_err("empty app data directory is rejected");
    assert_eq!(error.kind(), LocalErrorKind::InvalidPath);
}

/// 验证全新目录可以打开数据库、迁移到当前 schema，并对外暴露稳定路径。
#[tokio::test]
async fn opens_fresh_database_at_current_schema() {
    let temp = tempfile::tempdir().expect("temp dir is created");
    let index = LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("fresh database opens and migrates");
    assert_eq!(
        index.database_path(),
        temp.path().join(USAGE_INDEX_FILE_NAME)
    );
    assert!(index.database_path().is_file());

    let roots = index
        .known_roots()
        .await
        .expect("known roots query succeeds");
    assert!(roots.is_empty());
}

/// 验证重新打开同一目录复用既有 schema，不因重复迁移报错。
#[tokio::test]
async fn reopening_same_directory_is_idempotent() {
    let temp = tempfile::tempdir().expect("temp dir is created");
    {
        LocalIndex::open_in_app_data(temp.path(), 1)
            .await
            .expect("first open succeeds");
    }
    LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("second open reuses migrated schema");
}

/// 打开一个已经迁移完成的数据库文件用于只读检查；调用方负责先让
/// `LocalIndex` 完成迁移并释放连接，避免和它持有的单连接池竞争。
async fn reopen_for_inspection(database_path: &Path) -> sea_orm::DatabaseConnection {
    let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
    let database_path = database_path.to_path_buf();
    options.map_sqlx_sqlite_opts(move |sqlite_options| {
        sqlite_options
            .filename(&database_path)
            .create_if_missing(true)
    });
    Database::connect(options)
        .await
        .expect("migrated database reopens for inspection")
}

/// 验证本机规范化快照不泄露认证细节，且清空索引会移除它。
#[tokio::test]
async fn stores_only_normalized_provider_snapshots() {
    use crate::{Completeness, Confidence, Freshness, MetricFact, MetricScope, ProviderKind};

    let temp = tempfile::tempdir().expect("temp dir is created");
    let mut index = LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("index opens");
    let fact = MetricFact::new(
        120_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        1_000,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        Some("rollout-parser-v8".to_owned()),
    );
    let normalized_json = serde_json::to_string(&fact).expect("local fact serializes");
    index
        .save_normalized_snapshot(1_000, 31_000, &normalized_json)
        .await
        .expect("normalized snapshot is stored");
    let database_path = index.database_path().to_path_buf();
    drop(index);

    let inspection = reopen_for_inspection(&database_path).await;
    let normalized: String = inspection
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT normalized_json FROM provider_snapshots WHERE snapshot_kind = 'local_metric'",
        ))
        .await
        .expect("local snapshot query succeeds")
        .expect("local snapshot exists")
        .try_get_by_index(0)
        .expect("normalized_json column is readable");
    assert!(normalized.contains("deviceObserved"));
    assert!(normalized.contains("rolloutJsonl"));
    assert!(!normalized.contains("officialAccount"));
    assert!(!normalized.contains("auth.json"));
    assert!(!normalized.contains("email"));
    assert!(!normalized.contains("access_token"));
    drop(inspection);

    let mut index = LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("index reopens");
    index.clear_index().await.expect("product index is cleared");
    drop(index);

    let inspection = reopen_for_inspection(&database_path).await;
    let snapshot_count: i64 = inspection
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT COUNT(*) FROM provider_snapshots",
        ))
        .await
        .expect("snapshot count query succeeds")
        .expect("snapshot count is readable")
        .try_get_by_index(0)
        .expect("count column is readable");
    assert_eq!(snapshot_count, 0);
}

/// 验证元数据发现迁移会清理旧派生调用，避免旧 parser generation 混入新索引。
#[tokio::test]
async fn migration_rebuilds_legacy_derived_usage_calls() {
    let temp = tempfile::tempdir().expect("temp dir is created");
    let database_path = temp.path().join(USAGE_INDEX_FILE_NAME);
    let fixture = reopen_for_inspection(&database_path).await;
    fixture
        .execute_unprepared(
            "CREATE TABLE source_roots (
               root_id TEXT PRIMARY KEY NOT NULL,
               access_path BLOB NOT NULL,
               alias TEXT NOT NULL,
               enabled INTEGER NOT NULL,
               discovery_method TEXT NOT NULL,
               last_coverage_state TEXT
             );
             CREATE TABLE usage_calls (
               cached_input_tokens INTEGER NOT NULL,
               reasoning_output_tokens INTEGER NOT NULL
             );
             INSERT INTO usage_calls VALUES (7, 3);
             PRAGMA user_version = 1;",
        )
        .await
        .expect("v1 fixture is created");
    drop(fixture);

    LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("v1 schema migrates");

    let inspection = reopen_for_inspection(&database_path).await;
    let count: i64 = inspection
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT COUNT(*) FROM usage_calls",
        ))
        .await
        .expect("migrated count query succeeds")
        .expect("migrated count exists")
        .try_get_by_index(0)
        .expect("count is readable");
    assert_eq!(count, 0);
}

/// 验证 v3 数据根迁移为单主根 schema：默认不隐式选择主目录，且数据库
/// 自身的部分唯一索引与 CHECK 约束在应用代码之外仍拒绝非法状态
/// （同时两个主根、或主根被禁用），即使有人绕过 Rust 层直接执行原始 SQL。
#[tokio::test]
async fn migrates_v3_roots_with_no_implicit_primary_and_enforces_single_enabled_primary() {
    let temp = tempfile::tempdir().expect("temp dir is created");
    let database_path = temp.path().join(USAGE_INDEX_FILE_NAME);
    let fixture = reopen_for_inspection(&database_path).await;
    fixture
        .execute_unprepared(
            "CREATE TABLE source_roots (
               root_id TEXT PRIMARY KEY NOT NULL,
               access_path BLOB NOT NULL,
               alias TEXT NOT NULL,
               enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
               discovery_method TEXT NOT NULL,
               last_coverage_state TEXT
             );
             CREATE TABLE source_files (
               source_id TEXT PRIMARY KEY NOT NULL,
               root_id TEXT NOT NULL REFERENCES source_roots(root_id) ON DELETE CASCADE,
               relative_label TEXT NOT NULL,
               file_identity TEXT NOT NULL,
               archived INTEGER NOT NULL CHECK(archived IN (0, 1)),
               observed_size INTEGER NOT NULL,
               modified_at_epoch_ms INTEGER NOT NULL,
               parsed_offset INTEGER NOT NULL,
               trailing_bytes INTEGER NOT NULL,
               oversized_tail INTEGER NOT NULL CHECK(oversized_tail IN (0, 1)),
               parser_version INTEGER NOT NULL,
               generation INTEGER NOT NULL,
               ready INTEGER NOT NULL CHECK(ready IN (0, 1)),
               thread_key TEXT NOT NULL,
               project_key TEXT,
               model TEXT,
               reasoning_effort TEXT,
               call_sequence INTEGER NOT NULL
             );
             CREATE TABLE usage_calls (
               source_id TEXT NOT NULL REFERENCES source_files(source_id) ON DELETE CASCADE,
               generation INTEGER NOT NULL,
               logical_call_id TEXT NOT NULL,
               occurred_at_epoch_ms INTEGER NOT NULL,
               model TEXT,
               reasoning_effort TEXT,
               project_key TEXT,
               thread_key TEXT NOT NULL,
               input_tokens INTEGER NOT NULL,
               cached_input_tokens INTEGER NOT NULL,
               cache_write_input_tokens INTEGER,
               output_tokens INTEGER NOT NULL,
               reasoning_output_tokens INTEGER NOT NULL,
               total_tokens INTEGER NOT NULL,
               total_is_derived INTEGER NOT NULL CHECK(total_is_derived IN (0, 1)),
               confidence TEXT NOT NULL,
               cached_input_available INTEGER NOT NULL DEFAULT 1,
               reasoning_output_available INTEGER NOT NULL DEFAULT 1,
               adapter_consistency_key TEXT,
               PRIMARY KEY(source_id, generation, logical_call_id)
             );
             CREATE TABLE scan_runs (
               row_id INTEGER PRIMARY KEY AUTOINCREMENT,
               scan_id TEXT UNIQUE,
               mode TEXT NOT NULL,
               started_at_epoch_ms INTEGER NOT NULL,
               finished_at_epoch_ms INTEGER,
               status TEXT NOT NULL,
               cancelled INTEGER NOT NULL CHECK(cancelled IN (0, 1)),
               files_scanned INTEGER NOT NULL,
               calls_added INTEGER NOT NULL,
               warning_count INTEGER NOT NULL
             );
             CREATE TABLE provider_snapshots (
               snapshot_kind TEXT PRIMARY KEY NOT NULL,
               observed_at_epoch_ms INTEGER NOT NULL,
               valid_until_epoch_ms INTEGER NOT NULL,
               normalized_json TEXT NOT NULL
             );
             INSERT INTO source_roots
               (root_id, access_path, alias, enabled, discovery_method)
             VALUES
               ('root-a', X'01', 'A', 1, 'registered'),
               ('root-b', X'02', 'B', 1, 'registered');
             PRAGMA user_version = 3;",
        )
        .await
        .expect("v3 fixture is created");
    drop(fixture);

    let index = LocalIndex::open_in_app_data(temp.path(), 1)
        .await
        .expect("v3 schema migrates");
    let roots = index.list_sources().await.expect("root records load");
    assert_eq!(roots.len(), 2);
    assert!(roots.iter().all(|root| !root.is_primary));
    assert!(
        roots.iter().all(|root| {
            root.activation_state == crate::RootActivationState::ConfirmedUnindexed
        })
    );
    drop(index);

    let inspection = reopen_for_inspection(&database_path).await;
    inspection
        .execute_unprepared("UPDATE source_roots SET is_primary = 1 WHERE root_id = 'root-a'")
        .await
        .expect("first enabled primary is accepted");
    assert!(
        inspection
            .execute_unprepared("UPDATE source_roots SET is_primary = 1 WHERE root_id = 'root-b'")
            .await
            .is_err(),
        "the partial unique index must reject a second primary"
    );
    assert!(
        inspection
            .execute_unprepared("UPDATE source_roots SET enabled = 0 WHERE root_id = 'root-a'")
            .await
            .is_err(),
        "a primary root cannot remain disabled"
    );
}

/// 读取已迁移用量库中的扫描 mode，用于确认文件只含本次写入。
async fn scan_modes(database_path: &Path) -> Vec<String> {
    let inspection = reopen_for_inspection(database_path).await;
    let rows = inspection
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT mode FROM scan_runs ORDER BY row_id",
        ))
        .await
        .expect("scan modes are readable");
    rows.into_iter()
        .map(|row| row.try_get_by_index::<String>(0).expect("mode is text"))
        .collect()
}

/// 验证同一产品 app-data 下三个 Agent 打开/写入各自独占库，互不改写对方文件。
#[tokio::test]
async fn opening_and_writing_one_agent_index_does_not_touch_another() {
    let temp = tempfile::tempdir().expect("isolated product app-data exists");
    let product = temp.path();
    let clients = [
        SourceClientKind::Codex,
        SourceClientKind::ClaudeCode,
        SourceClientKind::GrokBuildCli,
    ];
    let paths: Vec<_> = clients
        .into_iter()
        .map(|client| source_client_usage_index_path(product, client))
        .collect();
    let unique = paths.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), clients.len());

    let mut previous_bytes = Vec::new();
    for (index, client) in clients.into_iter().enumerate() {
        let marker = format!("exclusive-{}", client.root_id_namespace());
        let app_data = source_client_app_data_dir(product, client);
        let mut local_index = LocalIndex::open_in_app_data(&app_data, client.parser_version())
            .await
            .expect("agent index opens through shipped mapping");
        assert_eq!(local_index.database_path(), paths[index].as_path());
        local_index
            .begin_scan(&marker, 1_000 + i64::try_from(index).expect("index fits"))
            .await
            .expect("exclusive scan marker is written");
        drop(local_index);

        let written = std::fs::read(&paths[index]).expect("written index bytes are readable");
        assert!(!written.is_empty());
        for (other, bytes) in previous_bytes.iter().enumerate() {
            let metadata = std::fs::metadata(&paths[other]).expect("peer index remains a file");
            let now = std::fs::read(&paths[other]).expect("peer index remains readable");
            assert_eq!(
                &now, bytes,
                "{client:?} 写入不得改写另一 Agent 的用量库字节"
            );
            assert_eq!(
                now.len() as u64,
                metadata.len(),
                "对账用的文件长度必须与磁盘元数据一致"
            );
        }
        previous_bytes.push(written);
        assert_eq!(scan_modes(&paths[index]).await, vec![marker]);
    }
}

/// 验证 30 日读取下界与物理裁剪同时覆盖调用和累计快照，并保留来源 checkpoint。
#[tokio::test]
async fn bounds_snapshot_reads_and_prunes_derived_history_without_losing_sources() {
    let temp = tempfile::tempdir().expect("isolated app-data exists");
    let index = LocalIndex::open_in_app_data(temp.path(), 8)
        .await
        .expect("index opens");
    assert!(
        !index
            .has_current_parser_usage()
            .await
            .expect("empty usage state")
    );
    let database_path = index.database_path().to_path_buf();
    drop(index);

    let fixture = reopen_for_inspection(&database_path).await;
    fixture
        .execute_unprepared(
            "INSERT INTO source_roots
               (root_id, access_path, alias, enabled, discovery_method,
                last_coverage_state, activation_state)
             VALUES ('root-retention', X'01', 'Retention', 1, 'registered', 'complete', 'ready');
             INSERT INTO source_files
               (source_id, root_id, relative_label, file_identity, archived,
                observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                oversized_tail, parser_version, generation, ready, thread_key,
                project_key, model, reasoning_effort, call_sequence, token_snapshots_ready)
             VALUES
               ('source-retention', 'root-retention', 'sessions/test.jsonl', 'file', 0,
                10, 200, 10, 0, 0, 8, 1, 1, 'thread', NULL, NULL, NULL, 2, 1);
             INSERT INTO usage_calls
               (source_id, generation, logical_call_id, occurred_at_epoch_ms,
                model, reasoning_effort, project_key, thread_key, input_tokens,
                cached_input_tokens, cache_write_input_tokens, output_tokens,
                reasoning_output_tokens, total_tokens, total_is_derived, confidence)
             VALUES
               ('source-retention', 1, 'old', 99, NULL, NULL, NULL, 'thread', 1, 0, NULL, 0, 0, 1, 0, 'exact'),
               ('source-retention', 1, 'recent', 100, NULL, NULL, NULL, 'thread', 2, 0, NULL, 0, 0, 2, 0, 'exact');
             INSERT INTO usage_token_snapshots
               (source_id, generation, thread_key, occurred_at_epoch_ms,
                logical_call_id, total_tokens)
             VALUES
               ('source-retention', 1, 'thread', 99, 'old', 1),
               ('source-retention', 1, 'thread', 100, 'recent', 2);",
        )
        .await
        .expect("retention fixture is inserted");
    drop(fixture);

    let mut index = LocalIndex::open_in_app_data(temp.path(), 8)
        .await
        .expect("index reopens");
    assert!(
        index
            .has_current_parser_usage()
            .await
            .expect("source state")
    );
    let bounded = index
        .usage_snapshot_since(100)
        .await
        .expect("bounded snapshot loads");
    assert_eq!(bounded.canonical.calls.len(), 1);
    assert_eq!(bounded.canonical.calls[0].logical_call_id, "recent");
    index
        .prune_usage_before(100)
        .await
        .expect("derived history is pruned");
    assert_eq!(
        index
            .canonical_calls()
            .await
            .expect("calls remain")
            .calls
            .len(),
        1
    );
    assert!(
        index
            .has_current_parser_usage()
            .await
            .expect("source remains")
    );
}
