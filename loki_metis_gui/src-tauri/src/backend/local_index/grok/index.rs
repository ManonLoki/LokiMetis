//! 把已验证 Grok `updates.jsonl` 增量写入：FS/parse 在 `spawn_blocking` 岛内。

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use loki_metis_core::{
    LocalError, LocalErrorKind, LocalIndex, SourceParseCheckpoint, SourceProvenance, UsageCall,
    retain_ingestable_calls,
};
use tauri::async_runtime::spawn_blocking;

use super::discovery::GrokDiscoveredRoot;
use super::jsonl::{
    GROK_PARSER_VERSION, GrokJsonlParseContext, GrokJsonlParseReport, GrokJsonlWarningCounts,
    parse_grok_jsonl_stream,
};
use super::path_rules::{grok_project_seed, grok_session_seed, is_grok_updates_path};
use crate::backend::local_index::discovery::{
    metadata_is_link_like, path_key, stable_id, walk_ancestors_for_link_component,
};
use crate::backend::local_index::file_source::{
    file_identity, modified_epoch_ms, same_opened_file,
};
use crate::backend::local_index::rollout_ingest::INSERT_BATCH_SIZE;
use crate::backend::local_index::scan::CancellationToken;

/// 一个 Grok updates 文件的增量索引结果。
pub(super) struct GrokFileIndexOutcome {
    pub(super) added_calls: u64,
    pub(super) rebuilt: bool,
    pub(super) unchanged: bool,
    pub(super) warnings: GrokJsonlWarningCounts,
    pub(super) cancelled: bool,
}

/// 保存 Grok 索引前已验证的来源身份与现有解析状态。
struct GrokIdentityPrep {
    source_id: String,
    relative_label: String,
    identity: String,
    observed_size: u64,
    modified_at_epoch_ms: i64,
    project_seed: Option<String>,
    thread_seed: String,
    file: File,
}

/// 汇总单轮 Grok 解析得到的调用批次与新检查点。
struct ParsedGrokBatches {
    batches: Vec<Vec<UsageCall>>,
    context: GrokJsonlParseContext,
    report: GrokJsonlParseReport,
}

/// 为 `LocalIndex` 补充 Grok updates 的增量索引编排。
pub(super) trait IndexGrokUpdates {
    /// 校验来源仍位于 `sessions` 允许层级后，增量解析并写入。
    /// `ingest_since_epoch_ms` 是解析结果允许入库的发生时间下界（保留窗口），
    /// 不是本轮扫描窗口。调用方必须已经在本次扫描中登记过 `root`（每根登记
    /// 一次即可，不要每个文件重复登记，避免每个文件都触发一次多余的事务）。
    async fn index_grok_updates(
        &mut self,
        root: &GrokDiscoveredRoot,
        file_path: &Path,
        max_line_bytes: usize,
        ingest_since_epoch_ms: i64,
        force_rebuild: bool,
        cancellation: &CancellationToken,
    ) -> Result<GrokFileIndexOutcome, LocalError>;
}

impl IndexGrokUpdates for LocalIndex {
    /// 校验来源仍位于 `sessions` 允许层级后，增量解析并写入。
    async fn index_grok_updates(
        &mut self,
        root: &GrokDiscoveredRoot,
        file_path: &Path,
        max_line_bytes: usize,
        ingest_since_epoch_ms: i64,
        force_rebuild: bool,
        cancellation: &CancellationToken,
    ) -> Result<GrokFileIndexOutcome, LocalError> {
        let root_id = root.root_id.clone();
        let root_path = root.path.clone();
        let path = file_path.to_path_buf();
        let root_id_for_prep = root_id.clone();
        let prep = spawn_blocking(move || prepare_grok_identity(path, root_path, root_id_for_prep))
            .await
            .map_err(|_| {
                LocalError::new(
                    LocalErrorKind::SourceUnavailable,
                    "Grok identity worker lost",
                )
            })??;
        let existing = self.stored_source_file(&prep.source_id).await?;
        if !force_rebuild
            && let Some(existing) = &existing
            && existing.observed_size == prep.observed_size
            && existing.modified_at_epoch_ms == prep.modified_at_epoch_ms
            && existing.parser_version == GROK_PARSER_VERSION
            && existing.file_identity == prep.identity
            && existing
                .parsed_offset
                .saturating_add(existing.trailing_bytes)
                == existing.observed_size
        {
            return Ok(GrokFileIndexOutcome {
                added_calls: 0,
                rebuilt: false,
                unchanged: true,
                warnings: GrokJsonlWarningCounts::default(),
                cancelled: false,
            });
        }
        let can_append = !force_rebuild
            && existing.as_ref().is_some_and(|checkpoint| {
                checkpoint.parser_version == GROK_PARSER_VERSION
                    && prep.observed_size > checkpoint.observed_size
                    && checkpoint.file_identity == prep.identity
            });
        let rebuilt = existing.is_some() && !can_append;
        let generation = match (&existing, can_append) {
            (Some(checkpoint), true) => checkpoint.generation,
            (Some(checkpoint), false) => checkpoint.generation.saturating_add(1),
            (None, _) => 1,
        };
        let parse_start = if can_append {
            existing
                .as_ref()
                .map(|checkpoint| checkpoint.parsed_offset)
                .unwrap_or_default()
        } else {
            0
        };
        let context = if can_append {
            self.load_parse_context(&prep.source_id)
                .await?
                .map(|checkpoint| GrokJsonlParseContext {
                    thread_key: checkpoint.thread_key,
                    project_key: checkpoint.project_key,
                    project_label: checkpoint.project_label,
                    call_sequence: checkpoint.call_sequence,
                })
                .unwrap_or_else(|| {
                    GrokJsonlParseContext::new(&prep.thread_seed, prep.project_seed.as_deref())
                })
        } else {
            GrokJsonlParseContext::new(&prep.thread_seed, prep.project_seed.as_deref())
        };
        if existing.is_none() {
            self.begin_source_file_checkpoint(
                &prep.source_id,
                &root_id,
                &prep.relative_label,
                &prep.identity,
                false,
                GROK_PARSER_VERSION,
                &context.thread_key,
                context.project_key.as_deref(),
                context.project_label.as_deref(),
                None,
            )
            .await?;
        }
        self.discard_calls_beyond_generation(
            &prep.source_id,
            existing.as_ref().map_or(0, |item| item.generation),
        )
        .await?;
        let source_id = prep.source_id.clone();
        let relative_label = prep.relative_label.clone();
        let root_id_for_parse = root_id.clone();
        let parse_cancellation = cancellation.clone();
        let observed_size = prep.observed_size;
        let mut parsed = spawn_blocking(move || {
            parse_grok_batches(GrokParseRequest {
                file: prep.file,
                observed_size,
                parse_start,
                max_line_bytes,
                source_id,
                root_id: root_id_for_parse,
                relative_label,
                context,
                cancellation: parse_cancellation,
            })
        })
        .await
        .map_err(|_| {
            LocalError::new(LocalErrorKind::SourceUnavailable, "Grok parse worker lost")
        })??;
        for batch in &mut parsed.batches {
            retain_ingestable_calls(batch, ingest_since_epoch_ms);
        }
        let context = parsed.context;
        let report = parsed.report;
        let mut added_calls = 0_u64;
        for mut batch in parsed.batches {
            added_calls = added_calls.saturating_add(
                self.insert_usage_batch(&prep.source_id, generation, &mut batch)
                    .await?,
            );
        }
        if report.cancelled && rebuilt && existing.is_some() {
            self.discard_generation(&prep.source_id, generation).await?;
            return Ok(GrokFileIndexOutcome {
                added_calls: 0,
                rebuilt,
                unchanged: false,
                warnings: report.warnings,
                cancelled: true,
            });
        }
        let parsed_offset = parse_start
            .checked_add(report.checkpoint.committed_bytes)
            .ok_or_else(|| LocalError::new(LocalErrorKind::Overflow, "file offset overflowed"))?;
        let checkpoint_observed_size = if report.cancelled {
            parsed_offset
                .checked_add(report.checkpoint.trailing_bytes)
                .ok_or_else(|| {
                    LocalError::new(LocalErrorKind::Overflow, "file offset overflowed")
                })?
        } else {
            prep.observed_size
        };
        self.commit_source_file_checkpoint(
            &prep.source_id,
            &root_id,
            &prep.relative_label,
            &prep.identity,
            false,
            checkpoint_observed_size,
            prep.modified_at_epoch_ms,
            parsed_offset,
            report.checkpoint.trailing_bytes,
            report.checkpoint.discarding_oversized_line,
            GROK_PARSER_VERSION,
            generation,
            &SourceParseCheckpoint {
                thread_key: context.thread_key,
                project_key: context.project_key,
                project_label: context.project_label,
                thread_label: None,
                model: None,
                reasoning_effort: None,
                call_sequence: context.call_sequence,
                adapter_state: None,
            },
        )
        .await?;
        Ok(GrokFileIndexOutcome {
            added_calls,
            rebuilt,
            unchanged: false,
            warnings: report.warnings,
            cancelled: report.cancelled,
        })
    }
}

/// 核对 Grok 根身份并加载可安全复用的解析检查点。
fn prepare_grok_identity(
    file_path: PathBuf,
    root_path: PathBuf,
    root_id: String,
) -> Result<GrokIdentityPrep, LocalError> {
    let mut file = File::open(&file_path)?;
    let metadata = file.metadata()?;
    let current_metadata = fs::symlink_metadata(&file_path)?;
    if !metadata.is_file()
        || !current_metadata.is_file()
        || metadata_is_link_like(&current_metadata)
        || walk_ancestors_for_link_component(&file_path)?
        || !same_opened_file(&file, &file_path, &metadata, &current_metadata)?
    {
        return Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "Grok session source changed during validation",
        ));
    }
    let normalized_path = fs::canonicalize(&file_path)?;
    let relative = normalized_path.strip_prefix(&root_path).map_err(|_| {
        LocalError::new(LocalErrorKind::InvalidPath, "Grok updates escaped its root")
    })?;
    if !is_grok_updates_path(relative) {
        return Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "Grok updates path is outside the approved sessions layout",
        ));
    }
    let relative_label = path_key(relative);
    let thread_seed = grok_session_seed(relative).unwrap_or_else(|| relative_label.clone());
    let source_id = stable_id("grok-source", &format!("{root_id}\u{0}{relative_label}"));
    let identity = file_identity(&mut file)?;
    Ok(GrokIdentityPrep {
        source_id,
        relative_label,
        identity,
        observed_size: metadata.len(),
        modified_at_epoch_ms: modified_epoch_ms(&metadata),
        project_seed: grok_project_seed(relative),
        thread_seed,
        file,
    })
}

/// 封装 Grok 批量解析所需的不可变输入与资源限制。
struct GrokParseRequest {
    file: File,
    observed_size: u64,
    parse_start: u64,
    max_line_bytes: usize,
    source_id: String,
    root_id: String,
    relative_label: String,
    context: GrokJsonlParseContext,
    cancellation: CancellationToken,
}

/// 按稳定顺序解析 Grok 更新文件并生成原子写入批次。
fn parse_grok_batches(request: GrokParseRequest) -> Result<ParsedGrokBatches, LocalError> {
    let GrokParseRequest {
        mut file,
        observed_size,
        parse_start,
        max_line_bytes,
        source_id,
        root_id,
        relative_label,
        mut context,
        cancellation,
    } = request;
    file.seek(SeekFrom::Start(parse_start))?;
    let readable_bytes = observed_size.saturating_sub(parse_start);
    let provenance = SourceProvenance {
        source_id,
        root_id,
        relative_label,
        archived: false,
    };
    let mut batch = Vec::<UsageCall>::with_capacity(INSERT_BATCH_SIZE);
    let mut batches = Vec::new();
    let report = parse_grok_jsonl_stream(
        file.take(readable_bytes),
        max_line_bytes,
        &mut context,
        &provenance,
        &cancellation,
        |call| {
            batch.push(call);
            if batch.len() >= INSERT_BATCH_SIZE {
                batches.push(std::mem::take(&mut batch));
                batch = Vec::with_capacity(INSERT_BATCH_SIZE);
            }
            Ok(())
        },
    )?;
    if !batch.is_empty() {
        batches.push(batch);
    }
    Ok(ParsedGrokBatches {
        batches,
        context,
        report,
    })
}
