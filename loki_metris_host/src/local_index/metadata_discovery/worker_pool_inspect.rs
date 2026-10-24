// 本文件是 worker_pool.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：单目录元数据检查，以及本模块单元测试

/// 只读取一个目录的目录项与链接元数据，并在每个潜在阻塞边界检查预算。
fn inspect_directory_metadata(
    path: &Path,
    coordinator: &RootDiscoveryCoordinator,
    budget: MetadataTraversalBudget,
    entry_limit: usize,
) -> DirectoryResult {
    let mut result = DirectoryResult::default();
    if let Some(stop_reason) = budget.stop_reason(coordinator) {
        result.stop_reason = Some(stop_reason);
        return result;
    }
    if is_obviously_network_path(path) {
        result.skipped = 1;
        return result;
    }
    let mut entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            result.permission_denied = 1;
            return result;
        }
        Err(_) => {
            result.io_errors = 1;
            return result;
        }
    };
    if let Some(stop_reason) = budget.stop_reason(coordinator) {
        result.stop_reason = Some(stop_reason);
        return result;
    }

    let mut entries_checked = 0_usize;
    loop {
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let Some(entry) = entries.next() else {
            break;
        };
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        if entries_checked >= entry_limit {
            result.skipped = result.skipped.saturating_add(1);
            result.stop_reason = Some(TraversalStopReason::DirectoryEntryLimit);
            break;
        }
        entries_checked = entries_checked.saturating_add(1);
        let Ok(entry) = entry else {
            result.io_errors = result.io_errors.saturating_add(1);
            continue;
        };
        let entry_path = entry.path();
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let metadata = fs::symlink_metadata(&entry_path);
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            result.stop_reason = Some(stop_reason);
            break;
        }
        let Ok(metadata) = metadata else {
            result.io_errors = result.io_errors.saturating_add(1);
            continue;
        };
        if metadata_is_link_like(&metadata) {
            result.skipped = result.skipped.saturating_add(1);
            continue;
        }
        if metadata.is_dir() {
            result.children.push(entry_path);
        } else if metadata.is_file() {
            result.file_names_checked = result.file_names_checked.saturating_add(1);
            if let Some(candidate) = candidate_from_file_name(&entry_path) {
                result.candidates.push(candidate);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

    /// 创建运行中的测试协调器，供取消与 deadline 合同复用。
    fn running_coordinator() -> RootDiscoveryCoordinator {
        let coordinator = RootDiscoveryCoordinator::default();
        assert!(coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::FullLocalVolumes,
            1,
        ));
        coordinator
    }

    /// 目录项上限只处理允许数量，并以可观察停止原因拒绝伪称完整覆盖。
    #[test]
    fn directory_entry_limit_stops_before_extra_metadata() {
        let temp = tempfile::tempdir().expect("temporary directory");
        for name in ["a.jsonl", "b.jsonl", "c.jsonl"] {
            fs::write(temp.path().join(name), b"must not be opened").expect("fixture file");
        }
        let coordinator = running_coordinator();
        let result = inspect_directory_metadata(
            temp.path(),
            &coordinator,
            MetadataTraversalBudget::with_timeout(Duration::from_secs(1)),
            2,
        );

        assert_eq!(result.file_names_checked, 2);
        assert_eq!(result.skipped, 1);
        assert_eq!(
            result.stop_reason,
            Some(TraversalStopReason::DirectoryEntryLimit)
        );
    }

    /// 已请求取消时不得读取首个目录项或返回可继续入队、提交的结果。
    #[test]
    fn directory_inspection_observes_preexisting_cancellation() {
        let temp = tempfile::tempdir().expect("temporary directory");
        fs::write(temp.path().join("a.jsonl"), b"must not be opened").expect("fixture file");
        let coordinator = running_coordinator();
        assert!(coordinator.request_cancel());

        let result = inspect_directory_metadata(
            temp.path(),
            &coordinator,
            MetadataTraversalBudget::with_timeout(Duration::from_secs(1)),
            DIRECTORY_ENTRY_LIMIT,
        );

        assert_eq!(result.stop_reason, Some(TraversalStopReason::Cancelled));
        assert_eq!(result.file_names_checked, 0);
        assert!(result.children.is_empty());
        assert!(result.candidates.is_empty());
    }

    /// 没有结果到达时，接收循环必须由总 deadline 唤醒而不是永久阻塞。
    #[test]
    fn result_receive_is_bounded_by_total_deadline() {
        let coordinator = running_coordinator();
        let budget = MetadataTraversalBudget::with_timeout(Duration::from_millis(20));
        let (sender, results) = mpsc::channel();
        let started = Instant::now();
        let mut stop_reason = None;
        while stop_reason.is_none() {
            if let Some(reason) = budget.stop_reason(&coordinator) {
                stop_reason = Some(reason);
                break;
            }
            match results.recv_timeout(budget.result_wait()) {
                Ok(()) => unreachable!("test channel never sends"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => unreachable!("sender is retained"),
            }
        }
        drop(sender);

        assert_eq!(stop_reason, Some(TraversalStopReason::DeadlineExceeded));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// 卡住的原生线程句柄转交静态 owner 时不得阻塞调用方或丢失所有权。
    #[test]
    fn unfinished_worker_is_retained_without_blocking_drop() {
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_finished = Arc::clone(&finished);
        let (release_sender, release_receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = release_receiver.recv();
            worker_finished.store(true, Ordering::Release);
        });
        let started = Instant::now();
        retain_workers(vec![worker]);
        assert!(started.elapsed() < Duration::from_millis(100));

        release_sender.send(()).expect("release retained worker");
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            reap_finished_workers();
            if finished.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("retained worker did not become reapable");
    }
}
