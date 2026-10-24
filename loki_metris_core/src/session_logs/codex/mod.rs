//! 实现有界、流式且不保留会话正文的 rollout JSONL 最小解析器。

use std::io::Read;

use jiff::Timestamp;
use crate::{
    CodexUsageAuthority, IncrementalTokenUsageDecision, SessionTokenSnapshot, SourceParseCheckpoint,
    SourceProvenance, TokenUsage, UsageCall, safe_path_basename, safe_thread_title,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::stable_id;
use crate::{LocalError, LocalErrorKind, ScanCancellation as CancellationToken};

pub(super) mod codec;
mod context;
mod protocol;
mod response;
mod non_usage_line;
#[cfg(test)]
mod response_tests;
#[cfg(test)]
mod oversized_tests;

use protocol::*;
pub use response::is_response_call;

#[cfg(any(test, feature = "test-support"))]
pub use codec::restore_incremental_checkpoint;
#[cfg(test)]
use codec::{IncrementalUsageCheckpoint, parse_rfc3339_millis};
use codec::{
    encode_incremental_checkpoint, normalize_usage, optional_token_fingerprint, parse_timestamp,
};
use context::*;

/// 当前最小 rollout 解析器版本，变更语义时必须递增并触发受影响来源重建。
pub const PARSER_VERSION: u32 = crate::SourceClientKind::Codex.parser_version();

/// Codex adapter 私有累计检查点前缀；固定宽度序号允许 SQLite 直接取最新值。
const CUMULATIVE_CHECKPOINT_PREFIX: &str = "codex-v9";

/// adapter 私有 parse checkpoint 的 schema 版本；与 rollout parser generation 分开演进。
const ADAPTER_STATE_SCHEMA_VERSION: u8 = 4;

/// `task_started.started_at` 与 owner 创建时刻允许的毫秒误差。
const OWNED_START_TOLERANCE_MS: u64 = 2_000;

/// 单条 JSONL 的默认缓冲上限；超长行只有完整确认非用量路由才能安全跳过。
pub const DEFAULT_MAX_JSONL_LINE_BYTES: usize = 1024 * 1024;

/// 数字时间戳只接受四位年份 RFC3339 能表达的 Unix 毫秒范围。
const MIN_SUPPORTED_TIMESTAMP_MS: i64 = -62_167_219_200_000;

/// 上界对应 `9999-12-30T22:00:00Z`，即 jiff `Timestamp` 实际可表示的四位年份上限；
/// jiff 的可表示范围比 chrono 略窄（尾部约 26 小时不可表示），
/// 收紧此常量以保持“范围内的值必定可安全转换”这一不变量成立。
const MAX_SUPPORTED_TIMESTAMP_MS: i64 = 253_402_207_200_000;

/// 主动发现只读取候选 rollout 首条记录的固定最大字节数。
pub const ROLLOUT_SIGNATURE_MAX_BYTES: u64 = 64 * 1024;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonlWarningCounts {
    /// 不能解析为 JSON 对象的完整行数量。
    pub malformed_lines: u64,
    /// 超过缓冲上限且无法完整确认非用量路由的完整行数量。
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
    /// fork/subagent 存在尚未找到自身开始边界的旧格式用量，无法排除漏账。
    pub unresolved_owned_boundaries: u64,
}

impl JsonlWarningCounts {
    /// 累计已经消费完整行的质量损失；所有权边界由当前状态计算，不能永久累加。
    fn add_consumed_lines(&mut self, warnings: Self) {
        self.malformed_lines = self.malformed_lines.saturating_add(warnings.malformed_lines);
        self.oversized_lines = self.oversized_lines.saturating_add(warnings.oversized_lines);
        self.unknown_events = self.unknown_events.saturating_add(warnings.unknown_events);
        self.missing_usage = self.missing_usage.saturating_add(warnings.missing_usage);
        self.invalid_usage = self.invalid_usage.saturating_add(warnings.invalid_usage);
        self.invalid_timestamps = self.invalid_timestamps.saturating_add(warnings.invalid_timestamps);
        self.cumulative_fallbacks = self.cumulative_fallbacks.saturating_add(warnings.cumulative_fallbacks);
    }

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
    /// 已确认 fork/subagent，等待匹配的 `task_started` 或有效本线程响应。
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
    /// 当前 generation 已消费完整行的质量事实；旧状态缺失时必须重建，不能补零。
    consumed_line_warnings: JsonlWarningCounts,
    /// rollout 所有权边界状态。
    ownership: RolloutOwnershipState,
    /// 有未归属旧用量的前缀；现代响应只证明自身，不能清除先前歧义。
    unresolved_legacy_prefix: bool,
    /// 最近一次可信累计基线，包括 fork 复制前缀内的基线。
    previous_cumulative: Option<TokenUsage>,
    /// 最近一次可信单次 last，用于识别自身边界后的重放。
    previous_last: Option<TokenUsage>,
    /// 当前自身区段的事实来源；旧检查点缺此字段必须重建。
    usage_authority: CodexUsageAuthority,
}

impl PersistedJsonlParseState {
    /// 未知或缺失质量的检查点拒绝恢复，不能把旧数据默认为完整。
    fn decode(state: Option<&str>) -> Option<Self> {
        let persisted: Self = serde_json::from_str(state?).ok()?;
        (persisted.schema_version == ADAPTER_STATE_SCHEMA_VERSION
            && persisted.consumed_line_warnings.unresolved_owned_boundaries == 0)
            .then_some(persisted)
    }
}

/// 将 adapter 私有检查点映射为 core 访问策略所需的通用质量事实。
pub fn persisted_source_quality(state: Option<&str>) -> crate::PersistedSourceQuality {
    let Some(persisted) = PersistedJsonlParseState::decode(state) else {
        return crate::PersistedSourceQuality::Unknown;
    };
    let boundary_warning = u64::from(persisted.unresolved_legacy_prefix);
    crate::PersistedSourceQuality::Known {
        warning_count: persisted.consumed_line_warnings.total().saturating_add(boundary_warning),
    }
}

/// 保存跨增量片段需要的会话上下文；除受控展示的 project_label/thread_label
/// 外均是哈希或白名单限制过的内容无关字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlParseContext {
    /// 当前 generation 已消费完整行的警告，不包含可解除的所有权边界。
    consumed_line_warnings: JsonlWarningCounts,
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
    /// 区分已知零贡献的空 fork 与可能漏记旧用量的前缀，必须跨追加持久化。
    unresolved_legacy_prefix: bool,
    /// 只有有效且属于自身区段的响应记录才能切换权威来源。
    usage_authority: CodexUsageAuthority,
}

impl JsonlParseContext {
    /// 从安全文件标签构造回退线程键，随后可由 session 元数据稳定替换。
    pub fn new(thread_seed: &str) -> Self {
        Self {
            consumed_line_warnings: JsonlWarningCounts::default(),
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
            unresolved_legacy_prefix: false,
            usage_authority: CodexUsageAuthority::default(),
        }
    }

    /// 从 core 原样持久化的安全 checkpoint 恢复完整 adapter 上下文。
    pub fn from_checkpoint(checkpoint: SourceParseCheckpoint) -> Option<Self> {
        let persisted = PersistedJsonlParseState::decode(checkpoint.adapter_state.as_deref())?;
        Some(Self {
            consumed_line_warnings: persisted.consumed_line_warnings,
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
            unresolved_legacy_prefix: persisted.unresolved_legacy_prefix,
            usage_authority: persisted.usage_authority,
        })
    }

    /// 编码可安全持久化的 adapter 私有状态；序列化失败时拒绝提交 checkpoint。
    pub fn adapter_state(&self) -> Result<String, LocalError> {
        serde_json::to_string(&PersistedJsonlParseState {
            schema_version: ADAPTER_STATE_SCHEMA_VERSION,
            consumed_line_warnings: self.consumed_line_warnings,
            ownership: self.ownership.clone(),
            unresolved_legacy_prefix: self.unresolved_legacy_prefix,
            previous_cumulative: self.previous_cumulative.clone(),
            previous_last: self.previous_last.clone(),
            usage_authority: self.usage_authority,
        })
        .map_err(|_| {
            LocalError::new(
                LocalErrorKind::Database,
                "rollout parser state encoding failed",
            )
        })
    }

    /// 将本轮完整行质量与已提交前缀合并，不累计尚未闭合的尾行或可解除边界。
    pub fn record_consumed_line_warnings(&mut self, warnings: JsonlWarningCounts) {
        self.consumed_line_warnings.add_consumed_lines(warnings);
    }

    /// 返回当前 generation 的质量事实，重复扫描不得把它清零。
    pub fn generation_warnings(&self) -> JsonlWarningCounts {
        JsonlWarningCounts {
            unresolved_owned_boundaries: u64::from(self.unresolved_legacy_prefix),
            ..self.consumed_line_warnings
        }
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - jsonl_stream.rs：对任意只读字节流执行有界 JSONL 解析，并逐调用交给流式 sink。；表示一条完整记录对流式解析状态产生的最小结果。；只反序列化一条完整记录中的允许字段，所有正文和工具字段由 serde 忽略。
include!("jsonl_stream.rs");
