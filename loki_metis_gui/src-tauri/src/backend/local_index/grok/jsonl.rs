//! Grok `updates.jsonl` 的有界流式解析器；只反序列化用量与模型/时间元数据。

use std::collections::BTreeMap;
use std::io::Read;

use jiff::Timestamp;
use loki_metis_core::{
    Confidence, SourceClientKind, SourceProvenance, TokenUsage, UsageCall,
    grok_project_display_label, safe_model_label,
};
use serde::Deserialize;

use super::super::discovery::stable_id;
use super::super::{CancellationToken, LocalError};

/// Grok 会话用量独立解析语义版本；必须等于 core 读写 generation。
pub const GROK_PARSER_VERSION: u32 = SourceClientKind::GrokBuildCli.parser_version();

/// 合成夹具：含进行中轮次、两次完成轮次、嵌套 update 以及缺用量行。
#[cfg(test)]
pub const SYNTHETIC_GROK_UPDATES_JSONL: &str = concat!(
    r#"{"sessionUpdate":"turn_started","timestamp":"2026-08-15T12:00:00Z","usage":{"inputTokens":9999,"outputTokens":9999,"totalTokens":19998},"model":"grok-in-progress"}"#,
    "\n",
    r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z","modelUsage":{"grok-4.5-build":{"inputTokens":100,"outputTokens":40,"totalTokens":140,"cachedReadTokens":20,"cacheCreationTokens":5,"reasoningTokens":10}}}"#,
    "\n",
    r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z","usage":{"inputTokens":50,"outputTokens":25,"totalTokens":75},"model":"grok-4.6"}"#,
    "\n",
    r#"{"update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":30,"outputTokens":15,"totalTokens":45,"cachedReadTokens":0}},"timestamp":"2026-08-15T12:00:00Z","model":"grok-4.5-build"}"#,
    "\n",
    r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z"}"#,
    "\n",
);

/// 生产 Grok Build CLI 信封：`turn_completed` 与 usage 位于 `params.update`。
#[cfg(test)]
pub const PRODUCTION_GROK_SESSION_ENVELOPE_JSONL: &str = concat!(
    r#"{"timestamp":1786891791,"method":"session/update","params":{"sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","update":{"sessionUpdate":"turn_started"}}}"#,
    "\n",
    r#"{"timestamp":1786891792,"method":"session/update","params":{"sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":80,"outputTokens":20,"totalTokens":100}}}}"#,
    "\n",
);

/// 生产完成轮次把真实模型 slug 放在 `usage.modelUsage` 键上。
#[cfg(test)]
pub const PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL: &str = concat!(
    r#"{"timestamp":1786891792,"method":"session/update","params":{"sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","update":{"sessionUpdate":"turn_completed","usage":{"inputTokens":80,"outputTokens":20,"totalTokens":100,"cachedReadTokens":10,"cacheCreationTokens":2,"reasoningTokens":4,"modelUsage":{"grok-4.5-build":{"inputTokens":80,"outputTokens":20,"totalTokens":100,"cachedReadTokens":10,"cacheCreationTokens":2,"reasoningTokens":4}}}}}}"#,
    "\n",
);

/// 保存增量扫描可安全提交的完整行偏移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrokJsonlCheckpoint {
    /// 已确认可提交的完整行字节数。
    pub committed_bytes: u64,
    /// 当前缓冲中尚未凑成完整行的尾部字节数。
    pub trailing_bytes: u64,
    /// 标识尾部正在丢弃一条超长行。
    pub discarding_oversized_line: bool,
}

/// 仅包含格式与数据质量计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrokJsonlWarningCounts {
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
}

impl GrokJsonlWarningCounts {
    /// 汇总全部质量告警计数。
    pub fn total(self) -> u64 {
        self.malformed_lines
            .saturating_add(self.oversized_lines)
            .saturating_add(self.unknown_events)
            .saturating_add(self.missing_usage)
            .saturating_add(self.invalid_usage)
            .saturating_add(self.invalid_timestamps)
    }
}

/// 跨增量片段持久化的安全上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokJsonlParseContext {
    /// 由会话目录派生的稳定线程键。
    pub thread_key: String,
    /// 由编码工作目录派生的项目键。
    pub project_key: Option<String>,
    /// 安全展示用的项目标签。
    pub project_label: Option<String>,
    /// 已发出完成轮次序号；缺 turnId 时保证跨增量追加仍能区分调用。
    pub call_sequence: u64,
}

impl GrokJsonlParseContext {
    /// 从会话/项目种子派生散列键与安全展示标签。
    pub fn new(thread_seed: &str, project_seed: Option<&str>) -> Self {
        Self {
            thread_key: stable_id("grok-thread", thread_seed),
            project_key: project_seed.map(|value| stable_id("grok-project", value)),
            project_label: project_seed.and_then(grok_project_display_label),
            call_sequence: 0,
        }
    }
}

/// 一次流式解析的安全结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokJsonlParseReport {
    /// 本次实际发出的调用条数。
    pub calls_emitted: u64,
    /// 本次读取的原始字节总数。
    pub bytes_consumed: u64,
    /// 可安全用于下次增量续读的检查点。
    pub checkpoint: GrokJsonlCheckpoint,
    /// 本次解析累计的格式与数据质量告警。
    pub warnings: GrokJsonlWarningCounts,
    /// 标识解析是否因取消信号提前终止。
    pub cancelled: bool,
}

#[derive(Deserialize)]
/// 只承载 Grok 单行事件解析所需的外层字段。
struct Envelope {
    #[serde(rename = "sessionUpdate")]
    session_update: Option<String>,
    update: Option<Box<Envelope>>,
    params: Option<Box<Envelope>>,
    usage: Option<RawUsage>,
    #[serde(rename = "modelUsage")]
    model_usage: Option<BTreeMap<String, RawUsage>>,
    #[serde(default)]
    timestamp: Option<serde_json::Value>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    model: Option<String>,
    #[serde(rename = "turnId")]
    turn_id: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize, Clone)]
/// 保存 Grok 更新行中未经规范化的 Token 计数。
struct RawUsage {
    #[serde(rename = "inputTokens", alias = "input_tokens")]
    input_tokens: Option<u64>,
    #[serde(rename = "outputTokens", alias = "output_tokens")]
    output_tokens: Option<u64>,
    #[serde(rename = "totalTokens", alias = "total_tokens")]
    total_tokens: Option<u64>,
    #[serde(rename = "cachedReadTokens", alias = "cached_read_tokens")]
    cached_read_tokens: Option<u64>,
    #[serde(rename = "cacheCreationTokens", alias = "cache_creation_tokens")]
    cache_creation_tokens: Option<u64>,
    #[serde(rename = "reasoningTokens", alias = "reasoning_tokens")]
    reasoning_tokens: Option<u64>,
    /// 完成轮次用量对象内按模型拆分的用量；键是模型 slug。
    #[serde(rename = "modelUsage", default)]
    model_usage: Option<BTreeMap<String, RawUsage>>,
}

/// 区分一行被忽略、形成调用或导致文件级失败的结果。
enum LineOutcome {
    Calls(Vec<UsageCall>),
    Ignored,
    Unknown,
    Malformed,
    MissingUsage,
    InvalidUsage,
    InvalidTimestamp,
}

/// 流式读取 Grok `updates.jsonl`，只发出已完成轮次的用量事实。
pub fn parse_grok_jsonl_stream<R, F>(
    mut reader: R,
    max_line_bytes: usize,
    context: &mut GrokJsonlParseContext,
    provenance: &SourceProvenance,
    cancellation: &CancellationToken,
    mut on_call: F,
) -> Result<GrokJsonlParseReport, LocalError>
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
    let mut warnings = GrokJsonlWarningCounts::default();
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
                        LineOutcome::Calls(calls) => {
                            for call in calls {
                                on_call(call)?;
                                calls_emitted = calls_emitted.saturating_add(1);
                            }
                        }
                        LineOutcome::Ignored => {}
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

    Ok(GrokJsonlParseReport {
        calls_emitted,
        bytes_consumed,
        checkpoint: GrokJsonlCheckpoint {
            committed_bytes,
            trailing_bytes: line_bytes,
            discarding_oversized_line: oversized,
        },
        warnings,
        cancelled,
    })
}

/// 从夹具原文累加已完成轮次的 Token，供测试对账，禁止手写魔术总量。
#[cfg(test)]
pub fn sum_completed_usage_from_fixture(jsonl: &str) -> TokenUsage {
    let mut total = TokenUsage::zero();
    let mut context = GrokJsonlParseContext::new("sum", None);
    let provenance = SourceProvenance {
        source_id: "sum".to_owned(),
        root_id: "sum".to_owned(),
        relative_label: "sum".to_owned(),
        archived: false,
    };
    for line in jsonl.lines() {
        if let LineOutcome::Calls(calls) = parse_line(line.as_bytes(), &mut context, &provenance) {
            for call in calls {
                total = total.checked_add(&call.usage).expect("fixture sum fits");
            }
        }
    }
    total
}

/// 优先信封级 `modelUsage`，否则读取完成轮次 `usage.modelUsage` 的模型键。
fn completed_model_usage(payload: &Envelope) -> BTreeMap<String, RawUsage> {
    payload
        .model_usage
        .as_ref()
        .filter(|map| !map.is_empty())
        .cloned()
        .or_else(|| {
            payload
                .usage
                .as_ref()
                .and_then(|usage| usage.model_usage.clone())
                .filter(|map| !map.is_empty())
        })
        .unwrap_or_default()
}

/// 生产信封把用量放在 `params.update`；旧夹具放在顶层或 `update`。
fn usage_payload(record: &Envelope) -> &Envelope {
    record
        .params
        .as_ref()
        .and_then(|params| params.update.as_ref())
        .or(record.update.as_ref())
        .map_or(record, Box::as_ref)
}

/// 接受 RFC3339 字符串或 Unix 秒/毫秒数字时间戳。
fn envelope_occurred_at_epoch_ms(record: &Envelope, payload: &Envelope) -> Option<i64> {
    parse_json_timestamp(record.timestamp.as_ref())
        .or_else(|| parse_json_timestamp(payload.timestamp.as_ref()))
        .or_else(|| parse_rfc3339(record.created_at.as_deref()))
        .or_else(|| parse_rfc3339(payload.created_at.as_deref()))
}

/// 从数字或字符串 JSON 值解析毫秒时间戳。
fn parse_json_timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    match value? {
        serde_json::Value::String(text) => parse_rfc3339(Some(text)),
        serde_json::Value::Number(number) => {
            let raw = number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))?;
            Some(if raw.abs() >= 1_000_000_000_000 {
                raw
            } else {
                raw.saturating_mul(1_000)
            })
        }
        _ => None,
    }
}

/// 将可选 RFC 3339 时间转换为毫秒时间戳。
fn parse_rfc3339(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|text| text.parse::<Timestamp>().ok())
        .map(|timestamp| timestamp.as_millisecond())
}

/// 解析单行 Grok 事件并只接受具有可信完成用量的轮次。
fn parse_line(
    line: &[u8],
    context: &mut GrokJsonlParseContext,
    provenance: &SourceProvenance,
) -> LineOutcome {
    if line.is_empty() {
        return LineOutcome::Ignored;
    }
    let record = match serde_json::from_slice::<Envelope>(line) {
        Ok(record) => record,
        Err(_) => return LineOutcome::Malformed,
    };
    let payload = usage_payload(&record);
    let session_update = payload.session_update.as_deref().unwrap_or_default();
    if session_update != "turn_completed" {
        return if session_update.is_empty() {
            LineOutcome::Unknown
        } else {
            LineOutcome::Ignored
        };
    }
    let occurred_at_epoch_ms = match envelope_occurred_at_epoch_ms(&record, payload) {
        Some(value) => value,
        None => return LineOutcome::InvalidTimestamp,
    };
    let model_usage = completed_model_usage(payload);
    let fallback_usage = payload.usage.clone();
    let fallback_model = payload.model.clone();
    let turn_id = payload.turn_id.as_deref().or(payload.id.as_deref());
    let mut calls = Vec::new();
    if !model_usage.is_empty() {
        for (model, usage) in model_usage {
            match to_call(
                context,
                provenance,
                occurred_at_epoch_ms,
                Some(&model),
                turn_id,
                context.call_sequence,
                &usage,
            ) {
                Ok(call) => calls.push(call),
                Err(LineOutcome::InvalidUsage) => return LineOutcome::InvalidUsage,
                Err(LineOutcome::MissingUsage) => return LineOutcome::MissingUsage,
                Err(other) => return other,
            }
        }
    } else if let Some(usage) = fallback_usage {
        match to_call(
            context,
            provenance,
            occurred_at_epoch_ms,
            fallback_model.as_deref(),
            turn_id,
            context.call_sequence,
            &usage,
        ) {
            Ok(call) => calls.push(call),
            Err(outcome) => return outcome,
        }
    } else {
        return LineOutcome::MissingUsage;
    }
    context.call_sequence = context.call_sequence.saturating_add(1);
    LineOutcome::Calls(calls)
}

/// 将已验证的 Grok 原始事件映射为规范调用记录。
fn to_call(
    context: &GrokJsonlParseContext,
    provenance: &SourceProvenance,
    occurred_at_epoch_ms: i64,
    model: Option<&str>,
    turn_id: Option<&str>,
    turn_sequence: u64,
    usage: &RawUsage,
) -> Result<UsageCall, LineOutcome> {
    let Some(input_tokens) = usage.input_tokens else {
        return Err(LineOutcome::MissingUsage);
    };
    let Some(output_tokens) = usage.output_tokens else {
        return Err(LineOutcome::MissingUsage);
    };
    let cached = usage.cached_read_tokens.unwrap_or_default();
    let cache_write = usage.cache_creation_tokens.unwrap_or_default();
    let reasoning = usage.reasoning_tokens.unwrap_or_default();
    let usage = TokenUsage::new_with_availability(
        input_tokens,
        Some(cached),
        Some(cache_write),
        output_tokens,
        Some(reasoning),
        usage.total_tokens,
    )
    .map_err(|_| LineOutcome::InvalidUsage)?;
    let identity = format!(
        "{}\u{0}{}\u{0}{}\u{0}{turn_sequence}",
        context.thread_key,
        turn_id.unwrap_or(""),
        model.unwrap_or("")
    );
    Ok(UsageCall {
        logical_call_id: stable_id("grok-call", &identity),
        occurred_at_epoch_ms,
        model: model.and_then(safe_model_label),
        reasoning_effort: None,
        project_key: context.project_key.clone(),
        project_label: context.project_label.clone(),
        thread_key: context.thread_key.clone(),
        thread_label: None,
        usage,
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![provenance.clone()],
    })
}
