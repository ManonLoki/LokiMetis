//! 为已登记数据根增加独立的首次索引激活状态。

use super::table_exists;
use sea_orm_migration::prelude::*;

/// 增加根激活状态；升级前已有根保留登记但必须显式重建索引。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 添加 `activation_state`，并移除旧派生索引以便按新语义重建。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        connection
            .execute_unprepared(
                "ALTER TABLE source_roots ADD COLUMN activation_state TEXT NOT NULL DEFAULT 'confirmed_unindexed'",
            )
            .await?;
        for sql in [
            ("usage_calls", "DELETE FROM usage_calls"),
            ("source_files", "DELETE FROM source_files"),
            ("scan_runs", "DELETE FROM scan_runs"),
        ] {
            if table_exists(connection, sql.0).await? {
                connection.execute_unprepared(sql.1).await?;
            }
        }
        Ok(())
    }

    /// 删除新增字段；派生索引不会在回滚时恢复。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE source_roots DROP COLUMN activation_state")
            .await?;
        Ok(())
    }
}
