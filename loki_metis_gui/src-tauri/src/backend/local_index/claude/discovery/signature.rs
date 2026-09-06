//! Claude transcript 结构签名探测与有界预算。

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use loki_metis_core::{CoverageState, RootCandidateEvidence};
use serde::Deserialize;

use super::super::super::discovery::{metadata_is_link_like, walk_ancestors_for_link_component};
use super::super::super::{CancellationToken, FullDiscoveryOptions};
use super::super::path_rules::{is_subagent_jsonl_name, is_uuid, is_uuid_jsonl_name};

/// 单条结构签名记录允许的最大字节数，超出即视为拒绝证据。
const MAX_SIGNATURE_LINE_BYTES: u64 = 1024 * 1024;
/// 每个 transcript 文件最多尝试读取的记录条数上限。
const MAX_SIGNATURE_RECORDS_PER_FILE: u64 = 64;

/// 结构签名探测过程中共享的有界预算，防止无限递归遍历目录树。
pub(crate) struct SignatureBudget<'a> {
    /// 用于中途响应取消请求的令牌。
    cancellation: &'a CancellationToken,
    /// 剩余允许遍历的目录数预算。
    remaining_directories: u64,
    /// 剩余允许处理的目录项数预算。
    remaining_entries: u64,
    /// 剩余允许打开的文件数预算。
    remaining_files: u64,
    /// 剩余允许读取的字节数预算。
    remaining_bytes: u64,
}

impl<'a> SignatureBudget<'a> {
    /// 从全设备发现选项初始化各项预算上限。
    pub(crate) fn new(options: &FullDiscoveryOptions, cancellation: &'a CancellationToken) -> Self {
        Self {
            cancellation,
            remaining_directories: options.max_signature_directories,
            remaining_entries: options.max_signature_entries,
            remaining_files: options.max_signature_files,
            remaining_bytes: options.max_signature_bytes,
        }
    }
}

/// 单次结构签名探测的结论。
// 区分“确认不是”（Rejected/NotRoot）与“无法确认”（Indeterminate/
// Cancelled/BudgetExhausted）：前者可放心当作否定结论丢弃候选根，后者
// 必须保留旧登记，不能因权限突变、路径被移动等瞬时问题误删用户数据源。
pub(crate) enum Inspection {
    /// 确认是 Claude transcript 根目录，并保留命中的结构证据类型。
    Found(RootCandidateEvidence),
    /// 确认不是候选根（例如目录不存在）。
    NotRoot,
    /// 确认结构不符合 Claude transcript 特征，视同否定结论。
    Rejected,
    /// 未能得出确定结论（如临时性 I/O 错误）；调用方须原样保留旧登记。
    Indeterminate,
    /// 探测过程中收到取消信号，提前中止。
    Cancelled,
    /// 预算耗尽提前中止，尚未得出结论。
    BudgetExhausted,
}

/// 结构签名探测只反序列化的最小字段集合，缺失字段一律容忍为 `None`。
#[derive(Deserialize)]
struct SignatureRecord {
    /// 事件类型标签；只有 `"assistant"` 才可能是用量证据。
    #[serde(rename = "type")]
    kind: Option<String>,
    /// 会话 ID。
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    /// 记录时间戳原始字符串。
    timestamp: Option<String>,
    /// 承载用量证据的消息片段。
    message: Option<SignatureMessage>,
}

/// 记录中承载用量证据的消息片段。
#[derive(Deserialize)]
struct SignatureMessage {
    /// 消息 ID。
    id: Option<String>,
    /// 模型名。
    model: Option<String>,
    /// Token 用量字段。
    usage: Option<SignatureUsage>,
}

/// 判定是否为真实用量记录所需的 Token 计数字段。
#[derive(Deserialize)]
struct SignatureUsage {
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

/// 探测候选路径下的 `projects` 子目录，判定其是否具备 Claude transcript 结构特征。
pub(crate) fn inspect_root(path: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    let projects = path.join("projects");
    let metadata = match fs::symlink_metadata(&projects) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Inspection::NotRoot,
        Err(_) => return Inspection::Indeterminate,
    };
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Inspection::Rejected;
    }
    inspect_projects(&projects, budget)
}

/// 遍历 `projects` 目录下的各项目子目录，任一项目命中即视为确认。
fn inspect_projects(projects: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    if budget.remaining_directories == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_directories -= 1;
    let entries = match fs::read_dir(projects) {
        Ok(entries) => entries,
        Err(_) => return Inspection::Indeterminate,
    };
    let mut indeterminate = false;
    for project in entries {
        if budget.cancellation.is_cancelled() {
            return Inspection::Cancelled;
        }
        if budget.remaining_entries == 0 {
            return Inspection::BudgetExhausted;
        }
        budget.remaining_entries -= 1;
        let project = match project {
            Ok(entry) => entry,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        let metadata = match fs::symlink_metadata(project.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        if metadata_is_link_like(&metadata) || !metadata.is_dir() {
            continue;
        }
        match inspect_project_directory(&project.path(), budget) {
            Inspection::Found(evidence) => return Inspection::Found(evidence),
            Inspection::Indeterminate => indeterminate = true,
            Inspection::Cancelled => return Inspection::Cancelled,
            Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
            Inspection::NotRoot | Inspection::Rejected => {}
        }
    }
    if indeterminate {
        Inspection::Indeterminate
    } else {
        Inspection::Rejected
    }
}

/// 在单个项目目录内查找 UUID 命名的 transcript 文件或 subagents 子目录。
fn inspect_project_directory(project: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    if budget.remaining_directories == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_directories -= 1;
    let entries = match fs::read_dir(project) {
        Ok(entries) => entries,
        Err(_) => return Inspection::Indeterminate,
    };
    let mut indeterminate = false;
    for entry in entries {
        if budget.cancellation.is_cancelled() {
            return Inspection::Cancelled;
        }
        if budget.remaining_entries == 0 {
            return Inspection::BudgetExhausted;
        }
        budget.remaining_entries -= 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            continue;
        }
        let is_uuid_jsonl = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(is_uuid_jsonl_name);
        if metadata.is_file() && is_uuid_jsonl {
            match inspect_transcript(&path, budget, RootCandidateEvidence::ClaudeTranscript) {
                Inspection::Found(evidence) => return Inspection::Found(evidence),
                Inspection::Indeterminate => indeterminate = true,
                Inspection::Cancelled => return Inspection::Cancelled,
                Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
                Inspection::NotRoot | Inspection::Rejected => {}
            }
        } else if metadata.is_dir()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(is_uuid)
        {
            match inspect_subagents_directory(&path.join("subagents"), budget) {
                Inspection::Found(evidence) => return Inspection::Found(evidence),
                Inspection::Indeterminate => indeterminate = true,
                Inspection::Cancelled => return Inspection::Cancelled,
                Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
                Inspection::NotRoot | Inspection::Rejected => {}
            }
        }
    }
    if indeterminate {
        Inspection::Indeterminate
    } else {
        Inspection::Rejected
    }
}

/// 检查 `subagents` 子目录内是否存在符合命名规则的 transcript 文件。
fn inspect_subagents_directory(path: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Inspection::NotRoot,
        Err(_) => return Inspection::Indeterminate,
    };
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Inspection::Rejected;
    }
    if budget.remaining_directories == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_directories -= 1;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return Inspection::Indeterminate,
    };
    let mut indeterminate = false;
    for entry in entries {
        if budget.cancellation.is_cancelled() {
            return Inspection::Cancelled;
        }
        if budget.remaining_entries == 0 {
            return Inspection::BudgetExhausted;
        }
        budget.remaining_entries -= 1;
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        if metadata_is_link_like(&metadata)
            || !metadata.is_file()
            || !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(is_subagent_jsonl_name)
        {
            continue;
        }
        match inspect_transcript(&path, budget, RootCandidateEvidence::ClaudeSubagent) {
            Inspection::Found(evidence) => return Inspection::Found(evidence),
            Inspection::Indeterminate => indeterminate = true,
            Inspection::Cancelled => return Inspection::Cancelled,
            Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
            Inspection::NotRoot | Inspection::Rejected => {}
        }
    }
    if indeterminate {
        Inspection::Indeterminate
    } else {
        Inspection::Rejected
    }
}

/// 逐行读取单个 transcript 文件，寻找符合用量特征的记录作为结构证据。
fn inspect_transcript(
    path: &Path,
    budget: &mut SignatureBudget<'_>,
    evidence: RootCandidateEvidence,
) -> Inspection {
    if budget.remaining_files == 0 || budget.remaining_bytes == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_files -= 1;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Inspection::Indeterminate,
    };
    let mut reader = BufReader::new(file);
    for _ in 0..MAX_SIGNATURE_RECORDS_PER_FILE {
        if budget.cancellation.is_cancelled() {
            return Inspection::Cancelled;
        }
        if budget.remaining_bytes == 0 {
            return Inspection::BudgetExhausted;
        }
        let line_limit = MAX_SIGNATURE_LINE_BYTES.min(budget.remaining_bytes);
        let mut line = Vec::new();
        let read = match reader
            .by_ref()
            .take(line_limit.saturating_add(1))
            .read_until(b'\n', &mut line)
        {
            Ok(read) => read,
            Err(_) => return Inspection::Indeterminate,
        };
        if read == 0 {
            return Inspection::Rejected;
        }
        let read_u64 = u64::try_from(read).unwrap_or(u64::MAX);
        budget.remaining_bytes = budget.remaining_bytes.saturating_sub(read_u64);
        if read_u64 > line_limit || !line.ends_with(b"\n") {
            return if budget.remaining_bytes == 0 {
                Inspection::BudgetExhausted
            } else {
                Inspection::Rejected
            };
        }
        if let Ok(record) = serde_json::from_slice::<SignatureRecord>(&line)
            && is_usage_signature(&record)
        {
            return Inspection::Found(evidence);
        }
    }
    Inspection::Rejected
}

/// 判定单条记录是否具备真实用量记录的最小字段特征。
fn is_usage_signature(record: &SignatureRecord) -> bool {
    if record.kind.as_deref() != Some("assistant")
        || !nonempty(record.session_id.as_deref())
        || record
            .timestamp
            .as_deref()
            .is_none_or(|value| value.parse::<Timestamp>().is_err())
    {
        return false;
    }
    let Some(message) = &record.message else {
        return false;
    };
    let Some(usage) = &message.usage else {
        return false;
    };
    nonempty(message.id.as_deref())
        && nonempty(message.model.as_deref())
        && usage.input_tokens.is_some()
        && usage.output_tokens.is_some()
        && usage
            .input_tokens
            .unwrap_or_default()
            .checked_add(usage.cache_read_input_tokens.unwrap_or_default())
            .and_then(|value| {
                value.checked_add(usage.cache_creation_input_tokens.unwrap_or_default())
            })
            .is_some()
}

/// 判定可选字符串在去除首尾空白后是否非空。
fn nonempty(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

/// 沿路径的每一级祖先检查是否存在链接组件，供候选路径安全性校验复用。
pub(super) fn reject_link_components(path: &Path) -> std::io::Result<bool> {
    match walk_ancestors_for_link_component(path) {
        Ok(has_link) => Ok(has_link),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// 取路径末段作为安全展示别名；末段缺失或为空时回退到固定中文占位。
pub(super) fn safe_alias(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Claude Code 数据根")
        .to_owned()
}

/// 按取消、预算耗尽与权限/跳过计数折算出本次发现的覆盖状态。
pub(super) fn coverage_state(
    cancelled: bool,
    budget_exhausted: bool,
    permission_denied_count: u64,
    skipped_count: u64,
) -> CoverageState {
    if cancelled {
        CoverageState::Cancelled
    } else if budget_exhausted || permission_denied_count > 0 || skipped_count > 0 {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    }
}

/// 判定路径是否落在本次遍历需要排除的固定根集合内。
pub(super) fn is_excluded(path: &Path, traversal_root: &Path, excluded_roots: &[PathBuf]) -> bool {
    loki_metis_core::is_discovery_path_excluded(path, traversal_root, excluded_roots)
}

/// 判定路径是否越过了另一个独立搜索根，避免同一子树被重复遍历。
pub(super) fn crosses_search_root(
    path: &Path,
    traversal_root: &Path,
    search_roots: &[PathBuf],
) -> bool {
    search_roots.iter().any(|other| {
        other != traversal_root && other.starts_with(traversal_root) && path.starts_with(other)
    })
}
