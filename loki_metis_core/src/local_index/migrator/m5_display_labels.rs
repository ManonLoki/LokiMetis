//! 为 usage_calls 与 source_files 增加受控项目/线程展示标签列。

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

/// 增加可空的项目末段与线程标题列。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 为来源与调用增加脱敏展示标签字段。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        for sql in [
            "ALTER TABLE usage_calls ADD COLUMN project_label TEXT",
            "ALTER TABLE usage_calls ADD COLUMN thread_label TEXT",
        ] {
            connection.execute_unprepared(sql).await?;
        }
        if table_exists(connection, "source_files").await? {
            for sql in [
                "ALTER TABLE source_files ADD COLUMN project_label TEXT",
                "ALTER TABLE source_files ADD COLUMN thread_label TEXT",
            ] {
                connection.execute_unprepared(sql).await?;
            }
        }
        Ok(())
    }

    /// 回滚脱敏展示标签字段。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        for sql in [
            "ALTER TABLE usage_calls DROP COLUMN project_label",
            "ALTER TABLE usage_calls DROP COLUMN thread_label",
        ] {
            connection.execute_unprepared(sql).await?;
        }
        if table_exists(connection, "source_files").await? {
            for sql in [
                "ALTER TABLE source_files DROP COLUMN project_label",
                "ALTER TABLE source_files DROP COLUMN thread_label",
            ] {
                connection.execute_unprepared(sql).await?;
            }
        }
        Ok(())
    }
}

/// `source_files` 在从旧版 `PRAGMA user_version` schema 桥接而来的库中可能
/// 缺失（`local_index::tests::migrates_v1_known_values_through_current_schema_without_turning_them_unknown`
/// 覆盖了这种真实历史 v1 schema）；usage_calls 则由 m1 或桥接逻辑保证始终存在，无需探测。
async fn table_exists(connection: &impl ConnectionTrait, name: &str) -> Result<bool, DbErr> {
    let row = connection
        .query_one(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name.into()],
        ))
        .await?;
    Ok(row.is_some())
}
