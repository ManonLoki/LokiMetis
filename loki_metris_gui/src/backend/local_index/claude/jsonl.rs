//! Claude Code transcript 的有界流式解析器；只反序列化用量事实，不接触消息正文。

use std::io::Read;

use jiff::Timestamp;
use loki_metris_core::{
    Confidence, SourceClientKind, SourceProvenance, TokenUsage, UsageCall,
    claude_project_display_label, safe_model_label, safe_technical_label,
};
use serde::Deserialize;

use super::super::discovery::stable_id;
use super::super::{CancellationToken, LocalError};

/// Claude transcript 独立解析语义版本；必须等于 core 读写 generation。
pub const CLAUDE_PARSER_VERSION: u32 = SourceClientKind::ClaudeCode.parser_version();

/// 保存增量扫描可安全提交的完整行偏移，不保留半行字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaudeJsonlCheckpoint {
    /// 已确认可提交的完整行字节数（不含尾部半行）。
    pub committed_bytes: u64,
    /// 当前缓冲中尚未凑成完整行的尾部字节数。
    pub trailing_bytes: u64,
    /// 标识尾部正在丢弃一条超长行的剩余内容。
    pub discarding_oversized_line: bool,
}

/// 仅包含格式与数据质量计数，不包含 transcript 内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClaudeJsonlWarningCounts {
    /// 无法解析为合法 JSON 的行数。
    pub malformed_lines: u64,
    /// 超出单行字节上限而被丢弃的行数。
    pub oversized_lines: u64,
    /// 未识别事件类型的行数。
    pub unknown_events: u64,
    /// 缺少判定用量所需字段的行数。
    pub missing_usage: u64,
    /// 用量字段存在但数值不合法的行数。
    pub invalid_usage: u64,
    /// 时间戳缺失或无法解析的行数。
    pub invalid_timestamps: u64,
    /// 因数值回退被拒绝合并的观察次数。
    pub regressed_observations: u64,
}

impl ClaudeJsonlWarningCounts {
    /// 汇总全部质量告警计数，供覆盖状态判定使用。
    pub fn total(self) -> u64 {
        self.malformed_lines
            .saturating_add(self.oversized_lines)
            .saturating_add(self.unknown_events)
            .saturating_add(self.missing_usage)
            .saturating_add(self.invalid_usage)
            .saturating_add(self.invalid_timestamps)
            .saturating_add(self.regressed_observations)
    }
}

/// 跨增量片段持久化的安全上下文；所有原始 ID 与项目目录名只保留散列键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeJsonlParseContext {
    /// 由原始线程种子派生出的稳定散列键。
    pub thread_key: String,
    /// 由原始项目种子派生出的稳定散列键；无项目关联时为空。
    pub project_key: Option<String>,
    /// 安全展示用的项目标签；不含原始项目路径。
    pub project_label: Option<String>,
}

impl ClaudeJsonlParseContext {
    /// 从原始线程/项目种子派生出散列键与安全展示标签。
    pub fn new(thread_seed: &str, project_seed: Option<&str>) -> Self {
        Self {
            thread_key: stable_id("claude-thread", thread_seed),
            project_key: project_seed.map(|value| stable_id("claude-project", value)),
            project_label: project_seed.and_then(claude_project_display_label),
        }
    }
}

/// 一次流式解析的安全结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeJsonlParseReport {
    /// 本次流式解析实际发出的调用条数。
    pub calls_emitted: u64,
    /// 本次调用读取的原始字节总数。
    pub bytes_consumed: u64,
    /// 可安全用于下次增量续读的行边界检查点。
    pub checkpoint: ClaudeJsonlCheckpoint,
    /// 本次解析累计的格式与数据质量告警。
    pub warnings: ClaudeJsonlWarningCounts,
    /// 标识解析是否因取消信号提前终止。
    pub cancelled: bool,
}

/// transcript 单行记录只反序列化的最小字段集合，缺失字段一律容忍为 `None`。
#[derive(Deserialize)]
struct TranscriptRecord {
    /// 事件类型标签；只有 `"assistant"` 才可能是用量记录。
    #[serde(rename = "type")]
    kind: Option<String>,
    /// 会话 ID。
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// 记录时间戳原始字符串。
    timestamp: Option<String>,
    /// 承载用量事实的消息片段。
    message: Option<AssistantMessage>,
    /// 助手用量事件上的推理强度；缺失时保持未提供。
    #[serde(default, alias = "reasoning_effort", alias = "reasoningEffort")]
    effort: Option<String>,
}

/// 记录中承载用量事实的 assistant 消息片段。
#[derive(Deserialize)]
struct AssistantMessage {
    /// 消息 ID。
    id: Option<String>,
    /// 模型名。
    model: Option<String>,
    /// Token 用量字段。
    usage: Option<AssistantUsage>,
}

/// 折算 Token 用量所需的原始字段。
#[derive(Deserialize)]
struct AssistantUsage {
    /// 输入 Token。
    input_tokens: Option<u64>,
    /// 输出 Token。
    output_tokens: Option<u64>,
    /// 缓存读取的输入 Token。
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    /// 缓存写入的输入 Token。
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

/// 单行解析后的稳定分类结果，供逐类质量计数使用。
enum LineOutcome {
    /// 解析出一条可用的用量调用。
    Call(Box<UsageCall>),
    /// 已知的非用量事件类型，安全忽略。
    Ignored,
    /// 未识别的事件类型。
    Unknown,
    /// 行内容不是合法 JSON。
    Malformed,
    /// 缺少判定用量所需的必要字段。
    MissingUsage,
    /// 用量字段存在但数值不合法（如溢出）。
    InvalidUsage,
    /// 时间戳缺失或无法解析。
    InvalidTimestamp,
}

/// 流式读取 Claude transcript；相邻同响应的逐步增长观察只向 sink 发送最后一条。
///
/// 非相邻或跨增量片段的相同逻辑调用由索引层使用同一 `logical_call_id` 再执行
/// 单调 upsert，避免用文件级无界内存换取去重。
pub fn parse_claude_jsonl_stream<R, F>(
    mut reader: R,
    max_line_bytes: usize,
    context: &mut ClaudeJsonlParseContext,
    provenance: &SourceProvenance,
    cancellation: &CancellationToken,
    mut on_call: F,
) -> Result<ClaudeJsonlParseReport, LocalError>
where
    R: Read,
    F: FnMut(UsageCall) -> Result<(), LocalError>,
{
    let mut chunk = [0_u8; 16 * 1024];
    let mut line = Vec::with_capacity(max_line_bytes.min(16 * 1024));
    let mut line_bytes = 0_u64;
    let mut bytes_consumed = 0_u64;
    let mut committed_bytes = 0_u64;
    let mut oversized = false;
    let mut warnings = ClaudeJsonlWarningCounts::default();
    let mut pending = None::<UsageCall>;
    let mut calls_emitted = 0_u64;
    let mut cancelled = false;

    'read: loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        for byte in &chunk[..read] {
            bytes_consumed = bytes_consumed.saturating_add(1);
            if *byte == b'\n' {
                if oversized {
                    warnings.oversized_lines = warnings.oversized_lines.saturating_add(1);
                } else {
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    match parse_line(&line, context, provenance) {
                        LineOutcome::Call(call) => {
                            merge_or_emit(
                                *call,
                                &mut pending,
                                &mut warnings,
                                &mut calls_emitted,
                                &mut on_call,
                            )?;
                        }
                        LineOutcome::Ignored => {}
                        LineOutcome::Unknown => {
                            warnings.unknown_events = warnings.unknown_events.saturating_add(1)
                        }
                        LineOutcome::Malformed => {
                            warnings.malformed_lines = warnings.malformed_lines.saturating_add(1)
                        }
                        LineOutcome::MissingUsage => {
                            warnings.missing_usage = warnings.missing_usage.saturating_add(1)
                        }
                        LineOutcome::InvalidUsage => {
                            warnings.invalid_usage = warnings.invalid_usage.saturating_add(1)
                        }
                        LineOutcome::InvalidTimestamp => {
                            warnings.invalid_timestamps =
                                warnings.invalid_timestamps.saturating_add(1)
                        }
                    }
                }
                line.clear();
                line_bytes = 0;
                oversized = false;
                committed_bytes = bytes_consumed;
                if cancellation.is_cancelled() {
                    cancelled = true;
                    break 'read;
                }
                continue;
            }
            line_bytes = line_bytes.saturating_add(1);
            if !oversized {
                if line.len() < max_line_bytes {
                    line.push(*byte);
                } else {
                    line.clear();
                    oversized = true;
                }
            }
        }
    }

    if let Some(call) = pending {
        on_call(call)?;
        calls_emitted = calls_emitted.saturating_add(1);
    }
    Ok(ClaudeJsonlParseReport {
        calls_emitted,
        bytes_consumed,
        checkpoint: ClaudeJsonlCheckpoint {
            committed_bytes,
            trailing_bytes: line_bytes,
            discarding_oversized_line: oversized,
        },
        warnings,
        cancelled,
    })
}

/// 合并相邻同一逻辑调用的连续观察，只在遇到不同调用时才真正 emit 前一条。
// `pending` 是一个只装“最多一条”的前瞻缓冲区，不是无界的去重 Map：
// Claude transcript 文件里，同一条 assistant 消息在流式生成过程中会
// 被连续写入多行、一行比一行更完整。merge_or_emit 每次只跟“刚缓存的
// 上一条”比较——如果新记录和上一条是同一个 logical_call_id，就地合并
// （保留其中数值更完整/单调的那份，不立即向下游发送）；一旦遇到不同
// 的 logical_call_id，说明连续重复观察已经结束，才把缓存的那条真正
// emit 出去。这样只用 O(1) 额外内存就能压掉“连续相邻重复”的绝大多数
// 冗余，真正跨行、跨文件的重复交给 call_store.rs 里的单调 upsert 兜底。
fn merge_or_emit<F>(
    call: UsageCall,
    pending: &mut Option<UsageCall>,
    warnings: &mut ClaudeJsonlWarningCounts,
    calls_emitted: &mut u64,
    on_call: &mut F,
) -> Result<(), LocalError>
where
    F: FnMut(UsageCall) -> Result<(), LocalError>,
{
    let Some(previous) = pending.take() else {
        *pending = Some(call);
        return Ok(());
    };
    if previous.logical_call_id != call.logical_call_id {
        on_call(previous)?;
        *calls_emitted = calls_emitted.saturating_add(1);
        *pending = Some(call);
        return Ok(());
    }
    if usage_is_monotonic(&previous, &call) {
        *pending = Some(call);
    } else {
        warnings.regressed_observations = warnings.regressed_observations.saturating_add(1);
        *pending = Some(previous);
    }
    Ok(())
}

/// 判定两次相邻观察在各 Token 计数上是否逐项非递减（即单调增长）。
// 任一侧缺少 cached_input_tokens 时直接判定非单调；merge_or_emit 遇到
// 非单调结果会整体丢弃更新的观察（既不合并也不单独 emit），只保留仍在
// pending 中的上一条，靠 regressed_observations 计数暴露这种丢弃，
// 而不是把字段更少的观察当新调用写入。
fn usage_is_monotonic(previous: &UsageCall, next: &UsageCall) -> bool {
    let (Some(previous_cached), Some(next_cached)) = (
        previous.usage.cached_input_tokens,
        next.usage.cached_input_tokens,
    ) else {
        return false;
    };
    let previous_uncached = previous
        .usage
        .input_tokens
        .checked_sub(previous_cached)
        .and_then(|value| {
            value.checked_sub(previous.usage.cache_write_input_tokens.unwrap_or_default())
        });
    let next_uncached = next
        .usage
        .input_tokens
        .checked_sub(next_cached)
        .and_then(|value| {
            value.checked_sub(next.usage.cache_write_input_tokens.unwrap_or_default())
        });
    previous_uncached
        .zip(next_uncached)
        .is_some_and(|(previous, next)| next >= previous)
        && next.usage.input_tokens >= previous.usage.input_tokens
        && next_cached >= previous_cached
        && next.usage.cache_write_input_tokens.unwrap_or_default()
            >= previous.usage.cache_write_input_tokens.unwrap_or_default()
        && next.usage.output_tokens >= previous.usage.output_tokens
        && next.usage.total_tokens >= previous.usage.total_tokens
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - jsonl_tests.rs：本模块的单元测试
include!("jsonl_tests.rs");
