//! 数据根别名、移除、覆盖状态与会话来源对账。

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, TransactionTrait};

use crate::CoverageState;

use super::super::source_registry::coverage_state_label;
use super::super::{LocalError, LocalIndex};
use super::{in_placeholders, invalidate_primary_snapshot_cache, statement, was_primary};

impl LocalIndex {
    /// 更新已登记数据根的安全展示别名；访问路径和原始文件不变。
    pub async fn set_root_alias(&mut self, root_id: &str, alias: &str) -> Result<bool, LocalError> {
        let result = self
            .connection
            .execute(statement(
                "UPDATE source_roots SET alias = ?2 WHERE root_id = ?1",
                vec![root_id.into(), alias.into()],
            ))
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// 移除根及其本产品索引记录，外部数据始终保持只读不变。
    pub async fn remove_root(&mut self, root_id: &str) -> Result<bool, LocalError> {
        let transaction = self.connection.begin().await?;
        let was_primary = was_primary(&transaction, root_id).await?;
        let result = transaction
            .execute(statement(
                "DELETE FROM source_roots WHERE root_id = ?1",
                vec![root_id.into()],
            ))
            .await?;
        if was_primary {
            invalidate_primary_snapshot_cache(&transaction).await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// 在一个事务中移除已确认失效根及其级联索引，绝不修改外部数据文件。
    pub async fn remove_roots(&mut self, root_ids: &[String]) -> Result<u64, LocalError> {
        if root_ids.is_empty() {
            return Ok(0);
        }
        let transaction = self.connection.begin().await?;
        let placeholders = in_placeholders(1, root_ids.len());
        let id_values: Vec<sea_orm::Value> = root_ids.iter().map(Into::into).collect();
        let removed_primary = transaction
            .query_one(statement(
                &format!(
                    "SELECT 1 FROM source_roots WHERE root_id IN ({placeholders}) AND is_primary = 1 LIMIT 1"
                ),
                id_values.clone(),
            ))
            .await?
            .is_some();
        let result = transaction
            .execute(statement(
                &format!("DELETE FROM source_roots WHERE root_id IN ({placeholders})"),
                id_values,
            ))
            .await?;
        if removed_primary {
            invalidate_primary_snapshot_cache(&transaction).await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    /// 更新一个数据根最近的覆盖状态，供来源页解释部分或取消扫描。
    pub async fn record_root_coverage(
        &mut self,
        root_id: &str,
        state: CoverageState,
    ) -> Result<(), LocalError> {
        self.connection
            .execute(statement(
                "UPDATE source_roots SET last_coverage_state = ?2 WHERE root_id = ?1",
                vec![root_id.into(), coverage_state_label(state).into()],
            ))
            .await?;
        Ok(())
    }

    /// 用同一覆盖状态批量更新多个数据根；仅适用于一批根需要写入同一个
    /// `state` 的场景（例如快速重验证里一批"无法确认"的历史根都记为
    /// `Partial`）。单个根状态各不相同时，仍应逐个调用
    /// [`Self::record_root_coverage`]。
    pub async fn record_roots_coverage(
        &mut self,
        root_ids: &[String],
        state: CoverageState,
    ) -> Result<(), LocalError> {
        if root_ids.is_empty() {
            return Ok(());
        }
        let placeholders = in_placeholders(2, root_ids.len());
        let mut values: Vec<sea_orm::Value> = vec![coverage_state_label(state).into()];
        values.extend(root_ids.iter().map(Into::into));
        self.connection
            .execute(statement(
                &format!(
                    "UPDATE source_roots SET last_coverage_state = ?1 WHERE root_id IN ({placeholders})"
                ),
                values,
            ))
            .await?;
        Ok(())
    }

    /// 对账一个会话区域：精确拒绝始终删除，仅在完整枚举时推断并删除缺失来源。
    pub async fn reconcile_root_sources(
        &mut self,
        root_id: &str,
        archived: bool,
        retained_relative_labels: &HashSet<String>,
        rejected_relative_labels: &HashSet<String>,
        enumeration_complete: bool,
    ) -> Result<u64, LocalError> {
        let rows = self
            .connection
            .query_all(statement(
                "SELECT source_id, relative_label FROM source_files
                 WHERE root_id = ?1 AND archived = ?2",
                vec![root_id.into(), archived.into()],
            ))
            .await?;
        let mut stale_source_ids = Vec::new();
        for row in rows {
            let source_id: String = row.try_get_by_index(0)?;
            let relative_label: String = row.try_get_by_index(1)?;
            if rejected_relative_labels.contains(&relative_label)
                || (enumeration_complete && !retained_relative_labels.contains(&relative_label))
            {
                stale_source_ids.push(source_id);
            }
        }
        if stale_source_ids.is_empty() {
            return Ok(0);
        }
        let transaction = self.connection.begin().await?;
        let placeholders = in_placeholders(2, stale_source_ids.len());
        let mut values: Vec<sea_orm::Value> = vec![root_id.into()];
        values.extend(stale_source_ids.iter().map(Into::into));
        let result = transaction
            .execute(statement(
                &format!(
                    "DELETE FROM source_files WHERE root_id = ?1 AND source_id IN ({placeholders})"
                ),
                values,
            ))
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    /// 返回不含绝对路径的数据根 registry 展示记录，供 adapter 诊断与测试观察登记状态。
    pub async fn list_sources(&self) -> Result<Vec<crate::local_index::RootRecord>, LocalError> {
        crate::local_index::snapshot::load_root_records(&self.connection, self.parser_version).await
    }
}
