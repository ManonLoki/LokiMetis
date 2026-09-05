//! canonical 调用聚合、扫描运行记录与规范化 provider 快照的持久化。

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use crate::{
    CanonicalUsageSet, LocalIndexState, LocalUsageAggregate, ProviderKind,
    aggregate_canonical_usage,
};

use super::call_store::{from_sql_u64, to_sql_u64};
use super::id_codec::stable_id;
use super::snapshot::{
    UsageSnapshot, load_canonical_calls, load_usage_snapshot, load_usage_snapshot_since,
};
use super::{LocalError, LocalErrorKind, LocalIndex};

/// 为快照维护操作构造带绑定值的 SQLite 语句。
fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// 本机规范化快照在 `provider_snapshots` 中的稳定类别标签。
const LOCAL_METRIC_SNAPSHOT_KIND: &str = "local_metric";

impl LocalIndex {
    /// 返回当前 parser generation 是否已经存在可展示的派生用量。
    pub async fn has_current_parser_usage(&self) -> Result<bool, LocalError> {
        let row = self
            .connection
            .query_one(statement(
                "SELECT
                   EXISTS(
                     SELECT 1 FROM usage_calls u
                     JOIN source_files f ON f.source_id = u.source_id
                     WHERE f.parser_version = ?1 AND f.ready = 1
                       AND f.generation = u.generation
                   )
                   OR EXISTS(
                     SELECT 1 FROM usage_token_snapshots s
                     JOIN source_files f ON f.source_id = s.source_id
                     WHERE f.parser_version = ?1 AND f.ready = 1
                       AND f.generation = s.generation
                   )",
                vec![i64::from(self.parser_version).into()],
            ))
            .await?
            .ok_or_else(|| {
                LocalError::new(LocalErrorKind::Database, "usage state is unreadable")
            })?;
        Ok(row.try_get_by_index::<i64>(0)? != 0)
    }

    /// 只用有界计数与 EXISTS 查询读取索引四态，不装载全部 canonical 调用。
    pub async fn index_state(&self) -> Result<LocalIndexState, LocalError> {
        let canonical_call_count = usize::from(self.has_current_parser_usage().await?);
        super::snapshot::load_local_index_state(
            &self.connection,
            canonical_call_count,
            self.parser_version,
        )
        .await
    }

    /// 删除保留窗口以前的派生调用与累计快照，保留根和来源 checkpoint。
    pub async fn prune_usage_before(&mut self, cutoff_epoch_ms: i64) -> Result<(), LocalError> {
        let transaction = self.connection.begin().await?;
        transaction
            .execute(statement(
                "DELETE FROM usage_calls WHERE occurred_at_epoch_ms < ?1",
                vec![cutoff_epoch_ms.into()],
            ))
            .await?;
        transaction
            .execute(statement(
                "DELETE FROM usage_token_snapshots WHERE occurred_at_epoch_ms < ?1",
                vec![cutoff_epoch_ms.into()],
            ))
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 读取所有当前 generation 的观察并由 core 执行 canonical 去重。
    pub async fn canonical_calls(&self) -> Result<CanonicalUsageSet, LocalError> {
        load_canonical_calls(&self.connection, self.parser_version).await
    }

    /// 返回当前缓存的规范化 provider 快照数量；供 adapter 诊断与测试观察
    /// 主根切换、停用等操作是否正确使旧快照失效。
    pub async fn provider_snapshot_count(&self) -> Result<u64, LocalError> {
        let row = self
            .connection
            .query_one(statement("SELECT COUNT(*) FROM provider_snapshots", vec![]))
            .await?
            .ok_or_else(|| {
                LocalError::new(LocalErrorKind::Database, "snapshot count is unreadable")
            })?;
        from_sql_u64(row.try_get_by_index(0)?)
    }

    /// 在一个只读快照事务中取得统计所需调用与根记录，不触发扫描或写入。
    pub async fn usage_snapshot(&mut self) -> Result<UsageSnapshot, LocalError> {
        load_usage_snapshot(&self.connection, self.parser_version).await
    }

    /// 在一个事务中只装载给定时间下界后的调用，供 30 日视图读取使用。
    pub async fn usage_snapshot_since(
        &mut self,
        cutoff_epoch_ms: i64,
    ) -> Result<UsageSnapshot, LocalError> {
        load_usage_snapshot_since(&self.connection, self.parser_version, cutoff_epoch_ms).await
    }

    /// 返回全部 canonical 本机调用的聚合。
    pub async fn aggregate(&self) -> Result<LocalUsageAggregate, LocalError> {
        let canonical = self.canonical_calls().await?;
        aggregate_canonical_usage(&canonical).map_err(LocalError::from)
    }

    /// 按明确本机 provider 的业务口径聚合当前 generation，供扫描摘要复用。
    pub async fn aggregate_for_provider(
        &self,
        provider: ProviderKind,
    ) -> Result<LocalUsageAggregate, LocalError> {
        let canonical = self
            .canonical_calls()
            .await?
            .with_total_token_accounting(provider.local_total_token_accounting());
        aggregate_canonical_usage(&canonical).map_err(LocalError::from)
    }

    /// 写入一条不含账户身份的规范化本机快照 JSON，供清理/主根切换观察表生命周期。
    pub async fn save_normalized_snapshot(
        &mut self,
        observed_at_epoch_ms: i64,
        valid_until_epoch_ms: i64,
        normalized_json: &str,
    ) -> Result<(), LocalError> {
        self.save_provider_snapshot(
            LOCAL_METRIC_SNAPSHOT_KIND,
            observed_at_epoch_ms,
            valid_until_epoch_ms,
            normalized_json.to_owned(),
        )
        .await
    }

    /// 清空本产品派生调用、checkpoint、扫描与 provider 快照，保留用户根登记。
    // “清空索引”只删除本产品从原始文件派生出来的数据（调用记录、文件
    // checkpoint、扫描历史、缓存的 provider 快照），`source_roots` 表本身
    // （用户登记了哪些数据根、启用状态、别名）完全不动——对应产品语义
    // “清空索引不等于清空数据源设置”，用户下次扫描会重新从头解析原始
    // 文件，但不需要重新登记数据根。
    pub async fn clear_index(&mut self) -> Result<(), LocalError> {
        let transaction = self.connection.begin().await?;
        transaction
            .execute(statement("DELETE FROM usage_calls", vec![]))
            .await?;
        transaction
            .execute(statement("DELETE FROM source_files", vec![]))
            .await?;
        transaction
            .execute(statement("DELETE FROM scan_runs", vec![]))
            .await?;
        transaction
            .execute(statement("DELETE FROM provider_snapshots", vec![]))
            .await?;
        transaction
            .execute(statement(
                "UPDATE source_roots SET last_coverage_state = NULL",
                vec![],
            ))
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 创建一条扫描运行记录并返回稳定扫描 ID。
    // 先插入一行 `scan_id = NULL` 再回填，是因为 scan_id 的生成依赖
    // SQLite 自动分配的自增行 ID——插入前还不知道这一行会拿到哪个
    // 行 ID，所以只能“先插入占位、拿到自增 ID 后再用它计算出确定性
    // scan_id、回写”，这是利用数据库自增主键生成稳定业务 ID 的常见两步写法。
    pub async fn begin_scan(
        &mut self,
        mode: &str,
        started_at_epoch_ms: i64,
    ) -> Result<String, LocalError> {
        let result = self
            .connection
            .execute(statement(
                "INSERT INTO scan_runs
                 (scan_id, mode, started_at_epoch_ms, status, cancelled,
                  files_scanned, calls_added, warning_count)
                 VALUES (NULL, ?1, ?2, 'running', 0, 0, 0, 0)",
                vec![mode.into(), started_at_epoch_ms.into()],
            ))
            .await?;
        let row_id = to_sql_u64(result.last_insert_id())?;
        let scan_id = stable_id("scan", &format!("{started_at_epoch_ms}\u{0}{row_id}"));
        self.connection
            .execute(statement(
                "UPDATE scan_runs SET scan_id = ?2 WHERE row_id = ?1",
                vec![row_id.into(), (&scan_id).into()],
            ))
            .await?;
        Ok(scan_id)
    }

    /// 完成扫描运行记录，所有计数都来自脱敏摘要。
    #[allow(clippy::too_many_arguments)]
    pub async fn finish_scan(
        &mut self,
        scan_id: &str,
        finished_at_epoch_ms: i64,
        status: &str,
        cancelled: bool,
        files_scanned: u64,
        calls_added: u64,
        warning_count: u64,
    ) -> Result<(), LocalError> {
        self.connection
            .execute(statement(
                "UPDATE scan_runs SET
                   finished_at_epoch_ms = ?2, status = ?3, cancelled = ?4,
                   files_scanned = ?5, calls_added = ?6, warning_count = ?7
                 WHERE scan_id = ?1",
                vec![
                    scan_id.into(),
                    finished_at_epoch_ms.into(),
                    status.into(),
                    cancelled.into(),
                    to_sql_u64(files_scanned)?.into(),
                    to_sql_u64(calls_added)?.into(),
                    to_sql_u64(warning_count)?.into(),
                ],
            ))
            .await?;
        Ok(())
    }

    /// 用固定类别主键覆盖最近规范化快照。
    async fn save_provider_snapshot(
        &mut self,
        snapshot_kind: &str,
        observed_at_epoch_ms: i64,
        valid_until_epoch_ms: i64,
        normalized_json: String,
    ) -> Result<(), LocalError> {
        self.connection
            .execute(statement(
                "INSERT INTO provider_snapshots
                 (snapshot_kind, observed_at_epoch_ms, valid_until_epoch_ms, normalized_json)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(snapshot_kind) DO UPDATE SET
                   observed_at_epoch_ms = excluded.observed_at_epoch_ms,
                   valid_until_epoch_ms = excluded.valid_until_epoch_ms,
                   normalized_json = excluded.normalized_json",
                vec![
                    snapshot_kind.into(),
                    observed_at_epoch_ms.into(),
                    valid_until_epoch_ms.into(),
                    normalized_json.into(),
                ],
            ))
            .await?;
        Ok(())
    }
}
