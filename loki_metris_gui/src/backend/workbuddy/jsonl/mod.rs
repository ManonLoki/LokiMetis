//! WorkBuddy project JSONL 的有界流式用量投影。

#[cfg(test)]
mod borrow_tests;
mod cache;
#[cfg(test)]
mod cache_tests;
mod line_reader;

use line_reader::{LineRead, read_complete_line};
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufReader, Read, Take};
use std::path::Path;

use jiff::Timestamp;
use loki_metris_core::{
    CoverageReport, CoverageState, WorkbuddyUsageEventRecord, safe_path_basename, stable_id,
};
use serde::Deserialize;

use crate::backend::file_stamp::FileStamp;
use crate::backend::local_index::{
    DEFAULT_MAX_JSONL_LINE_BYTES, metadata_is_link_like, same_opened_file,
    walk_ancestors_for_link_component,
};

use super::WorkbuddyReadError;
use super::projects::{WorkbuddyProjectFile, discover_project_files};

/// 单次读取允许消耗的 project JSONL 总字节数。
const MAX_PROJECT_JSONL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 单次读取允许保留的 usage 事件数。
const MAX_USAGE_RECORDS: usize = 500_000;

/// 一次 project JSONL 读取的安全投影与覆盖结论。
pub(super) struct WorkbuddyProjectUsageRead {
    /// 读取到的用量记录。
    pub(super) records: Vec<WorkbuddyUsageEventRecord>,
    /// 覆盖结论。
    pub(super) coverage: CoverageReport,
}

/// 只反序列化顶层事件中统计必需的白名单字段。
#[derive(Deserialize)]
struct ProjectEvent<'a> {
    /// 事件时间（Unix 毫秒）。
    timestamp: Option<i64>,
    /// 会话 ID。
    #[serde(borrow, rename = "sessionId")]
    session_id: Option<JsonString<'a>>,
    /// 工作目录，只用于派生项目键。
    #[serde(borrow)]
    cwd: Option<JsonString<'a>>,
    /// provider 层数据。
    #[serde(borrow, rename = "providerData")]
    provider_data: Option<ProviderData<'a>>,
}

/// 无转义字符串直接借用当前行，合法转义仍由serde解码为独立拥有的字符串。
#[derive(Deserialize)]
struct JsonString<'a>(#[serde(borrow)] Cow<'a, str>);
impl std::ops::Deref for JsonString<'_> {
    /// 解引用为借用或已解码的字符串视图。
    type Target = str;
    /// 下游只读取字符串视图；输出记录需要保留的模型另行取得所有权。
    fn deref(&self) -> &str {
        self.0.as_ref()
    }
}

/// WorkBuddy provider 层的实际模型、调用 ID 与直接 usage。
#[derive(Deserialize)]
struct ProviderData<'a> {
    /// 消息 ID，与会话 ID 组成稳定调用 ID。
    #[serde(borrow, rename = "messageId")]
    message_id: Option<JsonString<'a>>,
    /// 实际模型。
    #[serde(borrow)]
    model: Option<JsonString<'a>>,
    /// 直接用量。
    usage: Option<ProviderUsage>,
    /// 原始用量，只取积分。
    #[serde(rename = "rawUsage")]
    raw_usage: Option<RawUsage>,
}

/// 直接 provider usage 是 Token 与请求数的唯一来源。
#[derive(Deserialize)]
struct ProviderUsage {
    /// 全部输入 Token。
    #[serde(rename = "inputTokens")]
    input_tokens: Option<i64>,
    /// 输入 Token 明细。
    #[serde(rename = "inputTokensDetails")]
    input_token_details: Option<Vec<InputTokenDetail>>,
    /// 输出 Token。
    #[serde(rename = "outputTokens")]
    output_tokens: Option<i64>,
    /// 上游总 Token。
    #[serde(rename = "totalTokens")]
    total_tokens: Option<i64>,
    /// 请求数。
    requests: Option<i64>,
}

/// 缓存输入只读取 `cached_tokens`，它是 input 的子集。
#[derive(Deserialize)]
struct InputTokenDetail {
    /// 命中缓存的输入 Token。
    cached_tokens: Option<i64>,
}

/// raw usage 仅用于逐请求积分；其它 provider 私有字段全部忽略。
#[derive(Deserialize)]
struct RawUsage {
    /// 逐请求积分。
    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    credit: Option<f64>,
}

/// 把任意 JSON credit 值宽容投影为有限非负数；不可用时保持 `None`。
fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_f64()
        .filter(|credit| credit.is_finite() && *credit >= 0.0))
}

/// 流式解析质量计数，不携带文件名或记录正文。
#[derive(Clone, Copy, Default)]
struct ProjectJsonlWarnings {
    /// 损坏行数。
    malformed_lines: u64,
    /// 超长行数。
    oversized_lines: u64,
    /// 时间戳无效的行数。
    invalid_timestamp: u64,
    /// 身份无效的行数。
    invalid_identity: u64,
    /// 用量无效的行数。
    invalid_usage: u64,
}

impl ProjectJsonlWarnings {
    /// 汇总所有会影响统计完整性的已知格式缺口。
    fn total(&self) -> u64 {
        self.malformed_lines
            .saturating_add(self.oversized_lines)
            .saturating_add(self.invalid_timestamp)
            .saturating_add(self.invalid_identity)
            .saturating_add(self.invalid_usage)
    }

    /// 合并单文件解析结果。
    fn merge(&mut self, other: Self) {
        self.malformed_lines = self.malformed_lines.saturating_add(other.malformed_lines);
        self.oversized_lines = self.oversized_lines.saturating_add(other.oversized_lines);
        self.invalid_timestamp = self
            .invalid_timestamp
            .saturating_add(other.invalid_timestamp);
        self.invalid_identity = self.invalid_identity.saturating_add(other.invalid_identity);
        self.invalid_usage = self.invalid_usage.saturating_add(other.invalid_usage);
    }
}

/// 完整行投影后的稳定分类；普通消息与工具事件属于 `Ignored`。
enum LineOutcome {
    /// 形成一条用量记录。
    Usage(Box<WorkbuddyUsageEventRecord>),
    /// 普通消息或工具事件。
    Ignored,
    /// JSON 损坏。
    Malformed,
    /// 时间戳无效。
    InvalidTimestamp,
    /// 会话或消息 ID 无效。
    InvalidIdentity,
    /// 用量不满足不变量。
    InvalidUsage,
}

/// 枚举并读取全部获批 project JSONL 布局；单文件失败时保留其它可用事实并标为部分覆盖。
pub(super) fn read_project_usage(
    workbuddy_home: &Path,
    root_id: &str,
) -> Result<WorkbuddyProjectUsageRead, WorkbuddyReadError> {
    let files = discover_project_files(workbuddy_home)?;
    let mut records = Vec::new();
    let mut warnings = ProjectJsonlWarnings::default();
    let mut io_skipped_count = 0_u64;
    let mut remaining_bytes = MAX_PROJECT_JSONL_BYTES;
    let mut budget_exhausted = files.budget_exhausted;

    for source in &files.files {
        if remaining_bytes == 0 || records.len() >= MAX_USAGE_RECORDS {
            budget_exhausted = true;
            break;
        }
        let (reader, stamp) = match open_fixed_prefix_stamped(&source.path) {
            Ok(value) => value,
            Err(_) => {
                io_skipped_count = io_skipped_count.saturating_add(1);
                continue;
            }
        };
        let observed_len = stamp.len();
        let allowed_len = observed_len.min(remaining_bytes);
        if allowed_len < observed_len {
            budget_exhausted = true;
        }
        remaining_bytes = remaining_bytes.saturating_sub(allowed_len);
        // 只有文件完整落入字节与记录预算时才复用：命中后的结果与重新解析逐项相同。
        let complete = allowed_len == observed_len;
        if complete
            && let Some(parsed) = cache::lookup(root_id, &stamp, source)
                && records.len().saturating_add(parsed.records.len()) < MAX_USAGE_RECORDS {
                    records.extend(parsed.records.iter().cloned());
                    warnings.merge(parsed.warnings);
                    continue;
                }
        let first_record = records.len();
        match parse_project_stream(reader.take(allowed_len), source, root_id, &mut records) {
            Ok(report) => {
                if complete && !report.record_budget_exhausted {
                    cache::store(
                        root_id,
                        &source.path,
                        cache::ParsedFile::new(
                            stamp,
                            source,
                            records[first_record..].to_vec(),
                            report.warnings,
                        ),
                    );
                }
                warnings.merge(report.warnings);
                budget_exhausted |= report.record_budget_exhausted;
            }
            Err(_) => {
                io_skipped_count = io_skipped_count.saturating_add(1);
            }
        }
        if budget_exhausted && (remaining_bytes == 0 || records.len() >= MAX_USAGE_RECORDS) {
            break;
        }
    }

    // 仅在整根读取完整结束时清理；预算中途停止的轮次可能没有访问到后面的文件。
    if !budget_exhausted {
        let seen: Vec<&Path> = files.files.iter().map(|source| source.path.as_path()).collect();
        cache::retain_seen(root_id, &seen);
    }

    let skipped_count = files
        .skipped_count
        .saturating_add(io_skipped_count)
        .saturating_add(u64::from(budget_exhausted));
    let warning_count = warnings.total();
    let state = if files.permission_denied_count > 0 || skipped_count > 0 || warning_count > 0 {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    Ok(WorkbuddyProjectUsageRead {
        records,
        coverage: CoverageReport {
            state,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: files.permission_denied_count,
            skipped_count,
            warning_count,
        },
    })
}

/// 打开普通文件、复核链接与句柄身份，并冻结本轮只读长度。
pub(super) fn open_fixed_prefix(path: &Path) -> std::io::Result<(File, u64)> {
    open_fixed_prefix_stamped(path).map(|(file, stamp)| (file, stamp.len()))
}

/// 同 [`open_fixed_prefix`]，另返回打开时刻的文件戳，供解析结果复用判断。
fn open_fixed_prefix_stamped(path: &Path) -> std::io::Result<(File, FileStamp)> {
    let file = File::open(path)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file() || walk_ancestors_for_link_component(path)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported WorkBuddy source",
        ));
    }
    let current_metadata = fs::symlink_metadata(path)?;
    if metadata_is_link_like(&current_metadata)
        || !current_metadata.is_file()
        || !same_opened_file(&file, path, &opened_metadata, &current_metadata)?
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "WorkBuddy source changed during inspection",
        ));
    }
    let stamp = FileStamp::of(&file, &opened_metadata);
    Ok((file, stamp))
}

/// 单文件解析报告。
struct ProjectStreamReport {
    /// 质量计数。
    warnings: ProjectJsonlWarnings,
    /// 记录预算已耗尽。
    record_budget_exhausted: bool,
}

/// 固定 16 KiB 缓冲按完整行投影；超长行有界跳过，末尾半行不提交。
fn parse_project_stream(
    reader: Take<File>,
    source: &WorkbuddyProjectFile,
    root_id: &str,
    records: &mut Vec<WorkbuddyUsageEventRecord>,
) -> std::io::Result<ProjectStreamReport> {
    let mut reader = BufReader::with_capacity(16 * 1024, reader);
    let mut line = Vec::with_capacity(DEFAULT_MAX_JSONL_LINE_BYTES.min(16 * 1024));
    let mut warnings = ProjectJsonlWarnings::default();
    let mut record_budget_exhausted = false;

    loop {
        match read_complete_line(&mut reader, &mut line, DEFAULT_MAX_JSONL_LINE_BYTES)? {
            LineRead::End => break,
            LineRead::Oversized => {
                warnings.oversized_lines = warnings.oversized_lines.saturating_add(1);
            }
            LineRead::Complete => {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                apply_line_outcome(
                    parse_complete_line(&line, source, root_id),
                    records,
                    &mut warnings,
                );
                if records.len() >= MAX_USAGE_RECORDS {
                    record_budget_exhausted = true;
                    break;
                }
            }
        }
    }
    Ok(ProjectStreamReport {
        warnings,
        record_budget_exhausted,
    })
}

/// 把一条完整行分类并累计安全结果。
fn apply_line_outcome(
    outcome: LineOutcome,
    records: &mut Vec<WorkbuddyUsageEventRecord>,
    warnings: &mut ProjectJsonlWarnings,
) {
    match outcome {
        LineOutcome::Usage(record) => records.push(*record),
        LineOutcome::Ignored => {}
        LineOutcome::Malformed => {
            warnings.malformed_lines = warnings.malformed_lines.saturating_add(1)
        }
        LineOutcome::InvalidTimestamp => {
            warnings.invalid_timestamp = warnings.invalid_timestamp.saturating_add(1)
        }
        LineOutcome::InvalidIdentity => {
            warnings.invalid_identity = warnings.invalid_identity.saturating_add(1)
        }
        LineOutcome::InvalidUsage => {
            warnings.invalid_usage = warnings.invalid_usage.saturating_add(1)
        }
    }
}

/// 从一条完整 JSONL 记录提取直接 provider usage；嵌套 message usage 从未声明，因而不会计数。
fn parse_complete_line(line: &[u8], source: &WorkbuddyProjectFile, root_id: &str) -> LineOutcome {
    if line.is_empty() {
        return LineOutcome::Ignored;
    }
    let event = match serde_json::from_slice::<ProjectEvent<'_>>(line) {
        Ok(event) => event,
        Err(_) => return LineOutcome::Malformed,
    };
    let Some(provider) = event.provider_data else {
        return LineOutcome::Ignored;
    };
    let Some(usage) = provider.usage else {
        return LineOutcome::Ignored;
    };
    let Some(timestamp) = event.timestamp else {
        return LineOutcome::InvalidTimestamp;
    };
    if Timestamp::from_millisecond(timestamp).is_err() {
        return LineOutcome::InvalidTimestamp;
    }
    let Some(session_id) = nonempty(event.session_id.as_deref()) else {
        return LineOutcome::InvalidIdentity;
    };
    let Some(message_id) = nonempty(provider.message_id.as_deref()) else {
        return LineOutcome::InvalidIdentity;
    };
    let Some((input_tokens, cached_input_tokens, output_tokens, total_tokens, requests)) =
        validated_usage(&usage)
    else {
        return LineOutcome::InvalidUsage;
    };

    let project_seed = nonempty(event.cwd.as_deref()).unwrap_or(&source.project_key_material);
    let project_label = event.cwd.as_deref().and_then(safe_path_basename);
    let session_key = stable_id("workbuddy-session", &format!("{root_id}\0{session_id}"));
    let logical_call_id = stable_id(
        "workbuddy-call",
        &format!("{root_id}\0{session_id}\0{message_id}"),
    );
    let source_id = stable_id(
        "workbuddy-source",
        &format!("{root_id}\0{}", source.relative_key),
    );
    LineOutcome::Usage(Box::new(WorkbuddyUsageEventRecord {
        logical_call_id,
        session_key,
        source_id,
        root_id: root_id.to_owned(),
        occurred_at_epoch_ms: timestamp,
        model: provider.model.map(|value| value.0.into_owned()),
        project_key: Some(stable_id("workbuddy-project", project_seed)),
        project_label,
        request_count: requests,
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens,
        credit: provider.raw_usage.and_then(|raw| raw.credit),
        origin: source.origin,
    }))
}

/// 校验 usage 的精确整数、不变量与缓存子集关系。
fn validated_usage(usage: &ProviderUsage) -> Option<(i64, i64, i64, i64, i64)> {
    let input_tokens = usage.input_tokens?;
    let output_tokens = usage.output_tokens?;
    let total_tokens = usage.total_tokens?;
    let requests = usage.requests?;
    let cached_input_tokens = usage
        .input_token_details
        .as_ref()?
        .iter()
        .try_fold(0_i64, |sum, detail| sum.checked_add(detail.cached_tokens?))?;
    if input_tokens < 0
        || output_tokens < 0
        || total_tokens < 0
        || cached_input_tokens < 0
        || cached_input_tokens > input_tokens
        || !(1..=10_000).contains(&requests)
        || input_tokens.checked_add(output_tokens)? != total_tokens
    {
        return None;
    }
    Some((
        input_tokens,
        cached_input_tokens,
        output_tokens,
        total_tokens,
        requests,
    ))
}

/// 只接受去除首尾空白后的非空身份字段。
fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
