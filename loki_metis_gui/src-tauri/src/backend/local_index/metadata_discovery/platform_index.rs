//! 使用系统元数据索引提前返回 JSONL 文件名结果，不承担完整覆盖证明。

use std::path::PathBuf;

use loki_metis_core::RootDiscoveryCoordinator;

use super::super::LocalVolumeRoots;

/// Windows 通过进程内 WinRT Search API 分页查询索引中的 JSONL 路径。
#[cfg(target_os = "windows")]
pub(super) fn query(
    volumes: &LocalVolumeRoots,
    coordinator: &RootDiscoveryCoordinator,
) -> (bool, Vec<PathBuf>) {
    use windows::Storage::Search::{FolderDepth, IndexerOption, QueryOptions};
    use windows::Storage::StorageFolder;
    use windows::core::HSTRING;

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
    let mut any_query_succeeded = false;
    for root in &volumes.search_roots {
        if coordinator.is_cancel_requested() {
            break;
        }
        let Ok(folder_operation) =
            StorageFolder::GetFolderFromPathAsync(&HSTRING::from(root.to_string_lossy().as_ref()))
        else {
            continue;
        };
        let Ok(folder) = folder_operation.get() else {
            continue;
        };
        let Ok(query) = folder.CreateFileQueryWithOptions(&options) else {
            continue;
        };
        let mut offset = 0_u32;
        loop {
            let Ok(operation) = query.GetFilesAsync(offset, 512) else {
                break;
            };
            let Ok(files) = operation.get() else {
                break;
            };
            any_query_succeeded = true;
            let Ok(size) = files.Size() else {
                break;
            };
            for index in 0..size {
                let Ok(file) = files.GetAt(index) else {
                    continue;
                };
                if let Ok(path) = file.Path() {
                    paths.push(PathBuf::from(path.to_string()));
                }
            }
            if size < 512 {
                break;
            }
            offset = offset.saturating_add(size);
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
    use std::time::{Duration, Instant};

    use objc2_foundation::{NSDate, NSMetadataQuery, NSPredicate, NSRunLoop, NSString};

    use super::super::{LocalPathStatus, classify_local_path};

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
    let deadline = Instant::now() + Duration::from_secs(5);
    while query.isGathering() && Instant::now() < deadline {
        if coordinator.is_cancel_requested() {
            query.stopQuery();
            return (true, Vec::new());
        }
        run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.05));
    }
    query.disableUpdates();
    // Apple 文档中 `NSMetadataItemPathKey` 的字面值是 `kMDItemPath`；用字面量避免
    // 读取 objc2 的 extern static（本 crate 禁止 unsafe-code）。
    let path_attribute = NSString::from_str("kMDItemPath");
    let paths = (0..query.resultCount())
        .filter_map(|index| {
            query
                .valueOfAttribute_forResultAtIndex(&path_attribute, index)
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|path| PathBuf::from(path.to_string()))
        })
        .filter(|path| matches!(classify_local_path(path), LocalPathStatus::ConfirmedLocal))
        .collect();
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
