//! 在本机索引的单一快照事务中读取 canonical 调用与安全数据根记录。

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::{
    CanonicalUsageSet, CoverageReport, CoverageState, LocalIndexState, SessionTokenSnapshot, SourceProvenance,
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
    /// 同一事务确认最近扫描成功且启用来源完整，可作为精确用量的读取资格。
    pub read_ready: bool,
    /// 与调用读取处于同一数据库快照的数据根安全记录。
    pub roots: Vec<RootRecord>,
}

impl UsageSnapshot {
    /// 数据库内的不完整事实优先于宿主缓存；失败与取消的更强状态继续保留。
    pub fn effective_coverage(&self, mut report: CoverageReport) -> CoverageReport {
        if !self.read_ready && report.state == CoverageState::Complete {
            report.state = CoverageState::Partial;
        }
        report
    }
}

/// 在一个事务中读取调用与根记录，读取完成后只提交事务边界；所有查询共享
/// 同一数据库快照，避免并发写入让调用记录和数据根信息互相矛盾。
#[cfg(any(test, feature = "test-support"))]
pub(crate) async fn load_usage_snapshot(
    connection: &sea_orm::DatabaseConnection,
    parser_version: u32,
) -> Result<UsageSnapshot, LocalError> {
    let transaction = connection.begin().await?;
    let canonical = load_canonical_calls(&transaction, parser_version).await?;
    let index_state =
        load_local_index_state(&transaction, canonical.calls.len(), parser_version).await?;
    let roots = load_root_records(&transaction, parser_version).await?;
    let read_ready = load_read_ready(&transaction, index_state, &roots, parser_version).await?;
    transaction.commit().await?;
    Ok(UsageSnapshot {
        canonical,
        index_state,
        read_ready,
        roots,
    })
}

/// 只读取给定发生时间下界后的 canonical 调用，根和索引状态仍来自同一事务。
#[cfg(any(test, feature = "test-support"))]
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
    let read_ready = load_read_ready(&transaction, index_state, &roots, parser_version).await?;
    transaction.commit().await?;
    Ok(UsageSnapshot {
        canonical,
        index_state,
        read_ready,
        roots,
    })
}

/// 只读取当前生产视图真正消费的 canonical 调用、索引状态与根记录。
///
/// 历史 cumulative token 快照仅用于兼容旧索引，不参与当前概览、统计、
/// 图表或调用列表聚合；GUI 视图不得为这些未消费记录支付完整实体化成本。
pub(crate) async fn load_usage_view_snapshot_since(
    connection: &sea_orm::DatabaseConnection,
    parser_version: u32,
    cutoff_epoch_ms: i64,
) -> Result<UsageSnapshot, LocalError> {
    let transaction = connection.begin().await?;
    let canonical =
        load_canonical_usage_calls_since(&transaction, parser_version, cutoff_epoch_ms).await?;
    let index_state =
        load_local_index_state(&transaction, canonical.calls.len(), parser_version).await?;
    let roots = load_root_records(&transaction, parser_version).await?;
    let read_ready = load_read_ready(&transaction, index_state, &roots, parser_version).await?;
    transaction.commit().await?;
    Ok(UsageSnapshot {
        canonical,
        index_state,
        read_ready,
        roots,
    })
}

/// 最近一次扫描和全部启用来源必须共同证明完整，旧成功记录不能覆盖新失败。
async fn load_read_ready<C: ConnectionTrait>(
    connection: &C,
    index_state: LocalIndexState,
    roots: &[RootRecord],
    parser_version: u32,
) -> Result<bool, LocalError> {
    if !matches!(index_state, LocalIndexState::Ready | LocalIndexState::ReadyNoCalls)
        || !roots.iter().any(|root| root.enabled)
        || roots.iter().any(|root| root.enabled && root.last_coverage != Some(CoverageState::Complete))
    {
        return Ok(false);
    }
    let Some(row) = connection.query_one_raw(statement(
        "SELECT status, cancelled, finished_at_epoch_ms, warning_count,
                EXISTS(SELECT 1 FROM source_files f
                       JOIN source_roots r ON r.root_id = f.root_id
                       WHERE r.enabled = 1
                         AND (f.ready <> 1 OR f.token_snapshots_ready <> 1
                              OR f.parser_version <> ?1
                              OR f.parsed_offset > f.observed_size
                              OR f.trailing_bytes <> f.observed_size - f.parsed_offset))
         FROM scan_runs ORDER BY row_id DESC LIMIT 1",
        vec![i64::from(parser_version).into()],
    )).await? else {
        return Ok(false);
    };
    Ok(row.try_get_by_index::<String>(0)? == "completed"
        && !row.try_get_by_index::<bool>(1)?
        && row.try_get_by_index::<Option<i64>>(2)?.is_some()
        && from_sql_u64(row.try_get_by_index(3)?)? == 0
        && !row.try_get_by_index::<bool>(4)?)
}

/// 从当前数据库快照推导索引四态，旧 parser 来源优先暴露为需要重扫。
pub(super) async fn load_local_index_state<C: ConnectionTrait>(
    connection: &C,
    canonical_call_count: usize,
    parser_version: u32,
) -> Result<LocalIndexState, LocalError> {
    let row = connection
        .query_one_raw(statement(
            "SELECT
               (SELECT COUNT(*) FROM scan_runs WHERE status = 'completed'),
               (SELECT COUNT(*) FROM source_files f
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version = ?1 AND r.enabled = 1),
               (SELECT COUNT(*) FROM source_files f
                JOIN source_roots r ON r.root_id = f.root_id
                WHERE f.ready = 1 AND f.parser_version <> ?1 AND r.enabled = 1),
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
/// 对应 core 里 `LocalIndexState` 的四态，按优先级从高到低依次判断
/// （一旦命中某个条件立刻返回，不再看后面的）：
///   1. 没有启用的数据根 -> 肯定是 NotScanned（连范围都没有，谈不上扫描）；
///   2. 启用根上仍有旧 parser generation 来源 -> NeedsRescan；同一根中的
///      新版来源不能掩盖窗口外尚未重建的旧来源；
///   3. 当前 generation 已经有 canonical 调用 -> Ready（正常可用状态）；
///   4. 还有启用但从未扫描过的根，或者压根没有任何完成过的扫描记录
///      也没有任何来源文件 -> NotScanned；
///   5. 排除以上情况，说明确实扫描过、数据也是最新版本，只是恰好没有
///      调用记录 -> ReadyNoCalls（区别于 NotScanned，这是“扫描确认过、
///      结果真的是空”，界面可以放心显示零用量而不是提示去扫描）。
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
        .query_all_raw(statement(
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
        .query_all_raw(statement(
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
        .query_one_raw(statement(
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - snapshot_usage_reads.rs：索引就绪判定、当前 generation 调用读取与有界时间范围查询。
include!("snapshot_usage_reads.rs");
