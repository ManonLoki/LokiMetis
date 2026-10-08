//! 管理数据根 registry 的登记、启停、别名，以及会话区域内失效来源的对账。
//!
//! 物理路径去重（比较两个路径是否指向同一挂载点/卷）依赖平台卷分类，
//! 那部分逻辑留在 GUI adapter（本 crate 保持 runtime-neutral，不引入卷
//! 枚举依赖）。因此“添加数据根时避免重复登记同一物理目录”的模糊去重
//! 检查由 adapter 在调用 [`LocalIndex::register_root_fields`] 之前自行
//! 完成；这里只保留一个不可绕过的硬保证：`root_id` 主键唯一，
//! [`LocalIndex::registered_root_paths`] 供 adapter 读取现状做去重判断。
//! adapter 侧的去重检查和这里的写入不在同一个事务里，理论上存在极窄的
//! 竞态窗口（两次几乎同时的手动添加）；最坏后果只是多出一行重复登记，
//! 用户可以随时移除，不是数据损坏或安全问题。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement, TransactionTrait};

use super::path_codec::{decode_path, encode_path};
use super::source_registry::{DiscoveryMethod, RegisteredRoot};
use super::{LocalError, LocalErrorKind, LocalIndex};
use crate::RootActivationState;

/// 为数据根登记读写构造带绑定值的 SQLite 语句。
pub(super) fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// 生成 `?start, ?start+1, ..., ?start+count-1` 形式的占位符列表，供动态
/// 长度的 `IN (...)` 子句使用；调用方必须按同一顺序绑定对应数量的实际值。
pub(super) fn in_placeholders(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 查询某个数据根当前是否是唯一主根；写入前用来判断是否需要连带清理主标记
/// 与缓存快照。不存在的 `root_id` 视作非主根。
pub(super) async fn was_primary(
    transaction: &DatabaseTransaction,
    root_id: &str,
) -> Result<bool, LocalError> {
    let is_primary: Option<bool> = transaction
        .query_one(statement(
            "SELECT is_primary FROM source_roots WHERE root_id = ?1",
            vec![root_id.into()],
        ))
        .await?
        .map(|row| row.try_get_by_index(0))
        .transpose()?;
    Ok(is_primary.unwrap_or(false))
}

/// 清空缓存的规范化快照；主根身份或启用状态变化后删除遗留行，避免旧行继续关联到新主根。
pub(super) async fn invalidate_primary_snapshot_cache(
    transaction: &DatabaseTransaction,
) -> Result<(), LocalError> {
    transaction
        .execute(statement("DELETE FROM provider_snapshots", vec![]))
        .await?;
    Ok(())
}

impl LocalIndex {
    /// 原子登记根的内部访问路径；若唯一主根的同一稳定 ID 被重绑到另一物理路径，
    /// 同一事务会清除主标记与遗留快照，避免旧行继续关联到新路径。
    // root_id 由路径规范化生成。用户重装或移动挂载点后，同一 root_id
    // 可能被重新绑定到另一个物理目录。若该根仍是主数据目录，继续保留
    // 旧 is_primary 标记和缓存快照会把旧路径的数据误绑到新路径。路径
    // 变化时同一事务清掉 primary 标记并清空 provider_snapshots。
    pub async fn register_root_fields(
        &mut self,
        root_id: &str,
        path: &Path,
        alias: &str,
        discovery_method: DiscoveryMethod,
    ) -> Result<(), LocalError> {
        let access_path = encode_path(path);
        let transaction = self.connection.begin().await?;
        let existing = transaction
            .query_one(statement(
                "SELECT access_path, is_primary FROM source_roots WHERE root_id = ?1",
                vec![root_id.into()],
            ))
            .await?
            .map(|row| {
                Ok::<_, LocalError>((
                    row.try_get_by_index::<Vec<u8>>(0)?,
                    row.try_get_by_index::<bool>(1)?,
                ))
            })
            .transpose()?;
        // 只做字节级路径比较：判断“同一物理目录”的模糊比较（symlink/挂载点
        // 识别）依赖平台卷分类，那部分留在 GUI adapter，本 crate 保持
        // runtime-neutral。字节不同就保守地当作“路径已变”并清掉 primary
        // 标记与缓存快照——比原实现更容易误判为“变了”，但方向是安全的：
        // 多清一次缓存只是下次要重新确认账号上下文，而不是让旧账号事实
        // 错误地关联到新路径。
        let primary_path_changed = existing
            .as_ref()
            .is_some_and(|(stored_path, is_primary)| *is_primary && stored_path != &access_path);
        transaction
            .execute(statement(
                "INSERT INTO source_roots
                 (root_id, access_path, alias, enabled, discovery_method)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT(root_id) DO UPDATE SET
                   access_path = excluded.access_path",
                vec![
                    root_id.into(),
                    access_path.into(),
                    alias.into(),
                    discovery_method.as_str().into(),
                ],
            ))
            .await?;
        if primary_path_changed {
            transaction
                .execute(statement(
                    "UPDATE source_roots SET is_primary = 0 WHERE root_id = ?1",
                    vec![root_id.into()],
                ))
                .await?;
            invalidate_primary_snapshot_cache(&transaction).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// 只登记用户确认的元数据候选，不读取内容并保持首次索引未激活。
    pub async fn register_confirmed_root_fields(
        &mut self,
        root_id: &str,
        path: &Path,
        alias: &str,
        discovery_method: DiscoveryMethod,
    ) -> Result<bool, LocalError> {
        let result = self
            .connection
            .execute(statement(
                "INSERT INTO source_roots
                 (root_id, access_path, alias, enabled, discovery_method, activation_state)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5)
                 ON CONFLICT(root_id) DO NOTHING",
                vec![
                    root_id.into(),
                    encode_path(path).into(),
                    alias.into(),
                    discovery_method.as_str().into(),
                    RootActivationState::ConfirmedUnindexed.as_str().into(),
                ],
            ))
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// 返回全部已登记根的 `(root_id, 内部访问路径)`；仅供 adapter 在写入前做
    /// 物理路径去重判断，不代表任何写入保证（见模块文档的竞态说明）。
    pub async fn registered_root_paths(&self) -> Result<Vec<(String, PathBuf)>, LocalError> {
        let rows = self
            .connection
            .query_all(statement(
                "SELECT root_id, access_path FROM source_roots",
                vec![],
            ))
            .await?;
        let mut roots = Vec::with_capacity(rows.len());
        for row in rows {
            let root_id: String = row.try_get_by_index(0)?;
            let encoded: Vec<u8> = row.try_get_by_index(1)?;
            let path = decode_path(&encoded).ok_or_else(|| {
                LocalError::new(LocalErrorKind::Database, "registered root path is invalid")
            })?;
            roots.push((root_id, path));
        }
        Ok(roots)
    }

    /// 返回全部已启用历史确认根，供启动快速发现补回主动发现候选。
    pub async fn known_roots(&self) -> Result<Vec<RegisteredRoot>, LocalError> {
        self.load_root_paths(false).await
    }

    /// 返回包括已停用项的全部历史根，只供主动发现复用精确身份与停用状态。
    pub async fn all_roots(&self) -> Result<Vec<RegisteredRoot>, LocalError> {
        self.load_root_paths(true).await
    }

    /// 返回唯一主数据根的内部访问位置；没有主根时保留进程继承的 Codex 上下文。
    pub async fn primary_root(&self) -> Result<Option<RegisteredRoot>, LocalError> {
        let Some(row) = self
            .connection
            .query_one(statement(
                "SELECT root_id, access_path, alias, enabled
                 FROM source_roots WHERE is_primary = 1",
                vec![],
            ))
            .await?
        else {
            return Ok(None);
        };
        let root_id: String = row.try_get_by_index(0)?;
        let encoded: Vec<u8> = row.try_get_by_index(1)?;
        let alias: String = row.try_get_by_index(2)?;
        let enabled: bool = row.try_get_by_index(3)?;
        if !enabled {
            return Err(LocalError::new(
                LocalErrorKind::Database,
                "primary root registry state is invalid",
            ));
        }
        let path = decode_path(&encoded).ok_or_else(|| {
            LocalError::new(LocalErrorKind::Database, "registered root path is invalid")
        })?;
        Ok(Some(RegisteredRoot {
            root_id: Some(root_id),
            path,
            alias,
            enabled,
        }))
    }

    /// 在单个写事务中切换唯一主数据根；`None` 表示恢复继承的 Codex 上下文。
    pub async fn set_primary_root(&mut self, root_id: Option<&str>) -> Result<bool, LocalError> {
        let transaction = self.connection.begin().await?;
        let current: Option<String> = transaction
            .query_one(statement(
                "SELECT root_id FROM source_roots WHERE is_primary = 1",
                vec![],
            ))
            .await?
            .map(|row| row.try_get_by_index(0))
            .transpose()?;
        if current.as_deref() == root_id {
            return Ok(false);
        }
        if let Some(root_id) = root_id {
            let enabled: Option<bool> = transaction
                .query_one(statement(
                    "SELECT enabled FROM source_roots WHERE root_id = ?1",
                    vec![root_id.into()],
                ))
                .await?
                .map(|row| row.try_get_by_index(0))
                .transpose()?;
            if enabled != Some(true) {
                return Ok(false);
            }
        }
        transaction
            .execute(statement(
                "UPDATE source_roots SET is_primary = 0 WHERE is_primary = 1",
                vec![],
            ))
            .await?;
        if let Some(root_id) = root_id {
            transaction
                .execute(statement(
                    "UPDATE source_roots SET is_primary = 1 WHERE root_id = ?1 AND enabled = 1",
                    vec![root_id.into()],
                ))
                .await?;
        }
        invalidate_primary_snapshot_cache(&transaction).await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// 从 registry 恢复 adapter 内部路径，并按调用边界决定是否包含停用项。
    async fn load_root_paths(
        &self,
        include_disabled: bool,
    ) -> Result<Vec<RegisteredRoot>, LocalError> {
        let sql = if include_disabled {
            "SELECT root_id, access_path, alias, enabled FROM source_roots
             ORDER BY alias, root_id"
        } else {
            "SELECT root_id, access_path, alias, enabled FROM source_roots
             WHERE enabled = 1 AND activation_state IN ('ready', 'indexing')
             ORDER BY alias, root_id"
        };
        let rows = self.connection.query_all(statement(sql, vec![])).await?;
        let mut roots = Vec::with_capacity(rows.len());
        for row in rows {
            let root_id: String = row.try_get_by_index(0)?;
            let encoded: Vec<u8> = row.try_get_by_index(1)?;
            let alias: String = row.try_get_by_index(2)?;
            let enabled: bool = row.try_get_by_index(3)?;
            let path = decode_path(&encoded).ok_or_else(|| {
                LocalError::new(LocalErrorKind::Database, "registered root path is invalid")
            })?;
            roots.push(RegisteredRoot {
                root_id: Some(root_id),
                path,
                alias,
                enabled,
            });
        }
        Ok(roots)
    }

    /// 把重新确认的验证失败根重新排入后台统计，已就绪根保持不变。
    pub async fn queue_confirmed_root_for_background_scan(
        &mut self,
        root_id: &str,
    ) -> Result<RootActivationState, LocalError> {
        self.connection
            .execute(statement(
                "UPDATE source_roots SET activation_state = 'confirmed_unindexed'
                 WHERE root_id = ?1 AND enabled = 1 AND activation_state = 'validation_failed'",
                vec![root_id.into()],
            ))
            .await?;
        let label: String = self
            .connection
            .query_one(statement(
                "SELECT activation_state FROM source_roots WHERE root_id = ?1",
                vec![root_id.into()],
            ))
            .await?
            .ok_or_else(|| LocalError::new(LocalErrorKind::Database, "root is not registered"))?
            .try_get_by_index(0)?;
        RootActivationState::from_label(&label).ok_or_else(|| {
            LocalError::new(LocalErrorKind::Database, "root activation state is invalid")
        })
    }

    /// 原子认领本轮后台统计根，遗留 `indexing` 根也会在下轮重试。
    pub async fn claim_background_index_roots(&mut self) -> Result<Vec<String>, LocalError> {
        let transaction = self.connection.begin().await?;
        let rows = transaction
            .query_all(statement(
                "SELECT root_id FROM source_roots
                 WHERE enabled = 1 AND activation_state IN ('confirmed_unindexed', 'indexing')
                 ORDER BY root_id",
                vec![],
            ))
            .await?;
        let root_ids = rows
            .into_iter()
            .map(|row| row.try_get_by_index(0))
            .collect::<Result<Vec<String>, _>>()?;
        transaction
            .execute(statement(
                "UPDATE source_roots SET activation_state = 'indexing'
                 WHERE enabled = 1 AND activation_state = 'confirmed_unindexed'",
                vec![],
            ))
            .await?;
        transaction.commit().await?;
        Ok(root_ids)
    }

    /// 只认领本轮发现结果中的精确根；不会把扫描开始后由其他入口新增的根带入本轮。
    pub async fn claim_discovered_roots_for_current_scan(
        &mut self,
        root_ids: &[String],
    ) -> Result<Vec<String>, LocalError> {
        let transaction = self.connection.begin().await?;
        let mut claimed = Vec::new();
        for root_id in root_ids.iter().map(String::as_str).collect::<BTreeSet<_>>() {
            let result = transaction
                .execute(statement(
                    "UPDATE source_roots SET activation_state = 'indexing'
                     WHERE root_id = ?1 AND enabled = 1
                       AND activation_state IN ('confirmed_unindexed', 'validation_failed')",
                    vec![root_id.into()],
                ))
                .await?;
            if result.rows_affected() > 0 {
                claimed.push(root_id.to_owned());
            }
        }
        transaction.commit().await?;
        Ok(claimed)
    }

    /// 根据当前 parser 已完整提交的来源完成本轮后台验证。
    ///
    /// 合法来源可能在本次日期窗口内没有任何调用；这种情况仍是已完成扫描的
    /// `ReadyNoCalls`，不能把“零调用”误当成候选结构验证失败。
    pub async fn finish_background_index_roots(
        &mut self,
        root_ids: &[String],
    ) -> Result<(), LocalError> {
        for root_id in root_ids {
            self.connection
                .execute(statement(
                    "UPDATE source_roots
                     SET activation_state = CASE
                       WHEN EXISTS (
                         SELECT 1 FROM source_files f
                         WHERE f.root_id = source_roots.root_id AND f.ready = 1
                           AND f.parser_version = ?2
                       ) THEN 'ready' ELSE 'validation_failed' END
                     WHERE root_id = ?1 AND activation_state = 'indexing'",
                    vec![
                        root_id.clone().into(),
                        i64::from(self.parser_version).into(),
                    ],
                ))
                .await?;
        }
        Ok(())
    }

    /// 启用或禁用登记根；禁用不会删除原始数据文件。
    pub async fn set_root_enabled(
        &mut self,
        root_id: &str,
        enabled: bool,
    ) -> Result<bool, LocalError> {
        let transaction = self.connection.begin().await?;
        let was_primary = was_primary(&transaction, root_id).await?;
        let result = if enabled {
            transaction
                .execute(statement(
                    "UPDATE source_roots SET enabled = 1 WHERE root_id = ?1",
                    vec![root_id.into()],
                ))
                .await?
        } else {
            transaction
                .execute(statement(
                    "UPDATE source_roots SET enabled = 0, is_primary = 0 WHERE root_id = ?1",
                    vec![root_id.into()],
                ))
                .await?
        };
        if !enabled && was_primary {
            invalidate_primary_snapshot_cache(&transaction).await?;
        }
        transaction.commit().await?;
        Ok(result.rows_affected() > 0)
    }
}

mod mutate;

#[cfg(test)]
mod tests;
