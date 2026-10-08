//! Claude transcript 单调 upsert 批次写入。

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DatabaseTransaction};

use crate::UsageCall;

use super::super::LocalError;
use super::ClaudeBatchOutcome;
use super::shared::{
    INSERT_USAGE_CALL_SQL, UPSERT_USAGE_CALL_SQL, execute_usage_call, from_sql_u64,
    in_placeholders, statement, to_sql_u64,
};

/// `(input, cached, cached_available, cache_write, output, total)`，字段顺序
/// 与 [`prefetch_existing_calls`] 的查询列顺序一致。
type ExistingClaudeCall = (u64, u64, bool, Option<u64>, u64, u64);

/// 一次性预取批次内全部 `logical_call_id` 对应的既有行；批内如果同一
/// `logical_call_id` 被后续写入更新，循环体会同步刷新这份内存缓存，
/// 因此和逐行查询数据库的写法在“看见批内更早写入”这件事上完全等价，
/// 只是把最多 N 次往返压成 1 次。
async fn prefetch_existing_calls(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: i64,
    logical_call_ids: &[String],
) -> Result<HashMap<String, ExistingClaudeCall>, LocalError> {
    let mut existing = HashMap::with_capacity(logical_call_ids.len());
    if logical_call_ids.is_empty() {
        return Ok(existing);
    }
    let placeholders = in_placeholders(3, logical_call_ids.len());
    let mut values: Vec<sea_orm::Value> = vec![source_id.into(), generation.into()];
    values.extend(logical_call_ids.iter().map(Into::into));
    let rows = transaction
        .query_all(statement(
            &format!(
                "SELECT logical_call_id, input_tokens, cached_input_tokens, cached_input_available,
                        cache_write_input_tokens, output_tokens, total_tokens
                 FROM usage_calls
                 WHERE source_id = ?1 AND generation = ?2 AND logical_call_id IN ({placeholders})"
            ),
            values,
        ))
        .await?;
    for row in rows {
        let logical_call_id: String = row.try_get_by_index(0)?;
        let state = (
            from_sql_u64(row.try_get_by_index(1)?)?,
            from_sql_u64(row.try_get_by_index(2)?)?,
            row.try_get_by_index::<bool>(3)?,
            row.try_get_by_index::<Option<i64>>(4)?
                .map(from_sql_u64)
                .transpose()?,
            from_sql_u64(row.try_get_by_index(5)?)?,
            from_sql_u64(row.try_get_by_index(6)?)?,
        );
        existing.insert(logical_call_id, state);
    }
    Ok(existing)
}

/// 对 Claude 相同消息的重复观察执行单调 upsert；任一 Token 字段回退都保留旧事实。
// Claude Code 的 transcript 里，同一条消息的用量记录可能被多次写入
// （比如流式生成过程中反复落盘同一条 assistant 消息的最新状态）：
// 后一次观察通常包含更完整的 Token 数据。这个函数对每条候选记录先查
// 数据库里是否已有同一逻辑调用的旧记录：
//   - 没有旧记录 -> 直接插入，计入 added；
//   - 有旧记录 -> 逐字段比较新旧数值，只有当输入/缓存/缓存写入/输出/总量
//     全部“不低于”旧值（单调不减）才允许用新值覆盖旧记录；只要有一个
//     字段变小，就判定这是一次不可信的“倒退”观察，保留旧记录不覆盖，
//     计入 regressed 但不中断整个批次（大概率是乱序或重复写入，
//     而不是数据本身有问题）。
pub(crate) async fn insert_claude_usage_batch(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: u64,
    batch: &mut Vec<UsageCall>,
) -> Result<ClaudeBatchOutcome, LocalError> {
    if batch.is_empty() {
        return Ok(ClaudeBatchOutcome {
            added: 0,
            regressed: 0,
        });
    }
    let generation = to_sql_u64(generation)?;
    let logical_call_ids: Vec<String> = batch
        .iter()
        .map(|call| call.logical_call_id.clone())
        .collect();
    let mut existing_calls =
        prefetch_existing_calls(transaction, source_id, generation, &logical_call_ids).await?;
    let mut added = 0_u64;
    let mut regressed = 0_u64;
    for call in batch.drain(..) {
        let existing = existing_calls.get(&call.logical_call_id).copied();
        if let Some((input, cached, cached_available, cache_write, output, total)) = existing {
            let (Some(next_cached), Some(previous_cache_write), Some(next_cache_write)) = (
                call.usage.cached_input_tokens,
                cache_write,
                call.usage.cache_write_input_tokens,
            ) else {
                regressed = regressed.saturating_add(1);
                continue;
            };
            if !cached_available {
                regressed = regressed.saturating_add(1);
                continue;
            }
            let previous_uncached = input
                .checked_sub(cached)
                .and_then(|value| value.checked_sub(previous_cache_write));
            let next_uncached = call
                .usage
                .input_tokens
                .checked_sub(next_cached)
                .and_then(|value| value.checked_sub(next_cache_write));
            let monotonic = previous_uncached
                .zip(next_uncached)
                .is_some_and(|(previous, next)| next >= previous)
                && call.usage.input_tokens >= input
                && next_cached >= cached
                && next_cache_write >= previous_cache_write
                && call.usage.output_tokens >= output
                && call.usage.total_tokens >= total;
            if !monotonic {
                regressed = regressed.saturating_add(1);
                continue;
            }
            execute_usage_call(
                transaction,
                UPSERT_USAGE_CALL_SQL,
                source_id,
                generation,
                &call,
            )
            .await?;
        } else {
            execute_usage_call(
                transaction,
                INSERT_USAGE_CALL_SQL,
                source_id,
                generation,
                &call,
            )
            .await?;
            added = added.saturating_add(1);
        }
        existing_calls.insert(
            call.logical_call_id.clone(),
            (
                call.usage.input_tokens,
                call.usage.cached_input_tokens.unwrap_or_default(),
                call.usage.cached_input_tokens.is_some(),
                call.usage.cache_write_input_tokens,
                call.usage.output_tokens,
                call.usage.total_tokens,
            ),
        );
    }
    Ok(ClaudeBatchOutcome { added, regressed })
}
