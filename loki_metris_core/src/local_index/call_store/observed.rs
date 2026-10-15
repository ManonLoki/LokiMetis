//! 保存身份稳定、不可覆盖的单响应账单事实，不替换历史累计回退。

use sea_orm::{ConnectionTrait, DatabaseTransaction};

use super::super::{LocalError, LocalErrorKind};
use super::shared::{INSERT_USAGE_CALL_SQL, execute_usage_call, statement, to_sql_u64};
use crate::{Confidence, UsageCall};

/// 插入已具备稳定响应身份的事实；同值重放幂等，矛盾值使整个批次失败。
/// 不按线程删除 Derived：旧协议前缀与现代响应属于不同真实调用。
pub(crate) async fn insert_observed_usage_batch(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: u64,
    ingest_since_epoch_ms: i64,
    batch: &mut Vec<UsageCall>,
) -> Result<u64, LocalError> {
    let generation = to_sql_u64(generation)?;
    // 重建只在新代完整提交后替换旧代；此处仍须尊重已经公开的响应事实。
    let published = transaction
        .query_one_raw(statement(
            "SELECT generation FROM source_files WHERE source_id = ?1",
            vec![source_id.into()],
        ))
        .await?
        .map(|row| row.try_get_by_index::<i64>(0))
        .transpose()?;
    let mut added = 0_u64;
    for mut call in batch.drain(..) {
        if call.confidence != Confidence::Exact {
            return Err(LocalError::new(
                LocalErrorKind::InvalidUsage,
                "observed response must be exact",
            ));
        }
        let current_time = verify_observed_bill(transaction, source_id, generation, &call).await?;
        let published_time = if let Some(published) = published.filter(|old| *old != generation) {
            verify_observed_bill(transaction, source_id, published, &call).await?
        } else {
            None
        };
        // 先恢复已公开的时间再判断保留期；较早或较晚的重放都不能改变归日与保留期。
        call.occurred_at_epoch_ms = published_time
            .or(current_time)
            .unwrap_or(call.occurred_at_epoch_ms);
        if call.occurred_at_epoch_ms < ingest_since_epoch_ms {
            continue;
        }
        let inserted = execute_usage_call(
            transaction,
            INSERT_USAGE_CALL_SQL,
            source_id,
            generation,
            &call,
        )
        .await?;
        added = added.saturating_add(inserted);
    }
    Ok(added)
}

/// 按主键校验完整账单与可用性；矛盾返回稳定错误，同值返回首次时间，缺失返回 None。
/// 展示标签与检查点可变化，查询不按来源扫描全部调用。
async fn verify_observed_bill(
    transaction: &DatabaseTransaction,
    source_id: &str,
    generation: i64,
    call: &UsageCall,
) -> Result<Option<i64>, LocalError> {
    // 查询本事务最新行，覆盖同批与跨批冲突；并发写入失败由外层事务整体回滚。
    // IS 同时比较 NULL 与值，None 不能被 Some(0) 静默替换。
    let row = transaction
        .query_one_raw(statement(
            "SELECT occurred_at_epoch_ms, NOT (thread_key = ?4 AND input_tokens = ?5
         AND cached_input_tokens = ?6 AND cache_write_input_tokens IS ?7
         AND output_tokens = ?8 AND reasoning_output_tokens = ?9 AND total_tokens = ?10
         AND total_is_derived = ?11 AND cached_input_available = ?12
         AND reasoning_output_available = ?13 AND confidence = 'exact')
         FROM usage_calls WHERE source_id = ?1 AND generation = ?2 AND logical_call_id = ?3",
            vec![
                source_id.into(),
                generation.into(),
                (&call.logical_call_id).into(),
                (&call.thread_key).into(),
                to_sql_u64(call.usage.input_tokens)?.into(),
                to_sql_u64(call.usage.cached_input_tokens.unwrap_or_default())?.into(),
                call.usage
                    .cache_write_input_tokens
                    .map(to_sql_u64)
                    .transpose()?
                    .into(),
                to_sql_u64(call.usage.output_tokens)?.into(),
                to_sql_u64(call.usage.reasoning_output_tokens.unwrap_or_default())?.into(),
                to_sql_u64(call.usage.total_tokens)?.into(),
                call.usage.total_is_derived.into(),
                call.usage.cached_input_tokens.is_some().into(),
                call.usage.reasoning_output_tokens.is_some().into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get_by_index::<i64>(1)? != 0 {
        return Err(LocalError::new(
            LocalErrorKind::InvalidUsage,
            "observed response identity has conflicting usage",
        ));
    }
    Ok(Some(row.try_get_by_index(0)?))
}
