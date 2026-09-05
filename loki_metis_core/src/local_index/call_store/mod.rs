//! 在本机索引内隔离调用行的短事务写入与 core 类型恢复。
//!
//! 这一层刻意不使用 SeaORM 的 `ActiveModel`/`QueryBuilder` DSL：批次写入
//! 依赖 `ON CONFLICT ... DO UPDATE SET x = excluded.x` 与逐字段单调比较，
//! 用查询构造器表达容易悄悄改变语义，而 SeaORM 官方就把裸 SQL
//! （[`Statement`] + [`ConnectionTrait::query_one`]/[`ConnectionTrait::execute`]）
//! 作为覆盖这类场景的正式手段；sqlx 驱动本身会按 SQL 文本自动缓存已解析
//! 的语句，所以这里不需要手工维护“准备一次、复用多次”的语句对象。

use sea_orm::{ConnectionTrait, TransactionTrait};

use crate::{SessionTokenSnapshot, UsageCall};

use super::{LocalError, LocalIndex};

impl LocalIndex {
    /// 返回指定来源 generation 中按适配器内部顺序最新的一致性检查点。
    // 一致性键的编码与解释仍归具体 adapter 所有；core 只提供严格按来源和
    // generation 隔离的只读原语，避免把 Codex 协议细节扩散进持久层。
    pub async fn latest_adapter_consistency_key(
        &self,
        source_id: &str,
        generation: u64,
    ) -> Result<Option<String>, LocalError> {
        let row = self
            .connection
            .query_one(shared::statement(
                "SELECT adapter_consistency_key FROM usage_calls
                 WHERE source_id = ?1 AND generation = ?2
                   AND adapter_consistency_key IS NOT NULL
                 ORDER BY adapter_consistency_key DESC
                 LIMIT 1",
                vec![source_id.into(), shared::to_sql_u64(generation)?.into()],
            ))
            .await?;
        row.map(|row| row.try_get_by_index::<String>(0).map_err(LocalError::from))
            .transpose()
    }

    /// 在一个短事务中插入或刷新一批来源调用（Codex 置信度替换规则），
    /// 并返回首次插入数量；批次内部语义见 [`insert_usage_batch`]。
    pub async fn insert_usage_batch(
        &mut self,
        source_id: &str,
        generation: u64,
        batch: &mut Vec<UsageCall>,
    ) -> Result<u64, LocalError> {
        let transaction = self.connection.begin().await?;
        let added = insert_usage_batch(&transaction, source_id, generation, batch).await?;
        transaction.commit().await?;
        Ok(added)
    }

    /// 在同一短事务中写入增量调用与会话 cumulative 快照。
    pub async fn insert_usage_and_snapshots(
        &mut self,
        source_id: &str,
        generation: u64,
        batch: &mut Vec<UsageCall>,
        snapshots: &mut Vec<SessionTokenSnapshot>,
    ) -> Result<u64, LocalError> {
        let transaction = self.connection.begin().await?;
        let added = insert_usage_batch(&transaction, source_id, generation, batch).await?;
        insert_token_snapshots(&transaction, source_id, generation, snapshots).await?;
        transaction.commit().await?;
        Ok(added)
    }

    /// 在一个短事务中对一批 Claude 观察执行单调 upsert；批次内部语义见
    /// [`insert_claude_usage_batch`]。
    pub async fn insert_claude_usage_batch(
        &mut self,
        source_id: &str,
        generation: u64,
        batch: &mut Vec<UsageCall>,
    ) -> Result<ClaudeBatchOutcome, LocalError> {
        let transaction = self.connection.begin().await?;
        let outcome = insert_claude_usage_batch(&transaction, source_id, generation, batch).await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

/// 汇总 Claude 单调 upsert 的新增与被拒绝回退观察数量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeBatchOutcome {
    /// 本次批次内首次插入的观察数量。
    pub added: u64,
    /// 本次批次内因回退而被拒绝、保留旧事实的观察数量。
    pub regressed: u64,
}

mod claude;
mod codex;
mod shared;
#[cfg(test)]
mod tests;

pub(crate) use claude::insert_claude_usage_batch;
pub(crate) use codex::insert_usage_batch;
pub(crate) use shared::{from_sql_u64, insert_token_snapshots, row_to_usage_call, to_sql_u64};
