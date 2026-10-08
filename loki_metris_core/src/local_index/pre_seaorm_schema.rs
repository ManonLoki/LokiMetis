//! 把旧版 `PRAGMA user_version` 管理的数据库无损接入 SeaORM 迁移记账。
//!
//! 本产品在切换到 SeaORM 之前，用手写 `rusqlite` 迁移并把当前 schema
//! 版本号存在 SQLite 内置的 `PRAGMA user_version` 里；`sea-orm-migration`
//! 改用自己的 `seaql_migrations` 表记录“哪些迁移已经跑过”。两者对不上：
//! 如果什么都不做，已发布版本的真实用户数据库会在下次启动时被
//! `sea-orm-migration` 当成空库，重新执行 `CREATE TABLE`，直接报错
//! （表已存在）而无法启动。这个模块在正式迁移器运行前抢先探测旧库、
//! 把它已经完成的迁移原样登记进 `seaql_migrations`，让迁移器只补跑
//! 真正缺失的步骤，不会重跑已经生效的 DDL，也不会动用户已有的数据。
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use super::LocalError;
use super::error::LocalErrorKind;
use super::migrator::migration_names;

const SEAQL_MIGRATIONS_TABLE_NAME: &str = "seaql_migrations";

/// 若检测到旧版 `PRAGMA user_version` 数据库，登记其已完成的迁移；否则不做任何事。
pub(crate) async fn adopt_pre_seaorm_schema_if_present(
    connection: &DatabaseConnection,
) -> Result<(), LocalError> {
    if migrations_table_exists(connection).await? {
        // 要么是全新数据库（由本次运行的迁移器自己建表），要么是已经
        // 完成过一次桥接或原生诞生于 SeaORM 时代的库：两种情况都不需要
        // 再做任何事，交给正常迁移流程接管。
        return Ok(());
    }

    let pre_seaorm_version = read_pre_seaorm_user_version(connection).await?;
    if pre_seaorm_version == 0 {
        // `user_version` 默认就是 0；一个从未被旧代码迁移过的全新文件
        // 走到这里也是 0，同样交给正常迁移流程从头建表。
        return Ok(());
    }
    let pre_seaorm_version = usize::try_from(pre_seaorm_version).map_err(|_| {
        LocalError::new(
            LocalErrorKind::UnsupportedSchema,
            "local index schema version is invalid",
        )
    })?;
    let names = migration_names();
    if pre_seaorm_version > names.len() {
        // 保留原实现的安全保护：数据库被更新版本的程序写过时拒绝继续，
        // 避免旧代码误读不认识的新增字段。
        return Err(LocalError::new(
            LocalErrorKind::UnsupportedSchema,
            "local index schema is newer than this application",
        ));
    }

    create_migrations_table(connection).await?;
    for name in &names[..pre_seaorm_version] {
        mark_migration_applied(connection, name).await?;
    }
    Ok(())
}

/// 检查 `seaql_migrations` 记账表是否已存在。
async fn migrations_table_exists(connection: &DatabaseConnection) -> Result<bool, LocalError> {
    let statement = Statement::from_sql_and_values(
        connection.get_database_backend(),
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [SEAQL_MIGRATIONS_TABLE_NAME.into()],
    );
    Ok(connection.query_one(statement).await?.is_some())
}

/// 读取旧版迁移写入的 `PRAGMA user_version`；从未迁移过的数据库返回 0。
async fn read_pre_seaorm_user_version(connection: &DatabaseConnection) -> Result<i64, LocalError> {
    let statement =
        Statement::from_string(connection.get_database_backend(), "PRAGMA user_version");
    let row = connection.query_one(statement).await?.ok_or_else(|| {
        LocalError::new(LocalErrorKind::Database, "local index schema is unreadable")
    })?;
    Ok(row.try_get_by_index(0)?)
}

/// 建立与 `sea-orm-migration` 自身完全兼容的记账表；已存在时是无操作。
async fn create_migrations_table(connection: &DatabaseConnection) -> Result<(), LocalError> {
    connection
        .execute_unprepared(
            "CREATE TABLE IF NOT EXISTS seaql_migrations (
               version TEXT NOT NULL PRIMARY KEY,
               applied_at BIGINT NOT NULL
             )",
        )
        .await?;
    Ok(())
}

/// 把一个迁移名登记为“旧代码已经完成”，供迁移器识别为无需重跑。
async fn mark_migration_applied(
    connection: &DatabaseConnection,
    name: &str,
) -> Result<(), LocalError> {
    let applied_at = jiff::Timestamp::now().as_second();
    let statement = Statement::from_sql_and_values(
        connection.get_database_backend(),
        "INSERT OR IGNORE INTO seaql_migrations (version, applied_at) VALUES (?1, ?2)",
        [name.into(), applied_at.into()],
    );
    connection.execute(statement).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectOptions, Database};
    use sea_orm_migration::MigratorTrait;

    use super::*;
    use crate::local_index::migrator::Migrator;

    /// 构造一个只经过手写 v1 建表（不含后续 ALTER）的旧库，桥接后迁移器
    /// 应当只补跑 m2..m4，且保留已有数据不被重建破坏。
    async fn pre_seaorm_connection_at_version(version: i64) -> DatabaseConnection {
        let connection = Database::connect(ConnectOptions::new("sqlite::memory:"))
            .await
            .expect("in-memory database opens");
        connection
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
                 );",
            )
            .await
            .expect("legacy base schema is created");
        connection
            .execute_unprepared(
                "INSERT INTO source_roots
                 (root_id, access_path, alias, enabled, discovery_method)
                 VALUES ('root-1', x'2f746d70', 'fixture', 1, 'registered')",
            )
            .await
            .expect("legacy fixture row is inserted");
        connection
            .execute_unprepared(&format!("PRAGMA user_version = {version}"))
            .await
            .expect("legacy user_version is set");
        connection
    }

    /// 验证桥接把旧库登记为已完成 m1，随后迁移器只补跑剩余步骤且保留旧数据。
    #[tokio::test]
    async fn adopts_pre_seaorm_version_one_database_without_losing_data() {
        let connection = pre_seaorm_connection_at_version(1).await;

        adopt_pre_seaorm_schema_if_present(&connection)
            .await
            .expect("legacy database is adopted");
        Migrator::up(&connection, None)
            .await
            .expect("remaining migrations apply cleanly");

        let alias: String = connection
            .query_one(Statement::from_string(
                connection.get_database_backend(),
                "SELECT alias FROM source_roots WHERE root_id = 'root-1'",
            ))
            .await
            .expect("fixture row query succeeds")
            .expect("fixture row survives adoption")
            .try_get_by_index(0)
            .expect("alias column is readable");
        assert_eq!(alias, "fixture");

        let is_primary_column_exists = connection
            .query_one(Statement::from_string(
                connection.get_database_backend(),
                "SELECT is_primary FROM source_roots WHERE root_id = 'root-1'",
            ))
            .await
            .is_ok();
        assert!(is_primary_column_exists, "m4 backfilled is_primary column");
    }

    /// 验证已经完成全部四步的旧库桥接后不会重跑或报错。
    #[tokio::test]
    async fn adopts_pre_seaorm_version_four_database_as_fully_applied() {
        let connection = pre_seaorm_connection_at_version(4).await;
        connection
            .execute_unprepared(
                "ALTER TABLE usage_calls
                   ADD COLUMN cached_input_available INTEGER NOT NULL DEFAULT 1;
                 ALTER TABLE usage_calls
                   ADD COLUMN reasoning_output_available INTEGER NOT NULL DEFAULT 1;
                 ALTER TABLE usage_calls
                   ADD COLUMN adapter_consistency_key TEXT;
                 ALTER TABLE source_roots
                   ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 0;
                 CREATE UNIQUE INDEX source_roots_single_primary_idx
                   ON source_roots(is_primary) WHERE is_primary = 1;",
            )
            .await
            .expect("legacy database is fully upgraded to version 4 shape");

        adopt_pre_seaorm_schema_if_present(&connection)
            .await
            .expect("fully upgraded legacy database is adopted");
        Migrator::up(&connection, None)
            .await
            .expect("no pending migrations remain");
    }

    /// 验证比当前代码认识的迁移还新的数据库被安全拒绝，而不是被误当成合法版本。
    #[tokio::test]
    async fn rejects_database_newer_than_known_migrations() {
        let connection = pre_seaorm_connection_at_version(99).await;
        let error = adopt_pre_seaorm_schema_if_present(&connection)
            .await
            .expect_err("unrecognized future schema version is rejected");
        assert_eq!(error.kind(), LocalErrorKind::UnsupportedSchema);
    }

    /// 验证全新空库不会被误判为需要桥接的旧库。
    #[tokio::test]
    async fn leaves_brand_new_database_untouched() {
        let connection = Database::connect(ConnectOptions::new("sqlite::memory:"))
            .await
            .expect("in-memory database opens");
        adopt_pre_seaorm_schema_if_present(&connection)
            .await
            .expect("brand new database has nothing to adopt");
        assert!(!migrations_table_exists(&connection).await.unwrap());
    }
}
