//! 把已验证 Claude transcript 增量写入：FS/parse 在 `spawn_blocking` 岛内，
//! checkpoint 与批次写入在 async 侧直接 await。

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use loki_metis_core::{
    LocalError, LocalErrorKind, LocalIndex, SourceParseCheckpoint, SourceProvenance, UsageCall,
    retain_ingestable_calls,
};
use tauri::async_runtime::spawn_blocking;

use super::discovery::ClaudeDiscoveredRoot;
use super::jsonl::{
    CLAUDE_PARSER_VERSION, ClaudeJsonlParseContext, ClaudeJsonlParseReport,
    ClaudeJsonlWarningCounts, parse_claude_jsonl_stream,
};
use super::path_rules::validate_transcript_path;
use crate::backend::local_index::discovery::{
    metadata_is_link_like, path_key, stable_id, walk_ancestors_for_link_component,
};
use crate::backend::local_index::file_source::{
    file_identity, modified_epoch_ms, same_opened_file,
};
use crate::backend::local_index::rollout_ingest::INSERT_BATCH_SIZE;
use crate::backend::local_index::scan::CancellationToken;

/// 一个 Claude transcript 的增量索引结果。
pub(super) struct ClaudeFileIndexOutcome {
    /// 该来源在数据库中的稳定 ID。
    pub(super) source_id: String,
    /// 本次写入新增的调用条数。
    pub(super) added_calls: u64,
    /// 标识本次是整份重建（而非增量续读）。
    pub(super) rebuilt: bool,
    /// 标识本次未发现任何新内容。
    pub(super) unchanged: bool,
    /// 本次解析累计的格式与数据质量告警。
    pub(super) warnings: ClaudeJsonlWarningCounts,
    /// 尚未凑成完整行、留待下次续读的尾部字节数。
    pub(super) trailing_bytes: u64,
    /// 标识本次索引因取消信号提前终止。
    pub(super) cancelled: bool,
}

/// 打开并通过路径校验后的 Claude 文件身份。
struct ClaudeIdentityPrep {
    /// 该来源在数据库中的稳定 ID。
    source_id: String,
    /// 不含绝对路径的相对展示标签。
    relative_label: String,
    /// 文件物理身份散列，用于跨增量比对是否被替换。
    identity: String,
    /// 打开时观测到的文件字节数。
    observed_size: u64,
    /// 打开时观测到的修改时间。
    modified_at_epoch_ms: i64,
    /// 派生项目键所需的原始项目种子；无项目关联时为空。
    project_seed: Option<String>,
    /// 已打开、已通过校验的文件句柄。
    file: File,
}

/// 解析岛产出的批次与上下文。
struct ParsedClaudeBatches {
    /// 按批次切分、待写入数据库的调用记录。
    batches: Vec<Vec<UsageCall>>,
    /// 解析过程中产生的散列上下文（线程/项目键等）。
    context: ClaudeJsonlParseContext,
    /// 本次流式解析的完整报告，含检查点与告警。
    report: ClaudeJsonlParseReport,
}

/// 为 `LocalIndex` 补充 Claude transcript 的增量索引编排。
pub(super) trait IndexClaudeTranscript {
    /// 校验来源仍位于 `projects` 允许层级后，增量解析并单调 upsert。
    /// `ingest_since_epoch_ms` 是解析结果允许入库的发生时间下界（保留窗口），
    /// 不是本轮扫描窗口。调用方必须已经在本次扫描中登记过 `root`（每根登记
    /// 一次即可，不要每个文件重复登记，避免每个文件都触发一次多余的事务）。
    async fn index_claude_transcript(
        &mut self,
        root: &ClaudeDiscoveredRoot,
        file_path: &Path,
        max_line_bytes: usize,
        ingest_since_epoch_ms: i64,
        force_rebuild: bool,
        cancellation: &CancellationToken,
    ) -> Result<ClaudeFileIndexOutcome, LocalError>;
}

impl IndexClaudeTranscript for LocalIndex {
    /// 校验来源仍位于 `projects` 允许层级后，增量解析并单调 upsert。
    async fn index_claude_transcript(
        &mut self,
        root: &ClaudeDiscoveredRoot,
        file_path: &Path,
        max_line_bytes: usize,
        ingest_since_epoch_ms: i64,
        force_rebuild: bool,
        cancellation: &CancellationToken,
    ) -> Result<ClaudeFileIndexOutcome, LocalError> {
        let root_id = root.root_id.clone();
        let root_path = root.path.clone();
        let path = file_path.to_path_buf();
        let root_id_for_prep = root_id.clone();
        let prep =
            spawn_blocking(move || prepare_claude_identity(path, root_path, root_id_for_prep))
                .await
                .map_err(|_| {
                    LocalError::new(
                        LocalErrorKind::SourceUnavailable,
                        "Claude identity worker lost",
                    )
                })??;
        let existing = self.stored_source_file(&prep.source_id).await?;
        if !force_rebuild
            && let Some(existing) = &existing
            && existing.observed_size == prep.observed_size
            && existing.modified_at_epoch_ms == prep.modified_at_epoch_ms
            && existing.parser_version == CLAUDE_PARSER_VERSION
            && existing.file_identity == prep.identity
            && existing
                .parsed_offset
                .saturating_add(existing.trailing_bytes)
                == existing.observed_size
        {
            return Ok(ClaudeFileIndexOutcome {
                source_id: prep.source_id,
                added_calls: 0,
                rebuilt: false,
                unchanged: true,
                warnings: ClaudeJsonlWarningCounts::default(),
                trailing_bytes: existing.trailing_bytes,
                cancelled: false,
            });
        }
        let can_append = !force_rebuild
            && existing.as_ref().is_some_and(|checkpoint| {
                checkpoint.parser_version == CLAUDE_PARSER_VERSION
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
        let thread_seed = prep.relative_label.clone();
        let context = if can_append {
            self.load_parse_context(&prep.source_id)
                .await?
                .map(|checkpoint| ClaudeJsonlParseContext {
                    thread_key: checkpoint.thread_key,
                    project_key: checkpoint.project_key,
                    project_label: checkpoint.project_label,
                })
                .unwrap_or_else(|| {
                    ClaudeJsonlParseContext::new(&thread_seed, prep.project_seed.as_deref())
                })
        } else {
            ClaudeJsonlParseContext::new(&thread_seed, prep.project_seed.as_deref())
        };
        if existing.is_none() {
            self.begin_source_file_checkpoint(
                &prep.source_id,
                &root_id,
                &prep.relative_label,
                &prep.identity,
                false,
                CLAUDE_PARSER_VERSION,
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
            parse_claude_batches(ClaudeParseRequest {
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
            LocalError::new(
                LocalErrorKind::SourceUnavailable,
                "Claude parse worker lost",
            )
        })??;
        for batch in &mut parsed.batches {
            retain_ingestable_calls(batch, ingest_since_epoch_ms);
        }
        let context = parsed.context;
        let report = parsed.report;
        let mut added_calls = 0_u64;
        let mut database_regressions = 0_u64;
        for mut batch in parsed.batches {
            let outcome = self
                .insert_claude_usage_batch(&prep.source_id, generation, &mut batch)
                .await?;
            added_calls = added_calls.saturating_add(outcome.added);
            database_regressions = database_regressions.saturating_add(outcome.regressed);
        }
        let mut warnings = report.warnings;
        warnings.regressed_observations = warnings
            .regressed_observations
            .saturating_add(database_regressions);
        if report.cancelled && rebuilt && existing.is_some() {
            self.discard_generation(&prep.source_id, generation).await?;
            return Ok(ClaudeFileIndexOutcome {
                source_id: prep.source_id,
                added_calls: 0,
                rebuilt,
                unchanged: false,
                warnings,
                trailing_bytes: report.checkpoint.trailing_bytes,
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
            CLAUDE_PARSER_VERSION,
            generation,
            &SourceParseCheckpoint {
                thread_key: context.thread_key,
                project_key: context.project_key,
                project_label: context.project_label,
                thread_label: None,
                model: None,
                reasoning_effort: None,
                call_sequence: 0,
                adapter_state: None,
            },
        )
        .await?;
        Ok(ClaudeFileIndexOutcome {
            source_id: prep.source_id,
            added_calls,
            rebuilt,
            unchanged: false,
            warnings,
            trailing_bytes: report.checkpoint.trailing_bytes,
            cancelled: report.cancelled,
        })
    }
}

/// 在 blocking 岛内打开、校验并计算 Claude transcript 身份。
fn prepare_claude_identity(
    file_path: PathBuf,
    root_path: PathBuf,
    root_id: String,
) -> Result<ClaudeIdentityPrep, LocalError> {
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
            "Claude transcript source changed during validation",
        ));
    }
    let normalized_path = fs::canonicalize(&file_path)?;
    validate_transcript_path(&root_path, &normalized_path)?;
    let observed_size = metadata.len();
    let modified_at_epoch_ms = modified_epoch_ms(&metadata);
    let relative = normalized_path.strip_prefix(&root_path).map_err(|_| {
        LocalError::new(
            LocalErrorKind::InvalidPath,
            "Claude transcript escaped its root",
        )
    })?;
    let relative_label = path_key(relative);
    let source_id = stable_id("claude-source", &format!("{root_id}\u{0}{relative_label}"));
    let identity = file_identity(&mut file)?;
    let project_seed = relative
        .components()
        .nth(1)
        .map(|component| path_key(Path::new(component.as_os_str())));
    Ok(ClaudeIdentityPrep {
        source_id,
        relative_label,
        identity,
        observed_size,
        modified_at_epoch_ms,
        project_seed,
        file,
    })
}

/// 单次 Claude transcript 增量解析所需的全部只读输入；打包成一个请求
/// 类型，避免在 blocking 岛的函数签名里堆砌九个独立参数。
struct ClaudeParseRequest {
    /// 已打开、已定位到续读起点的文件句柄。
    file: File,
    /// 打开时观测到的文件字节数。
    observed_size: u64,
    /// 本次续读的起始字节偏移。
    parse_start: u64,
    /// 单行允许的最大字节数。
    max_line_bytes: usize,
    /// 该来源在数据库中的稳定 ID。
    source_id: String,
    /// 所属数据根 ID。
    root_id: String,
    /// 不含绝对路径的相对展示标签。
    relative_label: String,
    /// 解析所需的散列上下文（线程/项目键等）。
    context: ClaudeJsonlParseContext,
    /// 用于中途响应取消请求的令牌。
    cancellation: CancellationToken,
}

/// 在 blocking 岛内流式解析 Claude transcript 并收集批次。
fn parse_claude_batches(request: ClaudeParseRequest) -> Result<ParsedClaudeBatches, LocalError> {
    let ClaudeParseRequest {
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
    let report = parse_claude_jsonl_stream(
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
    Ok(ParsedClaudeBatches {
        batches,
        context,
        report,
    })
}
