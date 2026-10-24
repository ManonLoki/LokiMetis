//! WorkBuddy project JSONL 的宿主读取：枚举文件、冻结只读长度、跨文件预算与解析缓存；逐行投影位于 core。

mod cache;
#[cfg(test)]
mod cache_tests;

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use loki_metris_core::session_logs::workbuddy::{
    MAX_USAGE_RECORDS, ProjectJsonlWarnings, parse_project_stream,
};
use loki_metris_core::{CoverageReport, CoverageState, WorkBuddyUsageEventRecord};

use crate::file_stamp::FileStamp;
use crate::local_index::{metadata_is_link_like, same_opened_file, walk_ancestors_for_link_component};

use super::WorkBuddyReadError;
use super::projects::discover_project_files;

/// 单次读取允许消耗的 project JSONL 总字节数。
const MAX_PROJECT_JSONL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 一次 project JSONL 读取的安全投影与覆盖结论。
pub(super) struct WorkBuddyProjectUsageRead {
    /// 读取到的用量记录。
    pub(super) records: Vec<WorkBuddyUsageEventRecord>,
    /// 覆盖结论。
    pub(super) coverage: CoverageReport,
}

/// 枚举并读取全部获批 project JSONL 布局；单文件失败时保留其它可用事实并标为部分覆盖。
pub(super) fn read_project_usage(
    workbuddy_home: &Path,
    root_id: &str,
) -> Result<WorkBuddyProjectUsageRead, WorkBuddyReadError> {
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
        match parse_project_stream(reader.take(allowed_len), source.line_source(), root_id, &mut records) {
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
    Ok(WorkBuddyProjectUsageRead {
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
