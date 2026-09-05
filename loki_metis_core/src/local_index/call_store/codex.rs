//! Codex 置信度替换规则下的调用批次写入。

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseTransaction};

use crate::{Confidence, UsageCall};

use super::super::LocalError;
use super::shared::{
    DELETE_DERIVED_CALLS_SQL, INSERT_USAGE_CALL_SQL, UPSERT_USAGE_CALL_SQL, execute_usage_call,
    in_placeholders, statement, to_sql_u64,
};

/// 一次性预取批次内全部线程当前是否已经写过 exact 记录；批内如果某个
/// 线程在处理过程中才第一次写入 exact 记录，循环体会同步把它计入这个
/// 内存集合，因此和逐行查询数据库的写法在“看见批内更早写入的 exact”
/// 这件事上完全等价，只是把最多 N 次往返压成 1 次。
async fn prefetch_exact_threads(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: i64,
    thread_keys: &[String],
) -> Result<HashSet<String>, LocalError> {
    let mut exact_threads = HashSet::with_capacity(thread_keys.len());
    if thread_keys.is_empty() {
        return Ok(exact_threads);
    }
    let placeholders = in_placeholders(3, thread_keys.len());
    let mut values: Vec<sea_orm::Value> = vec![source_id.into(), generation.into()];
    values.extend(thread_keys.iter().map(Into::into));
    let rows = transaction
        .query_all(statement(
            &format!(
                "SELECT DISTINCT thread_key FROM usage_calls
                 WHERE source_id = ?1 AND generation = ?2
                   AND confidence = 'exact' AND thread_key IN ({placeholders})"
            ),
            values,
        ))
        .await?;
    for row in rows {
        exact_threads.insert(row.try_get_by_index::<String>(0)?);
    }
    Ok(exact_threads)
}

/// 在一个短事务中插入或刷新一批来源调用，并返回首次插入数量。
// Codex 侧的置信度替换规则（与上面 Claude 的“单调覆盖”是不同的策略）：
// 一条线程可能先出现推算值（Derived，比如只有累计值没有单次值时的回退），
// 之后才出现精确值（Exact）。规则是“精确事实一旦出现，永久优先”：
//   - 新记录是 Derived，但该线程已经写过 Exact 记录 -> 直接丢弃，
//     不会让较弱的推算值污染已经确认精确的数据；
//   - 新记录是 Exact -> 先删掉该线程所有旧的 Derived 记录再写入，
//     确保精确事实出现后，界面上不会再混杂旧的推算值。
pub(crate) async fn insert_usage_batch(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: u64,
    batch: &mut Vec<UsageCall>,
) -> Result<u64, LocalError> {
    if batch.is_empty() {
        return Ok(0);
    }
    let generation = to_sql_u64(generation)?;
    let thread_keys: Vec<String> = batch.iter().map(|call| call.thread_key.clone()).collect();
    let mut exact_threads =
        prefetch_exact_threads(transaction, source_id, generation, &thread_keys).await?;
    let mut added = 0_u64;
    for call in batch.drain(..) {
        if call.confidence == Confidence::Derived {
            if exact_threads.contains(&call.thread_key) {
                continue;
            }
        } else if call.confidence == Confidence::Exact {
            transaction
                .execute(statement(
                    DELETE_DERIVED_CALLS_SQL,
                    vec![
                        source_id.into(),
                        generation.into(),
                        (&call.thread_key).into(),
                    ],
                ))
                .await?;
            exact_threads.insert(call.thread_key.clone());
        }
        let inserted = execute_usage_call(
            transaction,
            INSERT_USAGE_CALL_SQL,
            source_id,
            generation,
            &call,
        )
        .await?;
        if inserted == 0 {
            execute_usage_call(
                transaction,
                UPSERT_USAGE_CALL_SQL,
                source_id,
                generation,
                &call,
            )
            .await?;
        } else {
            added = added.saturating_add(1);
        }
    }
    Ok(added)
}
