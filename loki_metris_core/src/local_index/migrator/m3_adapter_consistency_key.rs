//! 为 `usage_calls` 补充跨根一致性判定用的适配器内部摘要列。

use sea_orm_migration::prelude::*;

const UP_SQL: &str = "ALTER TABLE usage_calls
   ADD COLUMN adapter_consistency_key TEXT;";

const DOWN_SQL: &str = "ALTER TABLE usage_calls DROP COLUMN adapter_consistency_key;";

/// 增加 `adapter_consistency_key` 列。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 为调用记录增加适配器一致性键以支持稳定去重。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    /// 回滚适配器一致性键字段与相关索引。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}
