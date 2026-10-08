//! 为来源 checkpoint 增加 adapter 私有、内容无关的解析状态。

use super::table_exists;
use sea_orm_migration::prelude::*;

const UP_SQL: &str = "ALTER TABLE source_files ADD COLUMN adapter_state TEXT;";
const DOWN_SQL: &str = "ALTER TABLE source_files DROP COLUMN adapter_state;";

/// 增加不向 core 解释的 adapter 增量解析状态列。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 创建按适配器隔离的解析状态表。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        if table_exists(connection, "source_files").await? {
            connection.execute_unprepared(UP_SQL).await?;
        }
        Ok(())
    }

    /// 删除按适配器隔离的解析状态表。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        if table_exists(connection, "source_files").await? {
            connection.execute_unprepared(DOWN_SQL).await?;
        }
        Ok(())
    }
}
