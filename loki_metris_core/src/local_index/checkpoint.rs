//! 单个来源文件的安全 checkpoint 读写原语。
//!
//! 增量解析（判断追加/重建、流式读取 JSONL、按行分派进 core 调用模型）
//! 是各 adapter 自己的格式特定逻辑，留在 adapter 内；这里只暴露它们
//! 编排增量索引所需的最小存储原语，任何原始文件正文都不会经过这一层。

use std::collections::HashMap;

use sea_orm::{ConnectionTrait, DbBackend, Statement, TransactionTrait};

use super::call_store::{from_sql_u64, to_sql_u64};
use super::{LocalError, LocalIndex};
use crate::policy::SourceFileObservation;

/// 为检查点读写构造带绑定值的 SQLite 语句。
fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DbBackend::Sqlite, sql, values)
}

/// 描述一个来源文件的安全 checkpoint，不包含绝对路径或半行正文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSourceFile {
    /// adapter 生成的稳定来源 ID。
    pub source_id: String,
    /// 所属数据根 ID。
    pub root_id: String,
    /// `sessions` 或 `archived_sessions` 下的安全相对标签。
    pub relative_label: String,
    /// 来源文件的内部身份指纹，只用于判断追加还是替换。
    pub file_identity: String,
    /// 标识来源属于归档区域。
    pub archived: bool,
    /// 上次快照读取到的文件大小。
    pub observed_size: u64,
    /// 上次快照的修改时间毫秒值。
    pub modified_at_epoch_ms: i64,
    /// 最后完整 JSONL 行之后的字节偏移。
    pub parsed_offset: u64,
    /// 未提交末行的字节数量，不保存其原始内容。
    pub trailing_bytes: u64,
    /// 当前来源采用的解析器版本。
    pub parser_version: u32,
    /// 截断或替换时递增的来源 generation。
    pub generation: u64,
    /// 本 generation 是否已写入兼容 cumulative 快照；同时作为完整解析标记。
    pub token_snapshots_ready: bool,
}

/// 描述增量解析恢复所需的安全上下文；除受控的 project_label（路径末段）
/// 与 thread_label（短标题）外，不含任何原始 session 内容或完整路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceParseCheckpoint {
    /// 恢复解析时使用的线程标识。
    pub thread_key: String,
    /// 恢复解析时使用的项目键。
    pub project_key: Option<String>,
    /// 受控项目末段。
    pub project_label: Option<String>,
    /// 可选短线程标题。
    pub thread_label: Option<String>,
    /// 恢复解析时使用的模型名。
    pub model: Option<String>,
    /// 恢复解析时使用的推理强度。
    pub reasoning_effort: Option<String>,
    /// 文件内下一个调用序号。
    pub call_sequence: u64,
    /// adapter 私有、内容无关的版本化解析状态；core 只负责原样持久化。
    pub adapter_state: Option<String>,
}

impl LocalIndex {
    /// 返回一个来源当前生效的安全 checkpoint。
    pub async fn stored_source_file(
        &self,
        source_id: &str,
    ) -> Result<Option<StoredSourceFile>, LocalError> {
        let Some(row) = self
            .connection
            .query_one(statement(
                "SELECT source_id, root_id, relative_label, file_identity, archived,
                        observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                        parser_version, generation, token_snapshots_ready
                 FROM source_files WHERE source_id = ?1 AND ready = 1",
                vec![source_id.into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(StoredSourceFile {
            source_id: row.try_get_by_index(0)?,
            root_id: row.try_get_by_index(1)?,
            relative_label: row.try_get_by_index(2)?,
            file_identity: row.try_get_by_index(3)?,
            archived: row.try_get_by_index(4)?,
            observed_size: from_sql_u64(row.try_get_by_index(5)?)?,
            modified_at_epoch_ms: row.try_get_by_index(6)?,
            parsed_offset: from_sql_u64(row.try_get_by_index(7)?)?,
            trailing_bytes: from_sql_u64(row.try_get_by_index(8)?)?,
            parser_version: u32::try_from(row.try_get_by_index::<i64>(9)?).map_err(|_| {
                LocalError::new(
                    super::LocalErrorKind::Database,
                    "stored parser version is invalid",
                )
            })?,
            generation: from_sql_u64(row.try_get_by_index(10)?)?,
            token_snapshots_ready: row.try_get_by_index(11)?,
        }))
    }

    /// 返回某根某区域下全部来源上次记录的大小与修改时间，按相对标签索引，
    /// 供枚举阶段判断窗口外文件是否在两次扫描之间被追加或替换。
    /// 未完成提交的占位行也会返回（大小与时间均为 0），从而必然被再次访问。
    pub async fn source_file_observations_for_root(
        &self,
        root_id: &str,
        archived: bool,
    ) -> Result<HashMap<String, SourceFileObservation>, LocalError> {
        let rows = self
            .connection
            .query_all(statement(
                "SELECT relative_label, observed_size, modified_at_epoch_ms
                 FROM source_files WHERE root_id = ?1 AND archived = ?2",
                vec![root_id.into(), archived.into()],
            ))
            .await?;
        let mut observations = HashMap::with_capacity(rows.len());
        for row in rows {
            let relative_label: String = row.try_get_by_index(0)?;
            observations.insert(
                relative_label,
                SourceFileObservation {
                    observed_size: from_sql_u64(row.try_get_by_index(1)?)?,
                    modified_at_epoch_ms: row.try_get_by_index(2)?,
                },
            );
        }
        Ok(observations)
    }

    /// 恢复增量解析所需的安全上下文；除受控的 project_label/thread_label
    /// 外，不恢复任何原始 session 内容或完整路径。
    pub async fn load_parse_context(
        &self,
        source_id: &str,
    ) -> Result<Option<SourceParseCheckpoint>, LocalError> {
        let Some(row) = self
            .connection
            .query_one(statement(
                "SELECT thread_key, project_key, project_label, thread_label,
                        model, reasoning_effort, call_sequence, adapter_state
                 FROM source_files WHERE source_id = ?1 AND ready = 1",
                vec![source_id.into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(SourceParseCheckpoint {
            thread_key: row.try_get_by_index(0)?,
            project_key: row.try_get_by_index(1)?,
            project_label: row.try_get_by_index(2)?,
            thread_label: row.try_get_by_index(3)?,
            model: row.try_get_by_index(4)?,
            reasoning_effort: row.try_get_by_index(5)?,
            call_sequence: from_sql_u64(row.try_get_by_index(6)?)?,
            adapter_state: row.try_get_by_index(7)?,
        }))
    }

    /// 为一个此前从未见过的来源占位建立 checkpoint 行；已存在时是无操作。
    /// 调用方随后必须以 [`Self::commit_source_file_checkpoint`] 完成写入，
    /// 否则该行会一直停留在 `ready = 0` 的占位状态。
    #[allow(clippy::too_many_arguments)]
    pub async fn begin_source_file_checkpoint(
        &mut self,
        source_id: &str,
        root_id: &str,
        relative_label: &str,
        file_identity: &str,
        archived: bool,
        parser_version: u32,
        thread_key: &str,
        project_key: Option<&str>,
        project_label: Option<&str>,
        thread_label: Option<&str>,
    ) -> Result<(), LocalError> {
        self.connection
            .execute(statement(
                "INSERT INTO source_files
                 (source_id, root_id, relative_label, file_identity, archived,
                  observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                  oversized_tail, parser_version, generation, ready,
                  thread_key, project_key, project_label, thread_label,
                  model, reasoning_effort, call_sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, 0, ?6, 0, 0,
                         ?7, ?8, ?9, ?10, NULL, NULL, 0)
                 ON CONFLICT(source_id) DO NOTHING",
                vec![
                    source_id.into(),
                    root_id.into(),
                    relative_label.into(),
                    file_identity.into(),
                    archived.into(),
                    i64::from(parser_version).into(),
                    thread_key.into(),
                    project_key.map(str::to_owned).into(),
                    project_label.map(str::to_owned).into(),
                    thread_label.map(str::to_owned).into(),
                ],
            ))
            .await?;
        Ok(())
    }

    /// 丢弃某来源超出 `keep_generation` 的调用观察；重建前用于清理未来 generation
    /// 的残留（例如上一次重建被取消，留下比当前 checkpoint 更新的部分数据）。
    pub async fn discard_calls_beyond_generation(
        &mut self,
        source_id: &str,
        keep_generation: u64,
    ) -> Result<(), LocalError> {
        let keep = to_sql_u64(keep_generation)?;
        self.connection
            .execute(statement(
                "DELETE FROM usage_calls WHERE source_id = ?1 AND generation > ?2",
                vec![source_id.into(), keep.into()],
            ))
            .await?;
        self.connection
            .execute(statement(
                "DELETE FROM usage_token_snapshots WHERE source_id = ?1 AND generation > ?2",
                vec![source_id.into(), keep.into()],
            ))
            .await?;
        Ok(())
    }

    /// 丢弃某来源指定 generation 的全部调用观察；用于重建被取消时撤销本次
    /// 已经提交的部分写入，让下次扫描从旧 checkpoint 重新开始完整重建。
    pub async fn discard_generation(
        &mut self,
        source_id: &str,
        generation: u64,
    ) -> Result<(), LocalError> {
        let generation = to_sql_u64(generation)?;
        self.connection
            .execute(statement(
                "DELETE FROM usage_calls WHERE source_id = ?1 AND generation = ?2",
                vec![source_id.into(), generation.into()],
            ))
            .await?;
        self.connection
            .execute(statement(
                "DELETE FROM usage_token_snapshots WHERE source_id = ?1 AND generation = ?2",
                vec![source_id.into(), generation.into()],
            ))
            .await?;
        Ok(())
    }

    /// 在一个事务中把一次增量索引的最终 checkpoint 写回，并清理该来源其余
    /// generation 的调用（新一代完整写完后才删除旧一代，避免中途状态可见）。
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_source_file_checkpoint(
        &mut self,
        source_id: &str,
        root_id: &str,
        relative_label: &str,
        file_identity: &str,
        archived: bool,
        observed_size: u64,
        modified_at_epoch_ms: i64,
        parsed_offset: u64,
        trailing_bytes: u64,
        oversized_tail: bool,
        parser_version: u32,
        generation: u64,
        checkpoint: &SourceParseCheckpoint,
    ) -> Result<(), LocalError> {
        let generation_sql = to_sql_u64(generation)?;
        let transaction = self.connection.begin().await?;
        transaction
            .execute(statement(
                "INSERT INTO source_files
                 (source_id, root_id, relative_label, file_identity, archived,
                  observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                  oversized_tail, parser_version, generation, ready,
                  thread_key, project_key, project_label, thread_label,
                  model, reasoning_effort, call_sequence, token_snapshots_ready,
                  adapter_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1,
                         ?13, ?14, ?15, ?16, ?17, ?18, ?19, 1, ?20)
                 ON CONFLICT(source_id) DO UPDATE SET
                   root_id = excluded.root_id,
                   relative_label = excluded.relative_label,
                   file_identity = excluded.file_identity,
                   archived = excluded.archived,
                   observed_size = excluded.observed_size,
                   modified_at_epoch_ms = excluded.modified_at_epoch_ms,
                   parsed_offset = excluded.parsed_offset,
                   trailing_bytes = excluded.trailing_bytes,
                   oversized_tail = excluded.oversized_tail,
                   parser_version = excluded.parser_version,
                   generation = excluded.generation,
                   ready = 1,
                   thread_key = excluded.thread_key,
                   project_key = excluded.project_key,
                   project_label = excluded.project_label,
                   thread_label = excluded.thread_label,
                   model = excluded.model,
                   reasoning_effort = excluded.reasoning_effort,
                   call_sequence = excluded.call_sequence,
                   token_snapshots_ready = 1,
                   adapter_state = excluded.adapter_state",
                vec![
                    source_id.into(),
                    root_id.into(),
                    relative_label.into(),
                    file_identity.into(),
                    archived.into(),
                    to_sql_u64(observed_size)?.into(),
                    modified_at_epoch_ms.into(),
                    to_sql_u64(parsed_offset)?.into(),
                    to_sql_u64(trailing_bytes)?.into(),
                    oversized_tail.into(),
                    i64::from(parser_version).into(),
                    generation_sql.into(),
                    (&checkpoint.thread_key).into(),
                    checkpoint.project_key.clone().into(),
                    checkpoint.project_label.clone().into(),
                    checkpoint.thread_label.clone().into(),
                    checkpoint.model.clone().into(),
                    checkpoint.reasoning_effort.clone().into(),
                    to_sql_u64(checkpoint.call_sequence)?.into(),
                    checkpoint.adapter_state.clone().into(),
                ],
            ))
            .await?;
        transaction
            .execute(statement(
                "DELETE FROM usage_calls WHERE source_id = ?1 AND generation <> ?2",
                vec![source_id.into(), generation_sql.into()],
            ))
            .await?;
        transaction
            .execute(statement(
                "DELETE FROM usage_token_snapshots WHERE source_id = ?1 AND generation <> ?2",
                vec![source_id.into(), generation_sql.into()],
            ))
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}
