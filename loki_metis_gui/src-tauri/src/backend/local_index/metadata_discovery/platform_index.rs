//! 使用系统元数据索引提前返回 JSONL 文件名结果，不承担完整覆盖证明。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use loki_metis_core::RootDiscoveryCoordinator;

use super::super::LocalVolumeRoots;

/// 系统索引只负责有界加速；完整覆盖仍由随后执行的普通元数据遍历证明。
const SYSTEM_INDEX_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// 一次系统索引查询最多检查的结果数，防止平台 API 返回无界集合。
const SYSTEM_INDEX_RESULT_LIMIT: usize = 10_000;

/// 把取消、总墙钟预算和结果数量上限折叠成每个页/行边界复用的判定。
#[derive(Debug, Clone, Copy)]
struct SystemIndexBudget {
    deadline: Instant,
}

impl SystemIndexBudget {
    /// 从当前时刻创建系统索引查询的固定总预算。
    fn new() -> Self {
        Self {
            deadline: Instant::now() + SYSTEM_INDEX_QUERY_TIMEOUT,
        }
    }

    /// 在下一页或下一行处理前统一检查取消、deadline 与结果上限。
    fn allows_next(self, coordinator: &RootDiscoveryCoordinator, inspected_results: usize) -> bool {
        !coordinator.is_cancel_requested()
            && Instant::now() < self.deadline
            && inspected_results < SYSTEM_INDEX_RESULT_LIMIT
    }

    #[cfg(target_os = "windows")]
    /// 返回不越过总 deadline 的 WinRT 状态轮询间隔。
    fn poll_delay(self) -> Duration {
        self.deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(10))
    }
}

/// 轮询 WinRT operation 的状态，使调用方能够在同一个总 deadline 内取消操作。
#[cfg(target_os = "windows")]
fn wait_for_windows_operation<T>(
    coordinator: &RootDiscoveryCoordinator,
    budget: SystemIndexBudget,
    inspected_results: usize,
    status: impl Fn() -> windows::core::Result<i32>,
    cancel: impl Fn(),
    get_results: impl FnOnce() -> windows::core::Result<T>,
) -> Option<T> {
    loop {
        if !budget.allows_next(coordinator, inspected_results) {
            cancel();
            return None;
        }
        match status() {
            Ok(0) => std::thread::sleep(budget.poll_delay()),
            Ok(1) => return get_results().ok(),
            Ok(_) | Err(_) => {
                cancel();
                return None;
            }
        }
    }
}

/// Windows 通过进程内 WinRT Search API 分页查询索引中的 JSONL 路径。
#[cfg(target_os = "windows")]
pub(super) fn query(
    volumes: &LocalVolumeRoots,
    coordinator: &RootDiscoveryCoordinator,
) -> (bool, Vec<PathBuf>) {
    use windows::Storage::Search::{FolderDepth, IndexerOption, QueryOptions};
    use windows::Storage::StorageFolder;
    use windows::core::HSTRING;

    /// Windows Search 每页最多读取的索引结果数。
    const PAGE_SIZE: u32 = 512;

    let budget = SystemIndexBudget::new();
    let options = match QueryOptions::new() {
        Ok(options) => options,
        Err(_) => return (false, Vec::new()),
    };
    if options.SetFolderDepth(FolderDepth::Deep).is_err()
        || options
            .SetIndexerOption(IndexerOption::OnlyUseIndexer)
            .is_err()
        || options
            .FileTypeFilter()
            .and_then(|filter| filter.Append(&HSTRING::from(".jsonl")))
            .is_err()
    {
        return (false, Vec::new());
    }
    let mut paths = Vec::new();
    let mut inspected_results = 0_usize;
    let mut any_query_succeeded = false;
    for root in &volumes.search_roots {
        if !budget.allows_next(coordinator, inspected_results) {
            break;
        }
        let Ok(folder_operation) =
            StorageFolder::GetFolderFromPathAsync(&HSTRING::from(root.to_string_lossy().as_ref()))
        else {
            continue;
        };
        let Some(folder) = wait_for_windows_operation(
            coordinator,
            budget,
            inspected_results,
            || folder_operation.Status().map(|status| status.0),
            || {
                let _ = folder_operation.Cancel();
            },
            || folder_operation.GetResults(),
        ) else {
            if !budget.allows_next(coordinator, inspected_results) {
                break;
            }
            continue;
        };
        let Ok(query) = folder.CreateFileQueryWithOptions(&options) else {
            continue;
        };
        let mut offset = 0_u32;
        loop {
            if !budget.allows_next(coordinator, inspected_results) {
                return (any_query_succeeded, paths);
            }
            let remaining = SYSTEM_INDEX_RESULT_LIMIT.saturating_sub(inspected_results);
            let requested = u32::try_from(remaining).unwrap_or(u32::MAX).min(PAGE_SIZE);
            let Ok(operation) = query.GetFilesAsync(offset, requested) else {
                break;
            };
            let Some(files) = wait_for_windows_operation(
                coordinator,
                budget,
                inspected_results,
                || operation.Status().map(|status| status.0),
                || {
                    let _ = operation.Cancel();
                },
                || operation.GetResults(),
            ) else {
                if !budget.allows_next(coordinator, inspected_results) {
                    return (any_query_succeeded, paths);
                }
                break;
            };
            any_query_succeeded = true;
            let Ok(size) = files.Size() else {
                break;
            };
            for index in 0..size {
                if !budget.allows_next(coordinator, inspected_results) {
                    return (any_query_succeeded, paths);
                }
                inspected_results = inspected_results.saturating_add(1);
                let Ok(file) = files.GetAt(index) else {
                    continue;
                };
                if let Ok(path) = file.Path() {
                    paths.push(PathBuf::from(path.to_string()));
                }
            }
            if size < requested {
                break;
            }
            let Some(next_offset) = offset.checked_add(size) else {
                break;
            };
            offset = next_offset;
        }
    }
    (any_query_succeeded, paths)
}

/// macOS 通过进程内 `NSMetadataQuery` 查询 Spotlight，结果仍逐项执行本地卷策略。
#[cfg(target_os = "macos")]
pub(super) fn query(
    _volumes: &LocalVolumeRoots,
    coordinator: &RootDiscoveryCoordinator,
) -> (bool, Vec<PathBuf>) {
    use objc2_foundation::{NSDate, NSMetadataQuery, NSPredicate, NSRunLoop, NSString};

    use super::super::{LocalPathStatus, classify_local_path};

    let budget = SystemIndexBudget::new();
    let Some(predicate) = NSPredicate::predicateFromMetadataQueryString(&NSString::from_str(
        "kMDItemFSName == '*.jsonl'cd",
    )) else {
        return (false, Vec::new());
    };
    let query = NSMetadataQuery::new();
    query.setPredicate(Some(&predicate));
    if !query.startQuery() {
        return (false, Vec::new());
    }

    let run_loop = NSRunLoop::currentRunLoop();
    while query.isGathering() && budget.allows_next(coordinator, 0) {
        if !budget.allows_next(coordinator, 0) {
            query.stopQuery();
            return (true, Vec::new());
        }
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    query.disableUpdates();
    // Apple 文档中 `NSMetadataItemPathKey` 的字面值是 `kMDItemPath`；用字面量避免
    // 读取 objc2 的 extern static（本 crate 禁止 unsafe-code）。
    let path_attribute = NSString::from_str("kMDItemPath");
    let mut paths = Vec::new();
    let mut inspected_results = 0_usize;
    for index in 0..query.resultCount() {
        if !budget.allows_next(coordinator, inspected_results) {
            break;
        }
        inspected_results = inspected_results.saturating_add(1);
        let Some(path) = query
            .valueOfAttribute_forResultAtIndex(&path_attribute, index)
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|path| PathBuf::from(path.to_string()))
        else {
            continue;
        };
        if matches!(classify_local_path(&path), LocalPathStatus::ConfirmedLocal) {
            paths.push(path);
        }
    }
    query.stopQuery();
    (true, paths)
}

/// 其他平台没有批准的系统索引加速器；调用方仍执行普通元数据兜底。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(super) fn query(
    _volumes: &LocalVolumeRoots,
    _coordinator: &RootDiscoveryCoordinator,
) -> (bool, Vec<PathBuf>) {
    (false, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

    /// 结果上限和用户取消都必须在下一条索引结果进入处理前生效。
    #[test]
    fn system_index_budget_stops_at_result_limit_and_cancellation() {
        let coordinator = RootDiscoveryCoordinator::default();
        assert!(coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        let budget = SystemIndexBudget::new();
        assert!(budget.allows_next(&coordinator, SYSTEM_INDEX_RESULT_LIMIT - 1));
        assert!(!budget.allows_next(&coordinator, SYSTEM_INDEX_RESULT_LIMIT));
        assert!(coordinator.request_cancel());
        assert!(!budget.allows_next(&coordinator, 0));
    }
}
