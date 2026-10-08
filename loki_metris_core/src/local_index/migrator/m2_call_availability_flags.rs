//! 为 `usage_calls` 补充缓存输入与推理输出的可得性标记列。

use sea_orm_migration::prelude::*;

const UP_SQL: &str = "ALTER TABLE usage_calls
   ADD COLUMN cached_input_available INTEGER NOT NULL DEFAULT 1
     CHECK(cached_input_available IN (0, 1));
 ALTER TABLE usage_calls
   ADD COLUMN reasoning_output_available INTEGER NOT NULL DEFAULT 1
     CHECK(reasoning_output_available IN (0, 1));";

const DOWN_SQL: &str = "ALTER TABLE usage_calls DROP COLUMN reasoning_output_available;
 ALTER TABLE usage_calls DROP COLUMN cached_input_available;";

/// 增加 `cached_input_available`/`reasoning_output_available` 两列。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 为调用表增加 Token 分项可用性标记。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    /// 回滚调用表的 Token 分项可用性标记。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}
