//! Grok home 结构签名探测：只承认 `sessions/**/updates.jsonl`。

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use loki_metis_core::{CoverageState, RootCandidateEvidence};
use serde::Deserialize;

use super::super::super::discovery::{metadata_is_link_like, walk_ancestors_for_link_component};
use super::super::super::{CancellationToken, FullDiscoveryOptions};
use super::super::path_rules::is_grok_updates_path;

const MAX_SIGNATURE_LINE_BYTES: u64 = 1024 * 1024;
const MAX_SIGNATURE_RECORDS_PER_FILE: u64 = 64;

/// 结构签名探测过程中共享的有界预算。
pub(crate) struct SignatureBudget<'a> {
    cancellation: &'a CancellationToken,
    remaining_directories: u64,
    remaining_entries: u64,
    remaining_files: u64,
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
pub(crate) enum Inspection {
    /// 确认是 Grok home，并保留命中的结构证据。
    Found(RootCandidateEvidence),
    /// 目录不存在。
    NotRoot,
    /// 确认结构不符合 Grok 会话特征。
    Rejected,
    /// 未能得出确定结论，须保留旧登记。
    Indeterminate,
    /// 探测过程中收到取消信号。
    Cancelled,
    /// 预算耗尽提前中止。
    BudgetExhausted,
}

#[derive(Deserialize)]
/// 仅反序列化 Grok 更新签名所需的外层事件字段。
struct SignatureEnvelope {
    #[serde(rename = "sessionUpdate")]
    session_update: Option<String>,
    update: Option<Box<SignatureEnvelope>>,
    params: Option<Box<SignatureEnvelope>>,
    usage: Option<SignatureUsage>,
    #[serde(rename = "modelUsage")]
    model_usage: Option<serde_json::Value>,
}

#[derive(Deserialize)]
/// 仅反序列化判断已完成用量所需的计数字段。
struct SignatureUsage {
    #[serde(rename = "inputTokens", alias = "input_tokens")]
    input_tokens: Option<u64>,
    #[serde(rename = "outputTokens", alias = "output_tokens")]
    output_tokens: Option<u64>,
}

/// 探测候选路径下的 `sessions` 子目录是否具备 Grok 会话结构。
pub(crate) fn inspect_root(path: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    let sessions = path.join("sessions");
    let metadata = match fs::symlink_metadata(&sessions) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Inspection::NotRoot,
        Err(_) => return Inspection::Indeterminate,
    };
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Inspection::Rejected;
    }
    inspect_sessions(path, &sessions, budget)
}

/// 在严格 sessions 目录内检查是否存在有效 Grok 更新文件。
fn inspect_sessions(root: &Path, sessions: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    walk_for_updates(root, sessions, budget, 0)
}

/// 在共享预算内遍历会话子树并寻找 `updates.jsonl`。
fn walk_for_updates(
    root: &Path,
    directory: &Path,
    budget: &mut SignatureBudget<'_>,
    depth: u32,
) -> Inspection {
    if budget.cancellation.is_cancelled() {
        return Inspection::Cancelled;
    }
    if budget.remaining_directories == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_directories -= 1;
    let entries = match fs::read_dir(directory) {
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
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            if depth >= 3 {
                continue;
            }
            match walk_for_updates(root, &entry.path(), budget, depth + 1) {
                Inspection::Found(evidence) => return Inspection::Found(evidence),
                Inspection::Cancelled => return Inspection::Cancelled,
                Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
                Inspection::Indeterminate => indeterminate = true,
                Inspection::NotRoot | Inspection::Rejected => {}
            }
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let entry_path = entry.path();
        let Ok(relative) = entry_path.strip_prefix(root) else {
            continue;
        };
        if !is_grok_updates_path(relative) {
            continue;
        }
        match inspect_updates_file(&entry.path(), budget) {
            Inspection::Found(evidence) => return Inspection::Found(evidence),
            Inspection::Cancelled => return Inspection::Cancelled,
            Inspection::BudgetExhausted => return Inspection::BudgetExhausted,
            Inspection::Indeterminate => indeterminate = true,
            Inspection::NotRoot | Inspection::Rejected => {}
        }
    }
    if indeterminate {
        Inspection::Indeterminate
    } else {
        Inspection::Rejected
    }
}

/// 读取单个有界更新文件并确认至少一个已完成用量事件。
fn inspect_updates_file(path: &Path, budget: &mut SignatureBudget<'_>) -> Inspection {
    if budget.remaining_files == 0 {
        return Inspection::BudgetExhausted;
    }
    budget.remaining_files -= 1;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return Inspection::Indeterminate,
    };
    let mut reader = BufReader::new(file);
    let mut records = 0_u64;
    let mut line = Vec::new();
    loop {
        if budget.cancellation.is_cancelled() {
            return Inspection::Cancelled;
        }
        if records >= MAX_SIGNATURE_RECORDS_PER_FILE {
            return Inspection::Rejected;
        }
        line.clear();
        let read = match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Inspection::Rejected,
            Ok(read) => read as u64,
            Err(_) => return Inspection::Indeterminate,
        };
        if budget.remaining_bytes < read {
            return Inspection::BudgetExhausted;
        }
        budget.remaining_bytes -= read;
        if read > MAX_SIGNATURE_LINE_BYTES {
            continue;
        }
        records += 1;
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line_has_completed_usage(&line) {
            return Inspection::Found(RootCandidateEvidence::GrokSessionUpdates);
        }
    }
}

/// 判断一行 JSON 是否携带可计入索引的已完成总量。
fn line_has_completed_usage(line: &[u8]) -> bool {
    let Ok(record) = serde_json::from_slice::<SignatureEnvelope>(line) else {
        return false;
    };
    let payload = signature_usage_payload(&record);
    let session_update = payload.session_update.as_deref().unwrap_or_default();
    if session_update != "turn_completed" {
        return false;
    }
    let has_model_usage = payload
        .model_usage
        .as_ref()
        .is_some_and(|value| value.as_object().is_some_and(|map| !map.is_empty()));
    has_model_usage
        || payload
            .usage
            .as_ref()
            .is_some_and(|item| item.input_tokens.is_some() && item.output_tokens.is_some())
}

/// 生产 `updates.jsonl` 把 turn 放在 `params.update`；旧夹具则在顶层或 `update`。
fn signature_usage_payload(record: &SignatureEnvelope) -> &SignatureEnvelope {
    record
        .params
        .as_ref()
        .and_then(|params| params.update.as_ref())
        .or(record.update.as_ref())
        .map_or(record, Box::as_ref)
}

/// 沿路径祖先检查链接组件。
pub(super) fn reject_link_components(path: &Path) -> std::io::Result<bool> {
    match walk_ancestors_for_link_component(path) {
        Ok(has_link) => Ok(has_link),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// 取路径末段作为安全展示别名。
pub(super) fn safe_alias(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Grok 数据根")
        .to_owned()
}

/// 按取消、预算与跳过计数折算覆盖状态。
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

/// 判定路径是否落在排除根集合内。
pub(super) fn is_excluded(path: &Path, traversal_root: &Path, excluded_roots: &[PathBuf]) -> bool {
    loki_metis_core::is_discovery_path_excluded(path, traversal_root, excluded_roots)
}

/// 判定路径是否越过了另一个独立搜索根。
pub(super) fn crosses_search_root(
    path: &Path,
    traversal_root: &Path,
    search_roots: &[PathBuf],
) -> bool {
    search_roots.iter().any(|other| {
        other != traversal_root && other.starts_with(traversal_root) && path.starts_with(other)
    })
}
