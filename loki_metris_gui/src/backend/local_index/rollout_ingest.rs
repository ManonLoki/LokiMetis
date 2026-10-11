//! 增量索引单个已验证 rollout：FS/parse 在 `spawn_blocking` 岛内完成，
//! checkpoint 与批次写入在 async 侧直接 await LocalIndex。

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use loki_metris_core::{
    LocalError, LocalErrorKind, LocalIndex, SourceProvenance, UsageCall, retain_ingestable_calls,
};
use tokio::task::spawn_blocking;

use super::discovery::{DiscoveredRoot, path_key, stable_id};
use super::file_source::{
    ValidatedRolloutFile, modified_epoch_ms, validate_rollout_path,
};
use super::jsonl::{
    JsonlParseContext, JsonlParseReport, JsonlWarningCounts, PARSER_VERSION, is_response_call,
    parse_jsonl_stream,
};
use super::scan::{CancellationToken, ScanConfig};

/// 一次插入批次的目标大小；达到后立即刷新，避免大文件把整批调用都留在内存里。
pub(crate) const INSERT_BATCH_SIZE: usize = 256;

/// 汇总一个 rollout 文件的增量索引结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileIndexOutcome {
    /// adapter 生成的稳定来源 ID。
    pub(crate) source_id: String,
    /// 本次首次写入该来源 generation 的调用数量。
    pub(crate) added_calls: u64,
    /// 本次是否因截断、替换或解析器升级重建该来源。
    pub(crate) rebuilt: bool,
    /// 本次文件没有变化，因而未重新读取。
    pub(crate) unchanged: bool,
    /// 当前 generation 已消费完整行及未解决归属边界的质量警告。
    pub(crate) warnings: JsonlWarningCounts,
    /// checkpoint 后仍未闭合的末行字节数。
    pub(crate) trailing_bytes: u64,
    /// 本次是否响应取消并停在完整行边界。
    pub(crate) cancelled: bool,
}

/// 打开句柄上完成的同步身份探测结果，供 async 侧决定 unchanged / append / rebuild。
struct RolloutIdentityPrep {
    /// 该来源在数据库中的稳定 ID。
    source_id: String,
    /// 不含绝对路径的相对展示标签。
    relative_label: String,
    /// 文件物理身份散列，用于跨增量比对是否被替换。
    file_identity: String,
    /// 打开时观测到的文件字节数。
    observed_size: u64,
    /// 打开时观测到的修改时间。
    modified_at_epoch_ms: i64,
    /// 已打开、已通过校验的文件句柄。
    file: std::fs::File,
}

/// 解析岛产出：批次已按 INSERT_BATCH_SIZE 切分，不含任何 SQLite 写入。
struct ParsedRolloutBatches {
    /// 按批次切分、待写入数据库的调用记录。
    batches: Vec<Vec<UsageCall>>,
    /// 解析过程中产生的散列上下文。
    context: JsonlParseContext,
    /// 本次流式解析的完整报告，含检查点与告警。
    report: JsonlParseReport,
}

/// 为 `LocalIndex` 补充 Codex rollout 文件的增量索引编排。
pub(crate) trait IndexRolloutFile {
    /// 增量索引一个已经通过首记录签名的 rollout 句柄；正文永不进入数据库。
    /// 调用方必须已经在本次扫描中登记过 `root`（每根登记一次即可，
    /// 不要每个文件重复登记，避免每个文件都触发一次多余的事务）。
    async fn index_rollout_file(
        &mut self,
        root: &DiscoveredRoot,
        source: ValidatedRolloutFile,
        archived: bool,
        config: ScanConfig,
        cancellation: &CancellationToken,
    ) -> Result<FileIndexOutcome, LocalError>;
}

impl IndexRolloutFile for LocalIndex {
    /// 增量索引一个已经通过首记录签名的 rollout 句柄；正文永不进入数据库。
    async fn index_rollout_file(
        &mut self,
        root: &DiscoveredRoot,
        source: ValidatedRolloutFile,
        archived: bool,
        config: ScanConfig,
        cancellation: &CancellationToken,
    ) -> Result<FileIndexOutcome, LocalError> {
        let file_path = source.path().to_path_buf();
        validate_rollout_path(root, &file_path, archived)?;
        let root_id = root.root_id.clone();
        let root_path = root.path.clone();
        let root_id_for_prep = root_id.clone();
        let prep = spawn_blocking(move || {
            prepare_rollout_identity(source, root_path, file_path, root_id_for_prep)
        })
        .await
        .map_err(|_| {
            LocalError::new(
                LocalErrorKind::SourceUnavailable,
                "rollout identity worker lost",
            )
        })??;
        let existing = self.stored_source_file(&prep.source_id).await?;
        let previous = existing.as_ref().map(|old| (old.file_identity.clone(), old.parsed_offset.saturating_add(old.trailing_bytes)));
        let probe_cancellation = cancellation.clone();
        let (prep, prefix_matches) = spawn_blocking(move || {
            let mut prep = prep;
            let (identity, matches) = super::source_fingerprint::probe(&mut prep.file, prep.observed_size,
                previous.as_ref().map(|(identity, size)| (identity.as_str(), *size)), &probe_cancellation)?;
            prep.file_identity = identity;
            Ok::<_, LocalError>((prep, matches))
        }).await.map_err(|_| LocalError::new(LocalErrorKind::SourceUnavailable, "rollout fingerprint worker lost"))??;
        let physically_appendable = existing.as_ref().is_some_and(|checkpoint| {
            checkpoint.parser_version == PARSER_VERSION
                && prep.observed_size >= checkpoint.observed_size
                && prefix_matches
                && checkpoint.token_snapshots_ready
        });
        let restored_context = if physically_appendable {
            self.load_parse_context(&prep.source_id)
                .await?
                .and_then(JsonlParseContext::from_checkpoint)
        } else {
            None
        };
        let previous_warnings = restored_context
            .as_ref()
            .map(JsonlParseContext::generation_warnings)
            .unwrap_or_default();
        let can_append =
            !config.force_rebuild && physically_appendable && restored_context.is_some();
        if !cancellation.is_cancelled() && let Some(existing) = &existing
            && existing.observed_size == prep.observed_size
            && existing.modified_at_epoch_ms == prep.modified_at_epoch_ms
            && existing.file_identity == prep.file_identity
            && existing.parsed_offset.checked_add(existing.trailing_bytes)
                == Some(existing.observed_size)
            && can_append
        {
            return Ok(FileIndexOutcome {
                source_id: prep.source_id,
                added_calls: 0,
                rebuilt: false,
                unchanged: true,
                warnings: previous_warnings,
                trailing_bytes: existing.trailing_bytes,
                cancelled: false,
            });
        }
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
            restored_context.expect("appendable source has a decoded parser context")
        } else {
            JsonlParseContext::new(&prep.relative_label)
        };
        if existing.is_none() {
            self.begin_source_file_checkpoint(
                &prep.source_id,
                &root_id,
                &prep.relative_label,
                &prep.file_identity,
                archived,
                PARSER_VERSION,
                &context.thread_key,
                None,
                None,
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
        let fingerprint = prep.file_identity.clone();
        let parsed = spawn_blocking(move || {
            parse_rollout_batches(RolloutParseRequest {
                file: prep.file,
                fingerprint,
                observed_size: prep.observed_size,
                parse_start,
                max_line_bytes: config.max_line_bytes,
                source_id,
                root_id: root_id_for_parse,
                relative_label,
                archived,
                context,
                cancellation: parse_cancellation,
            })
        })
        .await
        .map_err(|_| {
            LocalError::new(
                LocalErrorKind::SourceUnavailable,
                "rollout parse worker lost",
            )
        })??;
        let mut context = parsed.context;
        let mut report = parsed.report;
        context.record_consumed_line_warnings(report.warnings);
        report
            .snapshots
            .retain(|snapshot| snapshot.occurred_at_epoch_ms >= config.ingest_since_epoch_ms);
        let mut added_calls = 0_u64;
        for batch in parsed.batches {
            // 保留文件顺序，仅按 adapter 协议标记拆连续批；core 决定各自写入语义。
            let mut calls = batch.into_iter().peekable();
            while let Some(first) = calls.peek() {
                let observed = is_response_call(first);
                let mut group = Vec::with_capacity(INSERT_BATCH_SIZE);
                while calls
                    .peek()
                    .is_some_and(|call| is_response_call(call) == observed)
                {
                    group.push(calls.next().expect("peeked call exists"));
                }
                let added = if observed {
                    // 现代稳定事实由 core 恢复首次时间后过滤，不能先用重放时间丢弃。
                    self.insert_observed_usage_batch(
                        &prep.source_id, generation, config.ingest_since_epoch_ms, &mut group,
                    ).await?
                } else {
                    retain_ingestable_calls(&mut group, config.ingest_since_epoch_ms);
                    self.insert_usage_batch(&prep.source_id, generation, &mut group).await?
                };
                added_calls = added_calls.saturating_add(added);
            }
        }
        // 转移兼容快照，避免写库期间仍持有全部字符串与 provenance 的深拷贝。
        let mut snapshots = std::mem::take(&mut report.snapshots);
        self.insert_usage_and_snapshots(
            &prep.source_id,
            generation,
            &mut Vec::new(),
            &mut snapshots,
        )
        .await?;
        drop(snapshots);
        if report.cancelled && rebuilt && existing.is_some() {
            self.discard_generation(&prep.source_id, generation).await?;
            return Ok(FileIndexOutcome {
                source_id: prep.source_id,
                added_calls: 0,
                rebuilt,
                unchanged: false,
                warnings: report.warnings,
                trailing_bytes: report.checkpoint.trailing_bytes,
                cancelled: true,
            });
        }
        let parsed_offset = parse_start
            .checked_add(report.checkpoint.committed_bytes)
            .ok_or_else(|| LocalError::new(LocalErrorKind::Overflow, "file offset overflowed"))?;
        let adapter_state = context.adapter_state()?;
        let generation_warnings = context.generation_warnings();
        self.commit_source_file_checkpoint(
            &prep.source_id,
            &root_id,
            &prep.relative_label,
            &prep.file_identity,
            archived,
            prep.observed_size,
            prep.modified_at_epoch_ms,
            parsed_offset,
            report.checkpoint.trailing_bytes,
            report.checkpoint.discarding_oversized_line,
            PARSER_VERSION,
            generation,
            &loki_metris_core::SourceParseCheckpoint {
                thread_key: context.thread_key,
                project_key: context.project_key,
                project_label: context.project_label,
                thread_label: context.thread_label,
                model: context.model,
                reasoning_effort: context.reasoning_effort,
                call_sequence: context.call_sequence,
                adapter_state: Some(adapter_state),
            },
        )
        .await?;
        Ok(FileIndexOutcome {
            source_id: prep.source_id,
            added_calls,
            rebuilt,
            unchanged: false,
            warnings: generation_warnings,
            trailing_bytes: report.checkpoint.trailing_bytes,
            cancelled: report.cancelled,
        })
    }
}

/// 在 blocking 岛内完成相对路径、指纹与打开句柄移交。
fn prepare_rollout_identity(
    source: ValidatedRolloutFile,
    root_path: PathBuf,
    file_path: PathBuf,
    root_id: String,
) -> Result<RolloutIdentityPrep, LocalError> {
    let observed_size = source.metadata().len();
    let modified_at_epoch_ms = modified_epoch_ms(source.metadata());
    let relative = file_path.strip_prefix(&root_path).map_err(|_| {
        LocalError::new(
            LocalErrorKind::InvalidPath,
            "rollout source escaped its root",
        )
    })?;
    let relative_label = path_key(relative);
    let source_id = stable_id("source", &format!("{root_id}\u{0}{relative_label}"));
    Ok(RolloutIdentityPrep {
        source_id,
        relative_label,
        file_identity: String::new(),
        observed_size,
        modified_at_epoch_ms,
        file: source.into_file(),
    })
}

/// 单次 rollout 增量解析所需的全部只读输入；打包成一个请求类型，避免
/// 在 blocking 岛的函数签名里堆砌十个独立参数。
struct RolloutParseRequest {
    /// 已打开、已定位到续读起点的文件句柄。
    file: std::fs::File,
    /// 同句柄解析前的完整身份，提交批次前必须保持一致。
    fingerprint: String,
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
    /// 标识该文件来自归档目录。
    archived: bool,
    /// 解析所需的散列上下文。
    context: JsonlParseContext,
    /// 用于中途响应取消请求的令牌。
    cancellation: CancellationToken,
}

/// 在 blocking 岛内流式解析并按批次收集调用，不触碰 SQLite。
fn parse_rollout_batches(request: RolloutParseRequest) -> Result<ParsedRolloutBatches, LocalError> {
    let RolloutParseRequest {
        mut file,
        fingerprint,
        observed_size,
        parse_start,
        max_line_bytes,
        source_id,
        root_id,
        relative_label,
        archived,
        mut context,
        cancellation,
    } = request;
    file.seek(SeekFrom::Start(parse_start))?;
    let readable_bytes = observed_size.saturating_sub(parse_start);
    let provenance = SourceProvenance {
        source_id: source_id.clone(),
        root_id,
        relative_label,
        archived,
    };
    let mut batch = Vec::<UsageCall>::with_capacity(INSERT_BATCH_SIZE);
    let mut batches = Vec::new();
    let report = parse_jsonl_stream(
        (&mut file).take(readable_bytes),
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
    if !report.cancelled || report.bytes_consumed != 0 {
        super::source_fingerprint::verify(&mut file, &fingerprint)?;
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    Ok(ParsedRolloutBatches {
        batches,
        context,
        report,
    })
}
