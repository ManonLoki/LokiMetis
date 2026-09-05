//! WorkBuddy project JSONL 的受限、确定性路径枚举。

use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use loki_metis_core::{WORKBUDDY_PROJECTS_DIR_NAME, WorkbuddyUsageOrigin};

use crate::backend::local_index::{metadata_is_link_like, walk_ancestors_for_link_component};

use super::WorkbuddyReadError;

/// 单次读取最多处理的项目目录数。
const MAX_PROJECT_DIRECTORIES: usize = 10_000;
/// 单次读取最多观察的目录项数。
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
/// 单次读取最多接受的 transcript 文件数。
const MAX_TRANSCRIPT_FILES: usize = 25_000;

/// 一个已经通过相对布局与链接边界检查的 JSONL 候选。
pub(super) struct WorkbuddyProjectFile {
    pub(super) path: PathBuf,
    pub(super) relative_key: String,
    pub(super) project_key_material: String,
    pub(super) origin: WorkbuddyUsageOrigin,
}

/// 路径枚举的文件集合和覆盖计数。
#[derive(Default)]
pub(super) struct WorkbuddyProjectFiles {
    pub(super) files: Vec<WorkbuddyProjectFile>,
    pub(super) top_level_file_count: u64,
    pub(super) subagent_file_count: u64,
    pub(super) permission_denied_count: u64,
    pub(super) skipped_count: u64,
    pub(super) budget_exhausted: bool,
}

impl WorkbuddyProjectFiles {
    /// 返回两类受限来源文件总数。
    pub(super) fn file_count(&self) -> u64 {
        self.top_level_file_count
            .saturating_add(self.subagent_file_count)
    }
}

/// 只枚举 `projects/<project>/<session>.jsonl` 与一层 `subagents/*.jsonl`。
pub(super) fn discover_project_files(
    workbuddy_home: &Path,
) -> Result<WorkbuddyProjectFiles, WorkbuddyReadError> {
    let projects_root = workbuddy_home.join(WORKBUDDY_PROJECTS_DIR_NAME);
    let metadata = fs::symlink_metadata(&projects_root).map_err(map_root_error)?;
    if metadata_is_link_like(&metadata)
        || !metadata.is_dir()
        || walk_ancestors_for_link_component(&projects_root).unwrap_or(true)
    {
        return Err(WorkbuddyReadError::Read);
    }

    let mut result = WorkbuddyProjectFiles::default();
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
    result: &mut WorkbuddyProjectFiles,
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
                WorkbuddyUsageOrigin::TopLevel,
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
    result: &mut WorkbuddyProjectFiles,
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
            WorkbuddyUsageOrigin::Subagent,
            result,
        );
    }
}

/// 添加一个通过布局检查的普通文件并更新来源分类计数。
fn push_file(
    path: PathBuf,
    relative_key: String,
    project_key_material: &str,
    origin: WorkbuddyUsageOrigin,
    result: &mut WorkbuddyProjectFiles,
) {
    if walk_ancestors_for_link_component(&path).unwrap_or(true) {
        result.skipped_count = result.skipped_count.saturating_add(1);
        return;
    }
    match origin {
        WorkbuddyUsageOrigin::TopLevel => {
            result.top_level_file_count = result.top_level_file_count.saturating_add(1);
        }
        WorkbuddyUsageOrigin::Subagent => {
            result.subagent_file_count = result.subagent_file_count.saturating_add(1);
        }
    }
    result.files.push(WorkbuddyProjectFile {
        path,
        relative_key,
        project_key_material: project_key_material.to_owned(),
        origin,
    });
}

/// 读取并按文件名排序目录项，同时应用全局目录项预算。
fn read_sorted_entries(
    directory: &Path,
    result: &mut WorkbuddyProjectFiles,
    entry_budget: &mut usize,
) -> Result<Vec<DirEntry>, WorkbuddyReadError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        count_io_error(&error, result);
        WorkbuddyReadError::Read
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
fn ordinary_directory(entry: &DirEntry, result: &mut WorkbuddyProjectFiles) -> Option<PathBuf> {
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

/// 只有根目录确实缺失才属于未安装来源；权限和其它 I/O 错误保持读取失败。
fn map_root_error(error: std::io::Error) -> WorkbuddyReadError {
    if error.kind() == std::io::ErrorKind::NotFound {
        WorkbuddyReadError::SourceUnavailable
    } else {
        WorkbuddyReadError::Read
    }
}

/// 把 I/O 错误折叠成无路径覆盖计数。
fn count_io_error(error: &std::io::Error, result: &mut WorkbuddyProjectFiles) {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        result.permission_denied_count = result.permission_denied_count.saturating_add(1);
    } else {
        result.skipped_count = result.skipped_count.saturating_add(1);
    }
}
