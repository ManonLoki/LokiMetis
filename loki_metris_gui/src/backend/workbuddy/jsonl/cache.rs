//! 未改变的 project JSONL 复用上轮解析结果，避免后台摘要每 30 秒重读全部历史。
//!
//! 复用只依据"打开句柄时取得的文件戳 + 路径派生的来源属性"完全相同；文件仍在
//! 每轮重新枚举、打开并复核链接与句柄身份，因此消失、换名、权限变化与不安全
//! 目标照常失败关闭，只有内容读取阶段对未改变文件被省略。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use loki_metris_core::{WorkbuddyUsageEventRecord, WorkbuddyUsageOrigin};

use crate::backend::file_stamp::FileStamp;

use super::ProjectJsonlWarnings;
use super::super::projects::WorkbuddyProjectFile;

/// 进程内最多保留的解析记录数；超出后新文件不再入缓存，仍按原路径读取。
const MAX_CACHED_RECORDS: usize = 100_000;

/// 单个文件完整解析后的安全投影；记录数不超过全局记录预算时才会入缓存。
pub(super) struct ParsedFile {
    /// 解析时的文件戳；不一致即失效。
    stamp: FileStamp,
    /// 相对来源键。
    relative_key: String,
    /// 派生项目键的材料。
    project_key_material: String,
    /// 顶层或 subagent 来源。
    origin: WorkbuddyUsageOrigin,
    /// 解析出的用量记录。
    pub(super) records: Vec<WorkbuddyUsageEventRecord>,
    /// 质量计数。
    pub(super) warnings: ProjectJsonlWarnings,
}

impl ParsedFile {
    /// 记录一次完整解析的结果，来源属性一并保存以便命中时复核。
    pub(super) fn new(
        stamp: FileStamp,
        source: &WorkbuddyProjectFile,
        records: Vec<WorkbuddyUsageEventRecord>,
        warnings: ProjectJsonlWarnings,
    ) -> Self {
        Self {
            stamp,
            relative_key: source.relative_key.clone(),
            project_key_material: source.project_key_material.clone(),
            origin: source.origin,
            records,
            warnings,
        }
    }

    /// 仅当文件戳与路径派生属性都相同才视为同一份解析输入。
    fn matches(&self, stamp: &FileStamp, source: &WorkbuddyProjectFile) -> bool {
        self.stamp.is_cacheable() && stamp.is_cacheable() && self.stamp == *stamp
            && self.origin == source.origin
            && self.relative_key == source.relative_key
            && self.project_key_material == source.project_key_material
    }
}

/// 按（根 ID，文件路径）索引的已解析 WorkBuddy 文件缓存表。
type Entries = HashMap<(String, PathBuf), Arc<ParsedFile>>;

/// 全局缓存；键含固定根 ID，使国内版与国际版互不串用。
fn entries() -> &'static Mutex<Entries> {
    static ENTRIES: OnceLock<Mutex<Entries>> = OnceLock::new();
    ENTRIES.get_or_init(Default::default)
}

/// 命中时返回共享解析结果；锁只覆盖查找，记录复制发生在锁外。
pub(super) fn lookup(
    root_id: &str,
    stamp: &FileStamp,
    source: &WorkbuddyProjectFile,
) -> Option<Arc<ParsedFile>> {
    let entries = entries().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries
        .get(&(root_id.to_owned(), source.path.clone()))
        .filter(|parsed| parsed.matches(stamp, source))
        .cloned()
}

#[cfg(test)]
thread_local! {
    /// 本线程完成的完整解析次数；命中缓存的读取不会增加，供测试证明省略了内容读取。
    static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 本线程累计的完整解析次数。
#[cfg(test)]
pub(super) fn parses_on_this_thread() -> usize {
    PARSES.with(std::cell::Cell::get)
}

/// 保存完整解析结果；超过缓存记录上限时放弃保存，由下一轮继续读取该文件。
pub(super) fn store(root_id: &str, path: &Path, parsed: ParsedFile) {
    #[cfg(test)]
    PARSES.with(|count| count.set(count.get() + 1));
    let mut entries = entries().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = (root_id.to_owned(), path.to_owned());
    let others: usize = entries
        .iter()
        .filter(|(existing, _)| **existing != key)
        .map(|(_, parsed)| parsed.records.len())
        .sum();
    if parsed.stamp.is_cacheable() && others.saturating_add(parsed.records.len()) <= MAX_CACHED_RECORDS {
        entries.insert(key, Arc::new(parsed));
    } else {
        entries.remove(&key);
    }
}

/// 一轮读取成功结束后，丢弃该根下已不在枚举结果中的文件，防止缓存只增不减。
pub(super) fn retain_seen(root_id: &str, seen: &[&Path]) {
    let mut entries = entries().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    entries.retain(|(root, path), _| {
        root != root_id || seen.contains(&path.as_path())
    });
}
