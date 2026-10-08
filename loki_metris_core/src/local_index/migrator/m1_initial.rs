//! 从空数据库创建本机索引最初的五张表与索引。

use sea_orm_migration::prelude::*;

const UP_SQL: &str = "CREATE TABLE source_roots (
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
 CREATE INDEX usage_calls_logical_call_idx
   ON usage_calls(logical_call_id);
 CREATE INDEX usage_calls_occurred_idx
   ON usage_calls(occurred_at_epoch_ms);
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
 );";

const DOWN_SQL: &str = "DROP TABLE IF EXISTS usage_calls;
 DROP TABLE IF EXISTS source_files;
 DROP TABLE IF EXISTS scan_runs;
 DROP TABLE IF EXISTS provider_snapshots;
 DROP TABLE IF EXISTS source_roots;";

/// 创建 `source_roots`/`source_files`/`usage_calls`/`scan_runs`/`provider_snapshots` 五张表。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 创建本机索引初始表、索引与约束。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    /// 按依赖逆序删除初始本机索引结构。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}
