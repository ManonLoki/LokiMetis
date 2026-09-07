//! 在本机索引的单一快照事务中读取 canonical 调用与安全数据根记录。

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::{
    CanonicalUsageSet, LocalIndexState, SessionTokenSnapshot, SourceProvenance,
    attach_session_snapshots, canonicalize_usage_calls,
};

use super::call_store::{from_sql_u64, row_to_usage_call};
use super::source_registry::{
    DiscoveryMethod, RootRecord, RootUsageSummaryRecord, coverage_state_from_label,
};
use super::{LocalError, LocalErrorKind};

/// 为会话快照查询构造带绑定值的 SQLite 语句。
fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// 保存同一个只读快照事务中取得的 canonical 调用与安全数据根记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSnapshot {
    /// 当前 generation 的调用观察经 core 去重后的集合。
    pub canonical: CanonicalUsageSet,
    /// 从同一事务中的扫描与 parser 来源事实推导出的本机索引状态。
    pub index_state: LocalIndexState,
    /// 与调用读取处于同一数据库快照的数据根安全记录。
    pub roots: Vec<RootRecord>,
}

/// 在一个事务中读取调用与根记录，读取完成后只提交事务边界；三次查询共享
/// 同一数据库快照，避免并发写入让调用记录和数据根信息互相矛盾。
pub(crate) async fn load_usage_snapshot(
    connection: &sea_orm::DatabaseConnection,
    parser_version: u32,
) -> Result<UsageSnapshot, LocalError> {
    let transaction = connection.begin().await?;
    let canonical = load_canonical_calls(&transaction, parser_version).await?;
    let index_state =
        load_local_index_state(&transaction, canonical.calls.len(), parser_version).await?;
    let roots = load_root_records(&transaction, parser_version).await?;
    transaction.commit().await?;
    Ok(UsageSnapshot {
        canonical,
        index_state,
        roots,
    })
}

/// 只读取给定发生时间下界后的 canonical 调用，根和索引状态仍来自同一事务。
pub(crate) async fn load_usage_snapshot_since(
    connection: &sea_orm::DatabaseConnection,
    parser_version: u32,
    cutoff_epoch_ms: i64,
) -> Result<UsageSnapshot, LocalError> {
    let transaction = connection.begin().await?;
    let canonical =
        load_canonical_calls_since(&transaction, parser_version, cutoff_epoch_ms).await?;
    let index_state =
        load_local_index_state(&transaction, canonical.calls.len(), parser_version).await?;
    let roots = load_root_records(&transaction, parser_version).await?;
    transaction.commit().await?;
    Ok(UsageSnapshot {
        canonical,
        index_state,
        roots,
    })
}

/// 从当前数据库快照推导索引四态，旧 parser 来源优先暴露为需要重扫。
pub(super) async fn load_local_index_state<C: ConnectionTrait>(
    connection: &C,
    canonical_call_count: usize,
    parser_version: u32,
) -> Result<LocalIndexState, LocalError> {
    let row = connection
        .query_one(statement(
            "SELECT
               (SELECT COUNT(*) FROM scan_runs WHERE status = 'completed'),
               (SELECT COUNT(*) FROM source_files f
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version = ?1 AND r.enabled = 1),
               (SELECT COUNT(*) FROM source_files f
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version <> ?1 AND r.enabled = 1
                  AND NOT EXISTS (
                    SELECT 1 FROM source_files c
                    WHERE c.root_id = f.root_id
                      AND c.ready = 1
                      AND c.parser_version = ?1
                  )),
               (SELECT COUNT(*) FROM source_roots r
                WHERE r.enabled = 1
                  AND (r.last_coverage_state IS NULL
                       OR r.last_coverage_state IN ('partial', 'cancelled', 'failed'))),
               (SELECT COUNT(*) FROM source_roots WHERE enabled = 1)",
            vec![i64::from(parser_version).into()],
        ))
        .await?
        .ok_or_else(|| {
            LocalError::new(
                LocalErrorKind::Database,
                "index state summary is unreadable",
            )
        })?;
    let completed_scan_count = from_sql_u64(row.try_get_by_index(0)?)?;
    let current_source_count = from_sql_u64(row.try_get_by_index(1)?)?;
    let stale_source_count = from_sql_u64(row.try_get_by_index(2)?)?;
    let enabled_unscanned_root_count = from_sql_u64(row.try_get_by_index(3)?)?;
    let enabled_root_count = from_sql_u64(row.try_get_by_index(4)?)?;

    Ok(determine_local_index_state(
        completed_scan_count,
        current_source_count,
        stale_source_count,
        enabled_unscanned_root_count,
        enabled_root_count,
        canonical_call_count,
    ))
}

/// 按覆盖优先级把计数事实归一为稳定状态，供数据库 fixture 锁定边界。
// 对应 core 里 `LocalIndexState` 的四态，按优先级从高到低依次判断
// （一旦命中某个条件立刻返回，不再看后面的）：
//   1. 没有启用的数据根 -> 肯定是 NotScanned（连范围都没有，谈不上扫描）；
//   2. 启用根上仍有旧 parser generation 来源、且该根还没有任何当前
//      generation 来源 -> NeedsRescan。保留窗口外因 mtime 跳过而未重开的
//      旧行，只要同一根已经完成当前 parser 重建，就不得继续挡住概览；
//      当前读取仍只取当前 generation，不会把旧调用混进总量；
//   3. 当前 generation 已经有 canonical 调用 -> Ready（正常可用状态）；
//   4. 还有启用但从未扫描过的根，或者压根没有任何完成过的扫描记录
//      也没有任何来源文件 -> NotScanned；
//   5. 排除以上情况，说明确实扫描过、数据也是最新版本，只是恰好没有
//      调用记录 -> ReadyNoCalls（区别于 NotScanned，这是“扫描确认过、
//      结果真的是空”，前端可以放心显示零用量而不是提示去扫描）。
fn determine_local_index_state(
    completed_scan_count: u64,
    current_source_count: u64,
    stale_source_count: u64,
    enabled_unscanned_root_count: u64,
    enabled_root_count: u64,
    canonical_call_count: usize,
) -> LocalIndexState {
    if enabled_root_count == 0 {
        LocalIndexState::NotScanned
    } else if stale_source_count > 0 {
        LocalIndexState::NeedsRescan
    } else if canonical_call_count > 0 {
        LocalIndexState::Ready
    } else if enabled_unscanned_root_count > 0
        || (completed_scan_count == 0 && current_source_count == 0)
    {
        LocalIndexState::NotScanned
    } else {
        LocalIndexState::ReadyNoCalls
    }
}

/// 从指定数据库快照读取不含绝对路径的数据根记录。
pub(crate) async fn load_root_records<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<Vec<RootRecord>, LocalError> {
    let rows = connection
        .query_all(statement(
            "SELECT r.root_id, r.alias, r.enabled, r.is_primary, r.discovery_method,
                    r.activation_state, r.last_coverage_state,
                    (SELECT COUNT(*) FROM source_files f
                     WHERE f.root_id = r.root_id AND f.ready = 1
                       AND f.parser_version = ?1),
                    (SELECT COUNT(*) FROM usage_calls u
                     JOIN source_files f ON f.source_id = u.source_id
                     WHERE f.root_id = r.root_id AND f.ready = 1
                       AND f.parser_version = ?1
                       AND f.generation = u.generation)
             FROM source_roots r
             ORDER BY r.alias, r.root_id",
            vec![i64::from(parser_version).into()],
        ))
        .await?;
    rows.iter().map(root_record_from_row).collect()
}

/// 只为来源根摘要读取 registry 与按根 canonical 计数，避免完整快照重复聚合。
pub(crate) async fn load_root_usage_summary_records<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<Vec<RootUsageSummaryRecord>, LocalError> {
    validate_current_usage_integrity(connection, parser_version).await?;
    let rows = connection
        .query_all(statement(
            "WITH current_sources AS (
               SELECT source_id, root_id, generation
               FROM source_files
               WHERE ready = 1 AND parser_version = ?1
             ),
             source_totals AS (
               SELECT root_id, COUNT(*) AS source_file_count
               FROM current_sources
               GROUP BY root_id
             ),
             usage_totals AS (
               SELECT f.root_id,
                      COUNT(*) AS call_observation_count,
                      COUNT(DISTINCT u.logical_call_id) AS canonical_call_count
               FROM current_sources f
               JOIN usage_calls u ON u.source_id = f.source_id
                                 AND u.generation = f.generation
               GROUP BY f.root_id
             )
             SELECT r.root_id, r.alias, r.enabled, r.is_primary, r.discovery_method,
                    r.activation_state, r.last_coverage_state,
                    COALESCE(s.source_file_count, 0),
                    COALESCE(u.call_observation_count, 0),
                    CASE WHEN r.enabled = 1
                         THEN COALESCE(u.canonical_call_count, 0)
                         ELSE 0 END
             FROM source_roots r
             LEFT JOIN source_totals s ON s.root_id = r.root_id
             LEFT JOIN usage_totals u ON u.root_id = r.root_id
             ORDER BY r.alias, r.root_id",
            vec![i64::from(parser_version).into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(RootUsageSummaryRecord {
                root: root_record_from_row(row)?,
                canonical_call_count: from_sql_u64(row.try_get_by_index(9)?)?,
            })
        })
        .collect()
}

/// 解码两种根查询共享的 registry 与观察计数字段。
fn root_record_from_row(row: &sea_orm::QueryResult) -> Result<RootRecord, LocalError> {
    let method_label: String = row.try_get_by_index(4)?;
    let activation_label: String = row.try_get_by_index(5)?;
    let coverage_label: Option<String> = row.try_get_by_index(6)?;
    let discovery_method = DiscoveryMethod::from_label(&method_label).ok_or_else(|| {
        LocalError::new(LocalErrorKind::Database, "root discovery method is invalid")
    })?;
    let last_coverage = coverage_label
        .as_deref()
        .and_then(coverage_state_from_label);
    Ok(RootRecord {
        root_id: row.try_get_by_index(0)?,
        alias: row.try_get_by_index(1)?,
        enabled: row.try_get_by_index(2)?,
        activation_state: crate::RootActivationState::from_label(&activation_label).ok_or_else(
            || LocalError::new(LocalErrorKind::Database, "root activation state is invalid"),
        )?,
        is_primary: row.try_get_by_index(3)?,
        discovery_method,
        last_coverage,
        source_file_count: from_sql_u64(row.try_get_by_index(7)?)?,
        call_observation_count: from_sql_u64(row.try_get_by_index(8)?)?,
    })
}

/// 只用 SQLite 去重计数读取当前启用根的 canonical 调用数，不解码调用或快照。
pub(super) async fn load_canonical_call_count<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<u64, LocalError> {
    validate_current_usage_integrity(connection, parser_version).await?;
    let row = connection
        .query_one(statement(
            "SELECT COUNT(DISTINCT u.logical_call_id)
             FROM usage_calls u
             JOIN source_files f ON f.source_id = u.source_id
             JOIN source_roots r ON r.root_id = f.root_id
             WHERE f.ready = 1 AND f.parser_version = ?1
               AND r.enabled = 1
               AND f.generation = u.generation",
            vec![i64::from(parser_version).into()],
        ))
        .await?
        .ok_or_else(|| {
            LocalError::new(
                LocalErrorKind::Database,
                "canonical call count is unreadable",
            )
        })?;
    from_sql_u64(row.try_get_by_index(0)?)
}

/// 用有界聚合与 EXISTS 查询保持已覆盖持久化不变量的 fail-closed 边界，
/// 不把调用或快照实体化。
async fn validate_current_usage_integrity<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<(), LocalError> {
    let row = connection
        .query_one(statement(
            "SELECT
               (SELECT COALESCE(MAX(
                  CASE
                    WHEN (
                      typeof(u.logical_call_id) <> 'text'
                      OR typeof(u.occurred_at_epoch_ms) <> 'integer'
                      OR (u.model IS NOT NULL AND typeof(u.model) <> 'text')
                      OR (u.reasoning_effort IS NOT NULL
                          AND typeof(u.reasoning_effort) <> 'text')
                      OR (u.project_key IS NOT NULL AND typeof(u.project_key) <> 'text')
                      OR typeof(u.thread_key) <> 'text'
                      OR (u.project_label IS NOT NULL
                          AND typeof(u.project_label) <> 'text')
                      OR (u.thread_label IS NOT NULL
                          AND typeof(u.thread_label) <> 'text')
                      OR typeof(u.input_tokens) <> 'integer' OR u.input_tokens < 0
                      OR typeof(u.cached_input_tokens) <> 'integer'
                         OR u.cached_input_tokens < 0
                      OR (u.cache_write_input_tokens IS NOT NULL
                          AND (typeof(u.cache_write_input_tokens) <> 'integer'
                               OR u.cache_write_input_tokens < 0))
                      OR typeof(u.output_tokens) <> 'integer' OR u.output_tokens < 0
                      OR typeof(u.reasoning_output_tokens) <> 'integer'
                         OR u.reasoning_output_tokens < 0
                      OR typeof(u.total_tokens) <> 'integer' OR u.total_tokens < 0
                      OR typeof(u.total_is_derived) <> 'integer'
                      OR typeof(u.confidence) <> 'text'
                         OR u.confidence NOT IN ('exact', 'derived', 'suspected')
                      OR typeof(u.cached_input_available) <> 'integer'
                      OR typeof(u.reasoning_output_available) <> 'integer'
                      OR (u.adapter_consistency_key IS NOT NULL
                          AND typeof(u.adapter_consistency_key) <> 'text')
                      OR typeof(f.source_id) <> 'text'
                      OR typeof(f.root_id) <> 'text'
                      OR typeof(f.relative_label) <> 'text'
                      OR typeof(f.archived) <> 'integer'
                    ) THEN 2
                    WHEN (
                      (u.cached_input_available <> 0
                       AND u.cached_input_tokens > u.input_tokens)
                      OR (u.reasoning_output_available <> 0
                          AND u.reasoning_output_tokens > u.output_tokens)
                      OR (u.total_is_derived = 0
                          AND (u.total_tokens < u.input_tokens
                               OR u.total_tokens - u.input_tokens < u.output_tokens))
                    ) THEN 1
                    ELSE 0
                  END
                ), 0)
                FROM usage_calls u
                JOIN source_files f ON f.source_id = u.source_id
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version = ?1
                  AND r.enabled = 1 AND f.generation = u.generation),
               EXISTS(
                 SELECT 1 FROM usage_token_snapshots s
                 JOIN source_files f ON f.source_id = s.source_id
                 JOIN source_roots r ON r.root_id = f.root_id
                 WHERE f.ready = 1 AND f.parser_version = ?1
                   AND r.enabled = 1 AND f.generation = s.generation
                   AND (
                     typeof(s.thread_key) <> 'text'
                     OR typeof(s.occurred_at_epoch_ms) <> 'integer'
                     OR typeof(s.total_tokens) <> 'integer' OR s.total_tokens < 0
                     OR typeof(s.logical_call_id) <> 'text'
                     OR (s.model IS NOT NULL AND typeof(s.model) <> 'text')
                     OR (s.reasoning_effort IS NOT NULL
                         AND typeof(s.reasoning_effort) <> 'text')
                     OR (s.project_key IS NOT NULL AND typeof(s.project_key) <> 'text')
                     OR typeof(f.source_id) <> 'text'
                     OR typeof(f.root_id) <> 'text'
                     OR typeof(f.relative_label) <> 'text'
                     OR typeof(f.archived) <> 'integer'
                   )
               )",
            vec![i64::from(parser_version).into()],
        ))
        .await?
        .ok_or_else(|| {
            LocalError::new(LocalErrorKind::Database, "usage integrity is unreadable")
        })?;
    let call_integrity_kind: i64 = row.try_get_by_index(0)?;
    let malformed_snapshot: bool = row.try_get_by_index(1)?;
    if call_integrity_kind == 2 || malformed_snapshot {
        return Err(LocalError::new(
            LocalErrorKind::Database,
            "stored usage row is invalid",
        ));
    }
    if call_integrity_kind == 1 {
        return Err(LocalError::new(
            LocalErrorKind::InvalidUsage,
            "local token usage is invalid",
        ));
    }
    if call_integrity_kind != 0 {
        return Err(LocalError::new(
            LocalErrorKind::Database,
            "usage integrity result is invalid",
        ));
    }
    Ok(())
}

/// 从指定数据库快照读取当前 generation 调用并由 core 执行 canonical 去重。
pub(crate) async fn load_canonical_calls<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
) -> Result<CanonicalUsageSet, LocalError> {
    load_canonical_calls_since(connection, parser_version, i64::MIN).await
}

/// 从指定时间下界读取当前 generation 调用，避免视图装载窗口外全历史。
pub(crate) async fn load_canonical_calls_since<C: ConnectionTrait>(
    connection: &C,
    parser_version: u32,
    cutoff_epoch_ms: i64,
) -> Result<CanonicalUsageSet, LocalError> {
    let rows = connection
        .query_all(statement(
            "SELECT u.logical_call_id, u.occurred_at_epoch_ms, u.model,
                    u.reasoning_effort, u.project_key, u.thread_key,
                    u.project_label, u.thread_label,
                    u.input_tokens, u.cached_input_tokens, u.cache_write_input_tokens,
                    u.output_tokens, u.reasoning_output_tokens, u.total_tokens,
                    u.total_is_derived, u.confidence,
                    u.cached_input_available, u.reasoning_output_available,
                    u.adapter_consistency_key,
                    f.source_id, f.root_id, f.relative_label, f.archived
             FROM usage_calls u
             JOIN source_files f ON f.source_id = u.source_id
             JOIN source_roots r ON r.root_id = f.root_id
             WHERE f.ready = 1 AND f.parser_version = ?1
               AND r.enabled = 1
               AND f.generation = u.generation
               AND u.occurred_at_epoch_ms >= ?2
             ORDER BY u.occurred_at_epoch_ms, u.logical_call_id, f.root_id, f.source_id",
            vec![i64::from(parser_version).into(), cutoff_epoch_ms.into()],
        ))
        .await?;
    let mut calls = Vec::with_capacity(rows.len());
    for row in &rows {
        calls.push(row_to_usage_call(row)?);
    }
    let snapshot_rows = connection
        .query_all(statement(
            "SELECT s.thread_key, s.occurred_at_epoch_ms, s.total_tokens, s.logical_call_id,
                    s.model, s.reasoning_effort, s.project_key,
                    f.source_id, f.root_id, f.relative_label, f.archived
             FROM usage_token_snapshots s
             JOIN source_files f ON f.source_id = s.source_id
             JOIN source_roots r ON r.root_id = f.root_id
             WHERE f.ready = 1 AND f.parser_version = ?1
               AND r.enabled = 1
               AND f.generation = s.generation
               AND s.occurred_at_epoch_ms >= ?2
             ORDER BY s.occurred_at_epoch_ms, s.logical_call_id, s.thread_key",
            vec![i64::from(parser_version).into(), cutoff_epoch_ms.into()],
        ))
        .await?;
    let mut snapshots = Vec::with_capacity(snapshot_rows.len());
    for row in &snapshot_rows {
        snapshots.push(row_to_session_snapshot(row)?);
    }
    Ok(attach_session_snapshots(
        canonicalize_usage_calls(calls),
        snapshots,
    ))
}

/// 将数据库行严格解码为会话 Token 快照，拒绝越界或损坏数值。
fn row_to_session_snapshot(row: &sea_orm::QueryResult) -> Result<SessionTokenSnapshot, LocalError> {
    Ok(SessionTokenSnapshot {
        thread_key: row.try_get_by_index(0)?,
        occurred_at_epoch_ms: row.try_get_by_index(1)?,
        cumulative_total_tokens: from_sql_u64(row.try_get_by_index(2)?)?,
        logical_call_id: row.try_get_by_index(3)?,
        model: row.try_get_by_index(4)?,
        reasoning_effort: row.try_get_by_index(5)?,
        project_key: row.try_get_by_index(6)?,
        provenance: vec![SourceProvenance {
            source_id: row.try_get_by_index(7)?,
            root_id: row.try_get_by_index(8)?,
            relative_label: row.try_get_by_index(9)?,
            archived: row.try_get_by_index(10)?,
        }],
    })
}
