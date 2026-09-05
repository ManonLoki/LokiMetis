//! 为 `source_roots` 补充唯一主数据目录标记与其部分唯一索引。

use sea_orm_migration::prelude::*;

const UP_SQL: &str = "ALTER TABLE source_roots
   ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 0
     CHECK(is_primary IN (0, 1) AND (is_primary = 0 OR enabled = 1));
 CREATE UNIQUE INDEX source_roots_single_primary_idx
   ON source_roots(is_primary) WHERE is_primary = 1;";

const DOWN_SQL: &str = "DROP INDEX IF EXISTS source_roots_single_primary_idx;
 ALTER TABLE source_roots DROP COLUMN is_primary;";

/// 增加 `is_primary` 列与只允许一行为主根的部分唯一索引。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 为数据根登记增加本机主目录标记。
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    /// 回滚数据根的本机主目录标记。
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}
