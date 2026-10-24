//! WorkBuddy project JSONL 的受限、确定性路径枚举。

use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use loki_metris_core::{WORKBUDDY_PROJECTS_DIR_NAME, WorkBuddyUsageOrigin};

use crate::local_index::{metadata_is_link_like, walk_ancestors_for_link_component};

use super::WorkBuddyReadError;

/// 单次读取最多处理的项目目录数。
const MAX_PROJECT_DIRECTORIES: usize = 10_000;
/// 单次读取最多观察的目录项数。
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
/// 单次读取最多接受的 transcript 文件数。
const MAX_TRANSCRIPT_FILES: usize = 25_000;

/// 一个已经通过相对布局与链接边界检查的 JSONL 候选。
pub(super) struct WorkBuddyProjectFile {
    /// 文件路径。
    pub(super) path: PathBuf,
    /// 相对来源键。
    pub(super) relative_key: String,
    /// 派生项目键的材料。
    pub(super) project_key_material: String,
    /// 顶层或 subagent 来源。
    pub(super) origin: WorkBuddyUsageOrigin,
}

impl WorkBuddyProjectFile {
    /// 返回 core 行投影使用的不含路径的来源身份视图。
    pub(super) fn line_source(&self) -> loki_metris_core::session_logs::workbuddy::WorkBuddyLineSource<'_> {
        loki_metris_core::session_logs::workbuddy::WorkBuddyLineSource {
            relative_key: &self.relative_key,
            project_key_material: &self.project_key_material,
            origin: self.origin,
        }
    }
}

/// 路径枚举的文件集合和覆盖计数。
#[derive(Default)]
pub(super) struct WorkBuddyProjectFiles {
    /// 通过检查的 JSONL 候选。
    pub(super) files: Vec<WorkBuddyProjectFile>,
    /// 顶层会话文件数。
    pub(super) top_level_file_count: u64,
    /// subagent 文件数。
    pub(super) subagent_file_count: u64,
    /// 权限拒绝计数。
    pub(super) permission_denied_count: u64,
    /// 跳过计数。
    pub(super) skipped_count: u64,
    /// 预算已耗尽。
    pub(super) budget_exhausted: bool,
}

impl WorkBuddyProjectFiles {
    /// 返回两类受限来源文件总数。
    pub(super) fn file_count(&self) -> u64 {
        self.top_level_file_count
            .saturating_add(self.subagent_file_count)
    }
}

/// 只枚举 `projects/<project>/<session>.jsonl` 与一层 `subagents/*.jsonl`。
pub(super) fn discover_project_files(
    workbuddy_home: &Path,
) -> Result<WorkBuddyProjectFiles, WorkBuddyReadError> {
    let projects_root = workbuddy_home.join(WORKBUDDY_PROJECTS_DIR_NAME);
    let metadata = fs::symlink_metadata(&projects_root).map_err(map_root_error)?;
    if metadata_is_link_like(&metadata)
        || !metadata.is_dir()
        || walk_ancestors_for_link_component(&projects_root).unwrap_or(true)
    {
        return Err(WorkBuddyReadError::Read);
    }

    let mut result = WorkBuddyProjectFiles::default();
    let mut entry_budget = MAX_DIRECTORY_ENTRIES;
    let project_entries = read_sorted_entries(&projects_root, &mut result, &mut entry_budget)?;
    let mut project_count = 0_usize;
    for project_entry in project_entries {
        if project_count >= MAX_PROJECT_DIRECTORIES || result.files.len() >= MAX_TRANSCRIPT_FILES {
            result.budget_exhausted = true;
            break;
        }
        let Some(project_name) = project_entry.file_name().to_str().map(ToOwned::to_owned) else {
            result.skipped_count = result.skipped_count.saturating_add(1);
            continue;
        };
        let Some(project_dir) = ordinary_directory(&project_entry, &mut result) else {
            continue;
        };
        project_count += 1;
        enumerate_project(&project_dir, &project_name, &mut result, &mut entry_budget);
    }
    result
        .files
        .sort_by(|left, right| left.relative_key.cmp(&right.relative_key));
    Ok(result)
}

/// 枚举单个 project 目录的顶层 transcript 与 session/subagents 目录。
fn enumerate_project(
    project_dir: &Path,
    project_name: &str,
    result: &mut WorkBuddyProjectFiles,
    entry_budget: &mut usize,
) {
    let Ok(entries) = read_sorted_entries(project_dir, result, entry_budget) else {
        return;
    };
    for entry in entries {
        if result.files.len() >= MAX_TRANSCRIPT_FILES {
            result.budget_exhausted = true;
            return;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                count_io_error(&error, result);
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            result.skipped_count = result.skipped_count.saturating_add(1);
            continue;
        }
        if metadata.is_file() && has_jsonl_extension(&path) {
            push_file(
                path,
                format!("{project_name}/{}", entry.file_name().to_string_lossy()),
                project_name,
                WorkBuddyUsageOrigin::TopLevel,
                result,
            );
            continue;
        }
        if metadata.is_dir() {
            enumerate_subagents(&path, project_name, result, entry_budget);
        }
    }
}

/// 只进入 session 目录下字面量为 `subagents` 的一层子目录。
fn enumerate_subagents(
    session_dir: &Path,
    project_name: &str,
    result: &mut WorkBuddyProjectFiles,
    entry_budget: &mut usize,
) {
    let Some(session_name) = session_dir.file_name().and_then(|name| name.to_str()) else {
        result.skipped_count = result.skipped_count.saturating_add(1);
        return;
    };
    let subagents_dir = session_dir.join("subagents");
    let metadata = match fs::symlink_metadata(&subagents_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            count_io_error(&error, result);
            return;
        }
    };
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        result.skipped_count = result.skipped_count.saturating_add(1);
        return;
    }
    let Ok(entries) = read_sorted_entries(&subagents_dir, result, entry_budget) else {
        return;
    };
    for entry in entries {
        if result.files.len() >= MAX_TRANSCRIPT_FILES {
            result.budget_exhausted = true;
            return;
        }
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                count_io_error(&error, result);
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            result.skipped_count = result.skipped_count.saturating_add(1);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if !has_jsonl_extension(&path) {
            continue;
        }
        push_file(
            path,
            format!(
                "{project_name}/{session_name}/subagents/{}",
                entry.file_name().to_string_lossy()
            ),
            project_name,
            WorkBuddyUsageOrigin::Subagent,
            result,
        );
    }
}

/// 添加一个通过布局检查的普通文件并更新来源分类计数。
fn push_file(
    path: PathBuf,
    relative_key: String,
    project_key_material: &str,
    origin: WorkBuddyUsageOrigin,
    result: &mut WorkBuddyProjectFiles,
) {
    if walk_ancestors_for_link_component(&path).unwrap_or(true) {
        result.skipped_count = result.skipped_count.saturating_add(1);
        return;
    }
    match origin {
        WorkBuddyUsageOrigin::TopLevel => {
            result.top_level_file_count = result.top_level_file_count.saturating_add(1);
        }
        WorkBuddyUsageOrigin::Subagent => {
            result.subagent_file_count = result.subagent_file_count.saturating_add(1);
        }
    }
    result.files.push(WorkBuddyProjectFile {
        path,
        relative_key,
        project_key_material: project_key_material.to_owned(),
        origin,
    });
}

/// 读取并按文件名排序目录项，同时应用全局目录项预算。
fn read_sorted_entries(
    directory: &Path,
    result: &mut WorkBuddyProjectFiles,
    entry_budget: &mut usize,
) -> Result<Vec<DirEntry>, WorkBuddyReadError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        count_io_error(&error, result);
        WorkBuddyReadError::Read
    })?;
    let mut values = Vec::new();
    for entry in entries {
        if *entry_budget == 0 {
            result.budget_exhausted = true;
            break;
        }
        *entry_budget -= 1;
        match entry {
            Ok(entry) => values.push(entry),
            Err(error) => count_io_error(&error, result),
        }
    }
    values.sort_by_key(DirEntry::file_name);
    Ok(values)
}

/// 只接受未链接的普通目录。
fn ordinary_directory(entry: &DirEntry, result: &mut WorkBuddyProjectFiles) -> Option<PathBuf> {
    let path = entry.path();
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata_is_link_like(&metadata) && metadata.is_dir() => Some(path),
        Ok(metadata) if metadata_is_link_like(&metadata) => {
            result.skipped_count = result.skipped_count.saturating_add(1);
            None
        }
        Ok(_) => None,
        Err(error) => {
            count_io_error(&error, result);
            None
        }
    }
}

/// 扩展名按 WorkBuddy 固定小写形态精确匹配。
fn has_jsonl_extension(path: &Path) -> bool {
    path.extension().and_then(|value| value.to_str()) == Some("jsonl")
}

/// 只有根目录确实缺失才属于来源缺失；权限和其它 I/O 错误保持读取失败。
fn map_root_error(error: std::io::Error) -> WorkBuddyReadError {
    if error.kind() == std::io::ErrorKind::NotFound {
        WorkBuddyReadError::SourceUnavailable
    } else {
        WorkBuddyReadError::Read
    }
}

/// 把 I/O 错误折叠成无路径覆盖计数。
fn count_io_error(error: &std::io::Error, result: &mut WorkBuddyProjectFiles) {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        result.permission_denied_count = result.permission_denied_count.saturating_add(1);
    } else {
        result.skipped_count = result.skipped_count.saturating_add(1);
    }
}
