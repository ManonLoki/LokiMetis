//! 新增 Codex 会话 cumulative 快照表；ADR-104 后仅作历史诊断与索引兼容。
//!
//! 这是 schema 迁移，不是 parser generation：`usage_calls` 仍存 ADR-097 增量 last，
//! 无法用调用行还原历史 cumulative。旧索引可在下次扫描时回填兼容快照。

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

const UP_SQL: &str = "CREATE TABLE usage_token_snapshots (
   source_id TEXT NOT NULL REFERENCES source_files(source_id) ON DELETE CASCADE,
   generation INTEGER NOT NULL,
   thread_key TEXT NOT NULL,
   occurred_at_epoch_ms INTEGER NOT NULL,
   logical_call_id TEXT NOT NULL,
   total_tokens INTEGER NOT NULL,
   model TEXT,
   reasoning_effort TEXT,
   project_key TEXT,
   PRIMARY KEY(source_id, generation, thread_key, occurred_at_epoch_ms, logical_call_id)
 );
 CREATE INDEX usage_token_snapshots_thread_time_idx
   ON usage_token_snapshots(thread_key, occurred_at_epoch_ms);
 ALTER TABLE source_files ADD COLUMN token_snapshots_ready INTEGER NOT NULL DEFAULT 0 CHECK(token_snapshots_ready IN (0, 1));";

const DOWN_SQL: &str = "DROP INDEX IF EXISTS usage_token_snapshots_thread_time_idx;
 DROP TABLE IF EXISTS usage_token_snapshots;";

/// 创建 `usage_token_snapshots`，并为来源文件标记快照回填状态。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 创建会话 Token 快照表以支持区段增量会计。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        if table_exists(connection, "source_files").await? {
            connection.execute_unprepared(UP_SQL).await?;
        }
        Ok(())
    }

    /// 删除会话 Token 快照表及其索引。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}

/// 探测桥接旧 schema 中可能不存在的来源表。
async fn table_exists(connection: &impl ConnectionTrait, name: &str) -> Result<bool, DbErr> {
    Ok(connection
        .query_one(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name.into()],
        ))
        .await?
        .is_some())
}
