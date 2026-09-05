//! WorkBuddy 本机统计 adapter：只读 project JSONL 用量与独立 Trace 诊断。
//!
//! 用量唯一来自 `projects/<project>/<session>.jsonl` 和对应一层
//! `subagents/*.jsonl`。adapter 会解析完整 JSON 行，但只投影直接
//! `providerData.usage`、实际模型、时间戳与积分；正文不会进入返回值、索引或日志。

mod jsonl;
mod projects;

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use loki_metis_core::{
    LocalUsageWindow, SourceClientKind, TimeStandard, UsageDimension, UsageStatisticsPage,
    WorkbuddyModelUsageWindow, WorkbuddyScanSource, WorkbuddyStatisticsSnapshot,
    WorkbuddyTraceRecord, WorkbuddyTraceStatus, build_workbuddy_usage_details,
    compute_workbuddy_statistics_with_standard, discover_workbuddy_scan_source, path_key,
    source_root_alias_from_path, stable_id, workbuddy_home_from_user_home,
};
use serde::Deserialize;

use crate::backend::local_index::current_user_home;

use self::jsonl::{open_fixed_prefix, read_project_usage};
use self::projects::{WorkbuddyProjectFiles, discover_project_files};

/// WorkBuddy trace 记录子目录固定名称。
const WORKBUDDY_TRACES_DIR_NAME: &str = "traces";
/// 单次读取最多扫描的 trace 文件数。
const MAX_TRACE_FILES: usize = 5_000;
/// 单次 Trace 诊断最多观察的目录项数，限制大量无效文件或目录造成的资源消耗。
const MAX_TRACE_DIRECTORY_ENTRIES: usize = 25_000;
/// 单个 trace 允许的最大字节数；超限只影响独立诊断，不影响用量。
const MAX_TRACE_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// 单次 trace 诊断允许读取的总字节数。
const MAX_TRACE_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

/// 标识 WorkBuddy 本地统计读取失败的稳定类别，不携带路径细节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkbuddyReadError {
    /// 未定位安装目录，或获批 `projects` 根缺失/不是普通目录。
    SourceUnavailable,
    /// 目录或文件在读取过程中不可安全读取。
    Read,
}

/// 数据源页可展示的 project JSONL 结构证据；不包含任何路径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorkbuddyProjectSourceEvidence {
    /// 顶层与 subagent transcript 文件总数。
    pub(crate) file_count: u64,
    /// 顶层 transcript 文件数。
    pub(crate) top_level_file_count: u64,
    /// subagent transcript 文件数。
    pub(crate) subagent_file_count: u64,
    /// 枚举时权限拒绝数量。
    pub(crate) permission_denied_count: u64,
    /// 链接、I/O 或不安全候选跳过数量。
    pub(crate) skipped_count: u64,
    /// 是否触发有界枚举预算。
    pub(crate) budget_exhausted: bool,
}

impl WorkbuddyProjectSourceEvidence {
    /// 从内部枚举结果复制无路径计数。
    fn from_files(files: &WorkbuddyProjectFiles) -> Self {
        Self {
            file_count: files.file_count(),
            top_level_file_count: files.top_level_file_count,
            subagent_file_count: files.subagent_file_count,
            permission_denied_count: files.permission_denied_count,
            skipped_count: files.skipped_count,
            budget_exhausted: files.budget_exhausted,
        }
    }
}

/// 返回当前用户的 WorkBuddy 安装目录；无法解析主目录时返回 `None`。
pub(crate) fn resolve_workbuddy_home() -> Option<PathBuf> {
    current_user_home().map(|home| workbuddy_home_from_user_home(&home))
}

/// 只读探测当前用户是否存在普通的 WorkBuddy 安装目录。
pub(crate) fn discover_workbuddy_source() -> Option<WorkbuddyScanSource> {
    let home = current_user_home()?;
    discover_workbuddy_scan_source(&home)
}

/// 检查获批 project JSONL 布局并返回无路径证据；不会解析记录正文。
pub(crate) fn inspect_workbuddy_project_source(
    workbuddy_home: &Path,
) -> Result<WorkbuddyProjectSourceEvidence, WorkbuddyReadError> {
    discover_project_files(workbuddy_home)
        .map(|files| WorkbuddyProjectSourceEvidence::from_files(&files))
}

/// 读取 project JSONL 与 Trace，并按查看时间标准生成统计快照。
pub(crate) async fn read_workbuddy_statistics(
    workbuddy_home: &Path,
    now_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<WorkbuddyStatisticsSnapshot, WorkbuddyReadError> {
    // 用量与 Trace 走两棵互不相关的目录树，先各自起线程再一起等待。
    let trace_home = workbuddy_home.to_path_buf();
    let trace_task = tokio::task::spawn_blocking(move || read_traces(&trace_home));
    let (inputs, traces) = tokio::join!(read_workbuddy_usage_inputs(workbuddy_home), trace_task);
    let inputs = inputs?;
    let traces = traces.unwrap_or_default();
    Ok(compute_workbuddy_statistics_with_standard(
        &inputs.records,
        &traces,
        &inputs.coverage,
        now_epoch_ms,
        &time_standard,
        &jiff::tz::TimeZone::system(),
    ))
}

/// 只读取 project JSONL 用量，不触碰与 Collect/模型表无关的 Trace 目录。
pub(crate) async fn read_workbuddy_usage_snapshot(
    workbuddy_home: &Path,
    now_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<WorkbuddyStatisticsSnapshot, WorkbuddyReadError> {
    let inputs = read_workbuddy_usage_inputs(workbuddy_home).await?;
    Ok(compute_workbuddy_statistics_with_standard(
        &inputs.records,
        &[],
        &inputs.coverage,
        now_epoch_ms,
        &time_standard,
        &jiff::tz::TimeZone::system(),
    ))
}

/// 一次 WorkBuddy 读取返回同一快照下的通用统计页与逐模型明细。
pub(crate) struct WorkbuddyUsageDetails {
    pub(crate) statistics: UsageStatisticsPage,
    pub(crate) model_usage: WorkbuddyModelUsageWindow,
}

/// 用同一批 JSONL 记录构造统计页和模型表，避免跨午夜或活跃追加造成漂移。
pub(crate) async fn read_workbuddy_usage_details(
    workbuddy_home: &Path,
    window: LocalUsageWindow,
    dimension: UsageDimension,
    now_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<WorkbuddyUsageDetails, WorkbuddyReadError> {
    let inputs = read_workbuddy_usage_inputs(workbuddy_home).await?;
    let device_tz = jiff::tz::TimeZone::system();
    let (statistics, model_usage) = build_workbuddy_usage_details(
        &inputs.records,
        &inputs.coverage,
        &inputs.root_id,
        &inputs.root_alias,
        window,
        dimension,
        now_epoch_ms,
        &time_standard,
        &device_tz,
    )
    .map_err(|_| WorkbuddyReadError::Read)?;
    Ok(WorkbuddyUsageDetails {
        statistics,
        model_usage,
    })
}

/// adapter 内一次读取的全部安全输入。
struct WorkbuddyUsageInputs {
    records: Vec<loki_metis_core::WorkbuddyUsageEventRecord>,
    coverage: loki_metis_core::CoverageReport,
    root_id: String,
    root_alias: String,
}

/// 在阻塞线程中读取主用量；调用方按界面需要决定是否另读 Trace。
async fn read_workbuddy_usage_inputs(
    workbuddy_home: &Path,
) -> Result<WorkbuddyUsageInputs, WorkbuddyReadError> {
    let root_id = stable_id(
        SourceClientKind::WorkBuddy.root_id_namespace(),
        &path_key(workbuddy_home),
    );
    let root_alias = source_root_alias_from_path(workbuddy_home, SourceClientKind::WorkBuddy);
    let usage_home = workbuddy_home.to_path_buf();
    let usage_root_id = root_id.clone();
    let usage_task =
        tokio::task::spawn_blocking(move || read_project_usage(&usage_home, &usage_root_id));
    let usage = usage_task.await.map_err(|_| WorkbuddyReadError::Read)??;
    Ok(WorkbuddyUsageInputs {
        records: usage.records,
        coverage: usage.coverage,
        root_id,
        root_alias,
    })
}

/// trace JSON 文件的最小反序列化形状；未声明的 spans/正文由 serde 丢弃。
#[derive(Deserialize)]
struct TraceFileEnvelope {
    trace: TraceMeta,
}

/// Trace 仅保留开始时间、耗时和整体状态。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TraceMeta {
    started_at: String,
    duration: i64,
    status: String,
}

/// 只遍历 `traces/<pid>/*.json` 两层普通目录/文件，并应用文件与字节预算。
fn read_traces(workbuddy_home: &Path) -> Vec<WorkbuddyTraceRecord> {
    let traces_dir = workbuddy_home.join(WORKBUDDY_TRACES_DIR_NAME);
    let Ok(metadata) = fs::symlink_metadata(&traces_dir) else {
        return Vec::new();
    };
    if crate::backend::local_index::metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Vec::new();
    }
    let mut remaining_entries = MAX_TRACE_DIRECTORY_ENTRIES;
    let Ok(mut pid_entries) = sorted_entries(&traces_dir, &mut remaining_entries) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    let mut remaining_bytes = MAX_TRACE_TOTAL_BYTES;
    for pid_entry in pid_entries.drain(..) {
        let pid_path = pid_entry.path();
        let Ok(pid_metadata) = fs::symlink_metadata(&pid_path) else {
            continue;
        };
        if crate::backend::local_index::metadata_is_link_like(&pid_metadata)
            || !pid_metadata.is_dir()
        {
            continue;
        }
        if remaining_entries == 0 {
            return records;
        }
        let Ok(file_entries) = sorted_entries(&pid_path, &mut remaining_entries) else {
            continue;
        };
        for file_entry in file_entries {
            if records.len() >= MAX_TRACE_FILES || remaining_bytes == 0 {
                return records;
            }
            let path = file_entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some((record, consumed)) = read_trace_file(&path, remaining_bytes) else {
                continue;
            };
            remaining_bytes = remaining_bytes.saturating_sub(consumed);
            records.push(record);
        }
    }
    records
}

/// 确定性读取一个目录的直接子项。
fn sorted_entries(
    directory: &Path,
    remaining_entries: &mut usize,
) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)? {
        if *remaining_entries == 0 {
            break;
        }
        *remaining_entries -= 1;
        entries.push(entry?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

/// 从固定文件前缀流式解析一个完整 Trace；活动半写文件会自然被忽略。
fn read_trace_file(path: &Path, remaining_bytes: u64) -> Option<(WorkbuddyTraceRecord, u64)> {
    let (file, observed_len) = open_fixed_prefix(path).ok()?;
    if observed_len == 0 || observed_len > MAX_TRACE_FILE_BYTES || observed_len > remaining_bytes {
        return None;
    }
    let reader = BufReader::new(file.take(observed_len));
    let envelope: TraceFileEnvelope = serde_json::from_reader(reader).ok()?;
    let status = match envelope.trace.status.as_str() {
        "ok" => WorkbuddyTraceStatus::Ok,
        "error" => WorkbuddyTraceStatus::Error,
        "cancelled" | "canceled" => WorkbuddyTraceStatus::Cancelled,
        _ => return None,
    };
    let started_at_epoch_ms = envelope
        .trace
        .started_at
        .parse::<jiff::Timestamp>()
        .ok()?
        .as_millisecond();
    Some((
        WorkbuddyTraceRecord {
            started_at_epoch_ms,
            duration_ms: envelope.trace.duration.max(0),
            status,
        },
        observed_len,
    ))
}

#[cfg(test)]
mod tests;
