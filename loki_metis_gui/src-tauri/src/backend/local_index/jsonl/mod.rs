//! 在 GUI backend 内实现有界、流式且不保留会话正文的 rollout JSONL 最小解析器。

use std::io::Read;

use loki_metis_core::{
    IncrementalTokenUsageDecision, SessionTokenSnapshot, SourceParseCheckpoint, SourceProvenance,
    TokenUsage, UsageCall, safe_path_basename, safe_thread_title, select_incremental_call_usage,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::discovery::stable_id;
use super::{CancellationToken, LocalError, LocalErrorKind};

mod codec;
mod context;

#[cfg(test)]
pub(crate) use codec::restore_incremental_checkpoint;
#[cfg(test)]
use codec::{IncrementalUsageCheckpoint, parse_rfc3339_millis};
use codec::{
    encode_incremental_checkpoint, normalize_usage, optional_token_fingerprint, parse_timestamp,
};
use context::*;

/// 当前最小 rollout 解析器版本，变更语义时必须递增并触发受影响来源重建。
pub const PARSER_VERSION: u32 = 8;

/// Codex adapter 私有累计检查点前缀；固定宽度序号允许 SQLite 直接取最新值。
const CUMULATIVE_CHECKPOINT_PREFIX: &str = "codex-v8";

/// adapter 私有 parse checkpoint 的 schema 版本；与 rollout parser generation 分开演进。
const ADAPTER_STATE_SCHEMA_VERSION: u8 = 1;

/// `task_started.started_at` 与 owner 创建时刻允许的毫秒误差。
const OWNED_START_TOLERANCE_MS: u64 = 2_000;

/// 单条 JSONL 的默认最大字节数；更大记录会流式丢弃并计入覆盖警告。
pub const DEFAULT_MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;

/// 数字时间戳只接受四位年份 RFC3339 能表达的 Unix 毫秒范围。
const MIN_SUPPORTED_TIMESTAMP_MS: i64 = -62_167_219_200_000;

/// 上界对应 `9999-12-30T22:00:00Z`，即 jiff `Timestamp` 实际可表示的四位年份上限；
/// jiff 的可表示范围比 chrono 略窄（尾部约 26 小时不可表示），
/// 收紧此常量以保持“范围内的值必定可安全转换”这一不变量成立。
const MAX_SUPPORTED_TIMESTAMP_MS: i64 = 253_402_207_200_000;

/// 主动发现只读取候选 rollout 首条记录的固定最大字节数。
pub(crate) const ROLLOUT_SIGNATURE_MAX_BYTES: u64 = 64 * 1024;

/// 保存可安全持久化的 JSONL checkpoint，不保存半行原始字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JsonlCheckpoint {
    /// 从本次 reader 起点开始、截至最后完整换行的可提交字节数。
    pub committed_bytes: u64,
    /// 末尾未闭合行的字节数；下次从其起点重新只读解析。
    pub trailing_bytes: u64,
    /// 标识末尾半行已经超过上限，仍需等换行后才能安全越过。
    pub discarding_oversized_line: bool,
}

/// 汇总格式漂移与受控回退计数，不包含任何原始记录内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JsonlWarningCounts {
    /// 不能解析为 JSON 对象的完整行数量。
    pub malformed_lines: u64,
    /// 超过单行上限并被流式丢弃的完整行数量。
    pub oversized_lines: u64,
    /// 结构有效但不是当前支持事件的行数量。
    pub unknown_events: u64,
    /// Token 事件没有单次事实或可用累计回退的数量。
    pub missing_usage: u64,
    /// Token 字段违反 core 不变量的数量。
    pub invalid_usage: u64,
    /// Token 事件缺失或包含无法解析时间戳的数量。
    pub invalid_timestamps: u64,
    /// 因缺失 `last_token_usage` 而显式使用累计值的数量。
    pub cumulative_fallbacks: u64,
    /// fork/subagent 文件尚未找到可证明的自身开始边界。
    pub unresolved_owned_boundaries: u64,
}

impl JsonlWarningCounts {
    /// 返回会降低覆盖完整度的全部警告数量。
    pub fn total(self) -> u64 {
        self.malformed_lines
            .saturating_add(self.oversized_lines)
            .saturating_add(self.unknown_events)
            .saturating_add(self.missing_usage)
            .saturating_add(self.invalid_usage)
            .saturating_add(self.invalid_timestamps)
            .saturating_add(self.cumulative_fallbacks)
            .saturating_add(self.unresolved_owned_boundaries)
    }
}

/// 描述当前 rollout 是否已经越过可计入自身用量的所有权边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum RolloutOwnershipState {
    /// 尚未观察首条 owner 元数据；兼容无元数据的旧夹具时允许原样解析。
    AwaitingOwnerMetadata,
    /// 已确认根 rollout 或已经越过 fork 自身开始边界。
    Owned,
    /// 已确认 fork/subagent，等待与 owner 创建时刻匹配的自身 `task_started`。
    AwaitingOwnedStart {
        /// owner 首条元数据的创建时刻；缺失时无法安全放行。
        owner_created_at_epoch_ms: Option<i64>,
    },
}

/// SQLite 仅原样保存的 Codex adapter 增量状态，不含正文、路径或原始会话 ID。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedJsonlParseState {
    /// 私有状态 schema，未知版本必须触发来源重建。
    schema_version: u8,
    /// rollout 所有权边界状态。
    ownership: RolloutOwnershipState,
    /// 最近一次可信累计基线，包括 fork 复制前缀内的基线。
    previous_cumulative: Option<TokenUsage>,
    /// 最近一次可信单次 last，用于识别自身边界后的重放。
    previous_last: Option<TokenUsage>,
}

/// 保存跨增量片段需要的会话上下文；除受控展示的 project_label/thread_label
/// 外均是哈希或白名单限制过的内容无关字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlParseContext {
    /// 内容无关的线程键；原始 session ID 不会离开解析器。
    pub thread_key: String,
    /// 从工作目录或项目标识散列得到的匿名项目键。
    pub project_key: Option<String>,
    /// 受控项目路径末段。
    pub project_label: Option<String>,
    /// 可选短线程标题。
    pub thread_label: Option<String>,
    /// 最近安全识别的模型名。
    pub model: Option<String>,
    /// 最近安全识别的推理强度。
    pub reasoning_effort: Option<String>,
    /// 已观察 Token 事件序号，保证缺少上游调用 ID 时仍可稳定区分同文件调用。
    pub call_sequence: u64,
    /// 最近一次已计费累计快照；忽略的估算/same-last 不得写入，否则 last==total 增量会减错基线。
    pub previous_cumulative: Option<TokenUsage>,
    /// 最近一次已计费 `last_token_usage`，用于识别额度快照重放同一 last。
    pub previous_last: Option<TokenUsage>,
    /// 当前文件的 owner 初始化与 fork 自身边界状态。
    ownership: RolloutOwnershipState,
}

impl JsonlParseContext {
    /// 从安全文件标签构造回退线程键，随后可由 session 元数据稳定替换。
    pub fn new(thread_seed: &str) -> Self {
        Self {
            thread_key: stable_id("thread", thread_seed),
            project_key: None,
            project_label: None,
            thread_label: None,
            model: None,
            reasoning_effort: None,
            call_sequence: 0,
            previous_cumulative: None,
            previous_last: None,
            ownership: RolloutOwnershipState::AwaitingOwnerMetadata,
        }
    }

    /// 从 core 原样持久化的安全 checkpoint 恢复完整 adapter 上下文。
    pub(crate) fn from_checkpoint(checkpoint: SourceParseCheckpoint) -> Option<Self> {
        let persisted =
            serde_json::from_str::<PersistedJsonlParseState>(checkpoint.adapter_state.as_deref()?)
                .ok()?;
        if persisted.schema_version != ADAPTER_STATE_SCHEMA_VERSION {
            return None;
        }
        Some(Self {
            thread_key: checkpoint.thread_key,
            project_key: checkpoint.project_key,
            project_label: checkpoint.project_label,
            thread_label: checkpoint.thread_label,
            model: checkpoint.model,
            reasoning_effort: checkpoint.reasoning_effort,
            call_sequence: checkpoint.call_sequence,
            previous_cumulative: persisted.previous_cumulative,
            previous_last: persisted.previous_last,
            ownership: persisted.ownership,
        })
    }

    /// 编码可安全持久化的 adapter 私有状态；序列化失败时拒绝提交 checkpoint。
    pub(crate) fn adapter_state(&self) -> Result<String, LocalError> {
        serde_json::to_string(&PersistedJsonlParseState {
            schema_version: ADAPTER_STATE_SCHEMA_VERSION,
            ownership: self.ownership.clone(),
            previous_cumulative: self.previous_cumulative.clone(),
            previous_last: self.previous_last.clone(),
        })
        .map_err(|_| {
            LocalError::new(
                LocalErrorKind::Database,
                "rollout parser state encoding failed",
            )
        })
    }

    /// 返回当前 token_count 是否属于本 rollout 自身区段。
    fn owns_current_segment(&self) -> bool {
        matches!(
            self.ownership,
            RolloutOwnershipState::AwaitingOwnerMetadata | RolloutOwnershipState::Owned
        )
    }

    /// 返回当前是否仍等待 fork 自身开始边界。
    pub(crate) fn awaits_owned_start(&self) -> bool {
        matches!(
            self.ownership,
            RolloutOwnershipState::AwaitingOwnedStart { .. }
        )
    }
}

/// 描述一次流式解析的可持久化结果和覆盖质量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlParseReport {
    /// 成功规范化并交给索引 sink 的调用数量。
    pub calls_emitted: u64,
    /// reader 实际消耗的字节数；checkpoint 只提交完整行。
    pub bytes_consumed: u64,
    /// 不含原始半行的增量 checkpoint。
    pub checkpoint: JsonlCheckpoint,
    /// 各类格式或质量警告计数。
    pub warnings: JsonlWarningCounts,
    /// 标识解析是否在取消请求后提前停止。
    pub cancelled: bool,
    /// token_count 的会话 cumulative 快照（含 IgnoreNonIncremental）。
    pub snapshots: Vec<SessionTokenSnapshot>,
}

/// 对任意只读字节流执行有界 JSONL 解析，并逐调用交给流式 sink。
// 整体是一个手写的“逐字节扫描 + 状态机”实现，刻意不用
// `BufRead::read_line` 之类的高层 API，原因有二：
//   1. 需要精确控制单行内存上限（`max_line_bytes`）——遇到异常巨大的
//      单行时主动丢弃而不是无限增长缓冲区，防止恶意/损坏文件耗尽内存；
//   2. 需要在任意读取间隙检查取消标志（`cancellation`），
//      让用户点“取消扫描”后能在很短时间内响应，而不必等一整个大文件读完。
// `R: Read` 泛型让这个函数既能处理真实文件，也能在测试里处理内存中的
// `&[u8]`，不关心底层是什么具体的字节来源。
pub fn parse_jsonl_stream<R, F>(
    mut reader: R,
    max_line_bytes: usize,
    context: &mut JsonlParseContext,
    provenance: &SourceProvenance,
    cancellation: &CancellationToken,
    mut on_call: F,
) -> Result<JsonlParseReport, LocalError>
where
    R: Read,
    F: FnMut(UsageCall) -> Result<(), LocalError>,
{
    let mut chunk = [0_u8; 16 * 1024]; // 每次系统调用读取的固定大小缓冲区
    let mut line = Vec::with_capacity(max_line_bytes.min(16 * 1024)); // 当前正在累积的一行
    let mut line_bytes = 0_u64; // 当前行的真实字节数（可能超过 line 的容量，用于统计）
    let mut bytes_consumed = 0_u64; // 本次调用总共读取的字节数（含尚未提交的半行）
    let mut committed_bytes = 0_u64; // 已确认到完整换行为止、可安全写入 checkpoint 的字节数
    let mut oversized = false; // 当前行是否已超过上限、正在被丢弃
    let mut warnings = JsonlWarningCounts::default();
    let mut calls_emitted = 0_u64;
    let mut cancelled = false;
    let mut snapshots = Vec::new();

    // 外层 'read 循环：每轮从底层 reader 读一块（chunk）字节；
    // 内层 for 循环：逐字节扫描这块数据，用 `\n` 切分成一行行处理，
    // 这样即使一行跨越了多个 chunk 边界也能正确累积。
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
                // 遇到换行符，说明这一行已经完整：
                //   - 如果它已经因为超长被标记 oversized，只计一次警告并丢弃，
                //     不去解析已经被清空的 line 缓冲区；
                //   - 否则去掉可能的尾随 `\r`（兼容 CRLF 换行）后真正解析这一行。
                if oversized {
                    warnings.oversized_lines = warnings.oversized_lines.saturating_add(1);
                } else {
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    match parse_complete_line(&line, context, provenance) {
                        LineOutcome::Call(call, used_cumulative_fallback, snapshot) => {
                            if used_cumulative_fallback {
                                warnings.cumulative_fallbacks =
                                    warnings.cumulative_fallbacks.saturating_add(1);
                            }
                            if let Some(snapshot) = snapshot {
                                snapshots.push(*snapshot);
                            }
                            on_call(*call)?;
                            calls_emitted = calls_emitted.saturating_add(1);
                        }
                        LineOutcome::Context => {}
                        LineOutcome::Ignored => {}
                        LineOutcome::Snapshot(snapshot) => {
                            snapshots.push(*snapshot);
                        }
                        LineOutcome::Unknown => {
                            warnings.unknown_events = warnings.unknown_events.saturating_add(1);
                        }
                        LineOutcome::Malformed => {
                            warnings.malformed_lines = warnings.malformed_lines.saturating_add(1);
                        }
                        LineOutcome::MissingUsage => {
                            warnings.missing_usage = warnings.missing_usage.saturating_add(1);
                        }
                        LineOutcome::InvalidUsage => {
                            warnings.invalid_usage = warnings.invalid_usage.saturating_add(1);
                        }
                        LineOutcome::InvalidTimestamp => {
                            warnings.invalid_timestamps =
                                warnings.invalid_timestamps.saturating_add(1);
                        }
                    }
                }
                line.clear();
                line_bytes = 0;
                oversized = false;
                // 只有处理完一条完整行之后，才把 checkpoint 的“已提交字节数”
                // 推进到当前位置——这保证了 checkpoint 永远落在完整行边界上，
                // 下次增量扫描从这里续读不会切断一行导致解析错误。
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
                    // 超过单行上限：清空已缓冲内容释放内存，并标记 oversized，
                    // 后续字节直接被忽略，直到遇到换行符才重新开始累积下一行。
                    line.clear();
                    oversized = true;
                }
            }
        }
    }

    if !cancelled && context.awaits_owned_start() {
        warnings.unresolved_owned_boundaries = 1;
    }

    Ok(JsonlParseReport {
        calls_emitted,
        bytes_consumed,
        checkpoint: JsonlCheckpoint {
            committed_bytes,
            trailing_bytes: line_bytes,
            discarding_oversized_line: oversized,
        },
        warnings,
        cancelled,
        snapshots,
    })
}

/// 表示一条完整记录对流式解析状态产生的最小结果。
enum LineOutcome {
    /// 规范化出一个调用，并标识是否使用累计回退；可附带 cumulative 快照。
    Call(Box<UsageCall>, bool, Option<Box<SessionTokenSnapshot>>),
    /// 仅保存 cumulative 快照（IgnoreNonIncremental 但仍有 cumulative）。
    Snapshot(Box<SessionTokenSnapshot>),
    /// 更新了 session、模型、推理或项目上下文。
    Context,
    /// 已确认是正常但不参与用量统计的 rollout 记录。
    Ignored,
    /// 当前解析器不识别该事件。
    Unknown,
    /// 该行不是有效 JSON 对象。
    Malformed,
    /// Token 事件缺失可用的单次和累计事实。
    MissingUsage,
    /// Token 字段违反 core 不变量。
    InvalidUsage,
    /// Token 事件缺失或包含无法解析的发生时间。
    InvalidTimestamp,
}

/// 只反序列化一条完整记录中的允许字段，所有正文和工具字段由 serde 忽略。
// 这是单行 JSONL 记录的核心路由函数，按事件种类分流到不同结果：
//   1. 反序列化失败 -> Malformed；
//   2. 已知的非用量事件（如纯文本消息、工具调用正文）-> Ignored
//      （明确识别但业务上不关心，区别于“完全不认识”）；
//   3. `session_meta`/`turn_context` -> 更新解析上下文（模型、项目等），
//      不产生调用记录；
//   4. `token_count` -> 真正的用量事件，继续往下解析 Token 字段；
//   5. 其他未知种类 -> Unknown（用于统计格式漂移，可能是上游新增了
//      本解析器还不认识的事件类型）。
// 之所以要严格区分 Ignored 与 Unknown，是为了让“新版本 Codex 输出了
// 全新事件格式”这种情况能被统计出来，而不是和“正常的非用量事件”混在一起。
fn parse_complete_line(
    line: &[u8],
    context: &mut JsonlParseContext,
    provenance: &SourceProvenance,
) -> LineOutcome {
    if line.is_empty() {
        return LineOutcome::Unknown;
    }
    // `RolloutEnvelope`/`RolloutPayload` 结构体（见下文 `#[derive(Deserialize)]`）
    // 只声明了业务需要的少数字段，serde 默认会忽略 JSON 里其余的所有字段
    // （包括提示词、回答正文等敏感内容）——“只反序列化白名单字段”本身
    // 就是本解析器不留存会话正文的关键手段。
    let envelope = match serde_json::from_slice::<RolloutEnvelope>(line) {
        Ok(envelope) => envelope,
        Err(_) => return LineOutcome::Malformed,
    };
    let envelope_kind = envelope.kind.as_deref();
    let payload = match envelope.payload {
        Some(payload) => payload,
        None if is_known_non_usage_envelope(envelope_kind) => return LineOutcome::Ignored,
        None => return LineOutcome::Unknown,
    };
    let payload_kind = payload.kind.as_deref();
    // 事件类型可能记录在信封层或 payload 层，任一层给出即可。
    let event_kind = payload_kind.or(envelope_kind);

    if event_kind == Some("session_meta") {
        initialize_owner_context(
            context,
            &payload,
            parse_timestamp(envelope.timestamp.as_ref())
                .or_else(|| parse_timestamp(payload.timestamp.as_ref())),
        );
        return LineOutcome::Context;
    }
    if event_kind == Some("turn_context") {
        if context.owns_current_segment() {
            update_dynamic_context(context, &payload);
        }
        return LineOutcome::Context;
    }
    if event_kind == Some("task_started") {
        open_owned_segment_if_matching(context, parse_timestamp(payload.started_at.as_ref()));
        return LineOutcome::Context;
    }
    if event_kind == Some("thread_name_updated") {
        if context.owns_current_segment() {
            update_thread_title(context, &payload);
        }
        return LineOutcome::Context;
    }
    if event_kind != Some("token_count") {
        if is_known_non_usage_event(envelope_kind, payload_kind) {
            return LineOutcome::Ignored;
        }
        return LineOutcome::Unknown;
    }

    let owned_segment = context.owns_current_segment();
    if owned_segment {
        update_dynamic_context(context, &payload);
        context.call_sequence = context.call_sequence.saturating_add(1);
    }
    let info = match payload.info {
        Some(info) => info,
        None => return LineOutcome::MissingUsage,
    };
    let last = match info.last_token_usage.map(normalize_usage).transpose() {
        Ok(last) => last,
        Err(()) => return LineOutcome::InvalidUsage,
    };
    let cumulative = match info.total_token_usage.map(normalize_usage).transpose() {
        Ok(cumulative) => cumulative,
        Err(()) => return LineOutcome::InvalidUsage,
    };
    let occurred_at_epoch_ms = match parse_timestamp(envelope.timestamp.as_ref())
        .or_else(|| parse_timestamp(payload.timestamp.as_ref()))
    {
        Some(timestamp) => timestamp,
        None => return LineOutcome::InvalidTimestamp,
    };
    let decision = match select_incremental_call_usage(
        last.clone(),
        cumulative.clone(),
        context.previous_cumulative.as_ref(),
        context.previous_last.as_ref(),
    ) {
        Ok(decision) => decision,
        Err(_) => return LineOutcome::MissingUsage,
    };
    if !owned_segment {
        if matches!(decision, IncrementalTokenUsageDecision::Emit(_)) {
            update_incremental_baseline(context, last, cumulative);
        }
        return LineOutcome::Ignored;
    }
    let snapshot = cumulative.as_ref().map(|usage| {
        Box::new(session_token_snapshot(
            context,
            occurred_at_epoch_ms,
            usage.total_tokens,
            payload
                .call_id
                .as_deref()
                .or(payload.turn_id.as_deref())
                .or(payload.request_id.as_deref()),
            provenance,
        ))
    });
    let selected = match decision {
        IncrementalTokenUsageDecision::Emit(selected) => selected,
        IncrementalTokenUsageDecision::IgnoreNonIncremental => {
            // 重放、same-last 与 total-only 估算都不是账单事实，累计基线保持上次已发出调用。
            // 兼容快照仍可落库供历史诊断，但它不再覆盖 ADR-104 调用总量。
            return match snapshot {
                Some(snapshot) => LineOutcome::Snapshot(snapshot),
                None => LineOutcome::Ignored,
            };
        }
    };
    update_incremental_baseline(context, last, cumulative.clone());
    let adapter_consistency_key = cumulative.as_ref().map(|usage| {
        encode_incremental_checkpoint(context.call_sequence, usage, context.previous_last.as_ref())
    });
    // `logical_call_id` 是 aggregate.rs 里去重合并的关键（同一次真实调用
    // 无论出现在活动会话还是归档备份里，指纹都必须一致）。三级优先级：
    //   1. 上游给了明确的 call_id/turn_id/request_id -> 直接用它拼指纹，
    //      最可靠；
    //   2. 用了“累计回退”（没有单次 last_token_usage，只能用整段累计值）
    //      -> 固定用一个特殊后缀，因为累计值本身没有天然的“单次”边界；
    //   3. 都没有 -> 退化用“线程键 + 序号 + 时间 + 全部 Token 数值”拼出一个
    //      尽量唯一的合成指纹，只是这种情况下两次几乎相同的调用理论上
    //      可能被误判为同一条（概率很低，属于已知取舍）。
    // `\u{0}`（NUL 字节）作分隔符：普通文本字段几乎不可能包含它，
    // 用来避免像 "ab"+"c" 和 "a"+"bc" 拼接后产生同一个指纹字符串的歧义。
    let explicit_call_id = payload
        .call_id
        .as_deref()
        .or(payload.turn_id.as_deref())
        .or(payload.request_id.as_deref());
    let fingerprint_material = if selected.used_cumulative_fallback {
        format!("{}\u{0}cumulative-fallback", context.thread_key)
    } else {
        match explicit_call_id {
            Some(call_id) => format!("{}\u{0}{call_id}", context.thread_key),
            None => format!(
                "{}\u{0}{}\u{0}{occurred_at_epoch_ms}\u{0}{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
                context.thread_key,
                context.call_sequence,
                selected.usage.input_tokens,
                optional_token_fingerprint(selected.usage.cached_input_tokens),
                selected.usage.output_tokens,
                optional_token_fingerprint(selected.usage.reasoning_output_tokens),
                selected.usage.total_tokens,
            ),
        }
    };

    let logical_call_id = stable_id("call", &fingerprint_material);
    let snapshot = snapshot.map(|mut snapshot| {
        snapshot.logical_call_id = logical_call_id.clone();
        snapshot
    });
    LineOutcome::Call(
        Box::new(UsageCall {
            logical_call_id,
            occurred_at_epoch_ms,
            model: context.model.clone(),
            reasoning_effort: context.reasoning_effort.clone(),
            project_key: context.project_key.clone(),
            thread_key: context.thread_key.clone(),
            project_label: context.project_label.clone(),
            thread_label: context.thread_label.clone(),
            usage: selected.usage,
            adapter_consistency_key,
            confidence: selected.confidence,
            provenance: vec![provenance.clone()],
        }),
        selected.used_cumulative_fallback,
        snapshot,
    )
}

/// 为 token_count 的 cumulative.total 构造会话快照；Ignore 与 Emit 共用。
fn session_token_snapshot(
    context: &JsonlParseContext,
    occurred_at_epoch_ms: i64,
    cumulative_total_tokens: u64,
    explicit_call_id: Option<&str>,
    provenance: &SourceProvenance,
) -> SessionTokenSnapshot {
    let fingerprint_material = match explicit_call_id {
        Some(call_id) => format!("{0}\u{0}{call_id}\u{0}snapshot", context.thread_key),
        None => format!(
            "{0}\u{0}snapshot\u{0}{1}\u{0}{2}\u{0}{3}",
            context.thread_key,
            context.call_sequence,
            occurred_at_epoch_ms,
            cumulative_total_tokens
        ),
    };
    SessionTokenSnapshot {
        thread_key: context.thread_key.clone(),
        occurred_at_epoch_ms,
        cumulative_total_tokens,
        logical_call_id: stable_id("snap", &fingerprint_material),
        model: context.model.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        project_key: context.project_key.clone(),
        provenance: vec![provenance.clone()],
    }
}

/// 验证未知候选 rollout 的首条记录具有真实 session 元数据最小结构。
// 这个函数不是用来解析真实数据的，而是“数据源发现”阶段的签名校验：
// 遇到一个陌生目录/文件时，只读它的第一行，检查是不是长得像 Codex
// rollout 文件（`session_meta` 事件 + 合法时间戳 + 非空 session id），
// 从而拒绝把普通 JSONL 文件、日志文件误认成用量数据源，同时避免解析
// 完整文件带来的开销。
pub(crate) fn is_rollout_signature_line(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty()
        || u64::try_from(line.len()).unwrap_or(u64::MAX) > ROLLOUT_SIGNATURE_MAX_BYTES
    {
        return false;
    }
    let Ok(envelope) = serde_json::from_slice::<RolloutEnvelope>(line) else {
        return false;
    };
    if envelope.kind.as_deref() != Some("session_meta")
        || parse_timestamp(envelope.timestamp.as_ref()).is_none()
    {
        return false;
    }
    envelope.payload.is_some_and(|payload| {
        payload
            .session_id
            .as_deref()
            .or(payload.id.as_deref())
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 512)
    })
}

/// 只声明 JSONL 顶层时间、事件类型与最小 payload。
#[derive(Deserialize)]
struct RolloutEnvelope {
    /// 该行事件的时间戳原始值；格式在别处校验。
    #[serde(default)]
    timestamp: Option<Value>,
    /// 事件类型标签。
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// 事件正文；只解析索引需要的最小字段集合。
    #[serde(default)]
    payload: Option<RolloutPayload>,
}

/// 只声明 session、turn 与 token_count 所需字段，其余正文自动忽略。
#[derive(Deserialize)]
struct RolloutPayload {
    /// 正文的事件子类型。
    #[serde(rename = "type", default)]
    kind: Option<String>,
    /// `token_count` 事件携带的 Token 计数信息。
    #[serde(default)]
    info: Option<TokenCountInfo>,
    /// 会话元数据 ID。
    #[serde(default)]
    id: Option<String>,
    /// 会话 ID。
    #[serde(default, alias = "sessionId")]
    session_id: Option<String>,
    /// fork 来源标记；只判断是否存在，不读取或保存原始 ID。
    #[serde(default)]
    forked_from_id: Option<Value>,
    /// subagent 路径标记；只判断是否存在，不读取或保存原始路径。
    #[serde(default)]
    agent_path: Option<Value>,
    /// 单次调用 ID。
    #[serde(default, alias = "callId")]
    call_id: Option<String>,
    /// 所属回合 ID。
    #[serde(default, alias = "turnId")]
    turn_id: Option<String>,
    /// 底层请求 ID。
    #[serde(default, alias = "requestId")]
    request_id: Option<String>,
    /// 原始项目 ID；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default, alias = "projectId")]
    project_id: Option<String>,
    /// 原始项目键；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default, alias = "projectKey")]
    project_key: Option<String>,
    /// 原始工作目录；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default)]
    cwd: Option<String>,
    /// 智能体昵称。
    #[serde(default, alias = "agentNickname")]
    agent_nickname: Option<String>,
    /// 线程展示名。
    #[serde(default, alias = "threadName")]
    thread_name: Option<String>,
    /// 会话名。
    #[serde(default)]
    name: Option<String>,
    /// 模型名。
    #[serde(default, alias = "modelName")]
    model: Option<String>,
    /// 推理强度标签。
    #[serde(default, alias = "reasoningEffort", alias = "effort")]
    reasoning_effort: Option<String>,
    /// payload 内嵌的时间戳原始值。
    #[serde(default)]
    timestamp: Option<Value>,
    /// `task_started` 声明的任务起点，用于匹配当前 rollout 自身边界。
    #[serde(default, alias = "startedAt")]
    started_at: Option<Value>,
}

/// 只声明单次事实和累计回退两个 Token 对象。
#[derive(Deserialize)]
struct TokenCountInfo {
    /// 本次单条事实的 Token 用量。
    #[serde(default, alias = "lastTokenUsage")]
    last_token_usage: Option<RawTokenUsage>,
    /// 累计回退用的 Token 用量，`last_token_usage` 缺失时使用。
    #[serde(default, alias = "totalTokenUsage")]
    total_token_usage: Option<RawTokenUsage>,
}

/// 只声明允许进入聚合索引的 Token 数值字段。
#[derive(Deserialize)]
struct RawTokenUsage {
    /// 全部输入 Token（含缓存）。
    #[serde(default, alias = "inputTokens")]
    input_tokens: Option<u64>,
    /// 缓存命中的输入 Token。
    #[serde(default, alias = "cachedInputTokens")]
    cached_input_tokens: Option<u64>,
    /// 缓存写入的输入 Token。
    #[serde(default, alias = "cacheWriteInputTokens")]
    cache_write_input_tokens: Option<u64>,
    /// 输出 Token。
    #[serde(default, alias = "outputTokens")]
    output_tokens: Option<u64>,
    /// 推理输出 Token。
    #[serde(default, alias = "reasoningOutputTokens")]
    reasoning_output_tokens: Option<u64>,
    /// 单次总 Token；缺失时由输入输出计算。
    #[serde(default, alias = "totalTokens")]
    total_tokens: Option<u64>,
}

#[cfg(test)]
mod ownership_tests;

#[cfg(test)]
mod utc_day_tests;

#[cfg(test)]
mod tests;
