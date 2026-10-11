// 本文件是 manual_inspection.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use loki_metris_core::{RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStrategy};

    /// 记录最后一份命令预约引用是否已经释放。
    struct LeaseDropSignal(Arc<AtomicBool>);

    impl Drop for LeaseDropSignal {
        /// 最后一份预约离开 worker 时发布可观察信号。
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    /// 建立一轮手动子树协调状态，供取消与 deadline 合同回归复用。
    fn running_manual_coordinator() -> Arc<RootDiscoveryCoordinator> {
        let coordinator = Arc::new(RootDiscoveryCoordinator::default());
        assert!(coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::ManualSubtree,
            1,
        ));
        coordinator
    }

    /// 生产首次检查的四类签名预算都必须是明确有限值。
    #[test]
    fn manual_signature_options_are_finite() {
        let options = manual_signature_options();
        assert_eq!(
            options.max_signature_directories,
            MANUAL_SIGNATURE_DIRECTORY_LIMIT
        );
        assert_eq!(options.max_signature_entries, MANUAL_SIGNATURE_ENTRY_LIMIT);
        assert_eq!(options.max_signature_files, MANUAL_SIGNATURE_FILE_LIMIT);
        assert_eq!(options.max_signature_bytes, MANUAL_SIGNATURE_BYTE_LIMIT);
        assert!(options.max_signature_directories < u64::MAX);
        assert!(options.max_signature_entries < u64::MAX);
        assert!(options.max_signature_files < u64::MAX);
        assert!(options.max_signature_bytes < u64::MAX);
    }

    /// deadline 必须立即结束命令等待，但不合作 worker 与预约仍由 owner 持有。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deadline_retains_worker_and_lease_until_real_terminal_state() {
        let coordinator = running_manual_coordinator();
        let owner = BackgroundTaskOwner::new(Arc::clone(&coordinator));
        let cancellation = CancellationToken::new();
        let observed_cancellation = cancellation.clone();
        let release = Arc::new(AtomicBool::new(false));
        let task_release = Arc::clone(&release);
        let lease_dropped = Arc::new(AtomicBool::new(false));
        let lease = Arc::new(LeaseDropSignal(Arc::clone(&lease_dropped)));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let request = spawn_owned_manual_inspection(
            &owner,
            Arc::clone(&coordinator),
            cancellation,
            Arc::clone(&lease),
            move |_cancellation| {
                let _ = started_tx.send(());
                while !task_release.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                Ok(InspectBundle::found(PathBuf::from("/late-result")))
            },
        )
        .expect("manual inspection registers with owner");
        drop(lease);
        started_rx.await.expect("blocking worker starts");

        let started = tokio::time::Instant::now();
        let completion = request.wait(Duration::from_millis(20)).await;

        assert!(matches!(
            completion,
            ManualInspectionCompletion::DeadlineExceeded
        ));
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(observed_cancellation.is_cancelled());
        assert!(coordinator.is_cancel_requested());
        assert_eq!(owner.owned_task_count(), 1);
        assert!(!lease_dropped.load(Ordering::Acquire));

        release.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(1), owner.shutdown())
            .await
            .expect("owner reaps released worker within a bounded wait");
        assert_eq!(owner.owned_task_count(), 0);
        assert!(lease_dropped.load(Ordering::Acquire));
    }

    /// 显式协调器取消必须传播到签名令牌，并让等待方返回取消而非晚到结果。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coordinator_cancel_reaches_owned_inspection() {
        let coordinator = running_manual_coordinator();
        let owner = BackgroundTaskOwner::new(Arc::clone(&coordinator));
        let cancellation = CancellationToken::new();
        let observed_cancellation = cancellation.clone();
        let request = spawn_owned_manual_inspection(
            &owner,
            Arc::clone(&coordinator),
            cancellation,
            Arc::new(()),
            move |cancellation| {
                while !cancellation.is_cancelled() {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                Ok(InspectBundle::unverified(
                    ManualSourceInspectOutcome::Cancelled,
                ))
            },
        )
        .expect("manual inspection registers with owner");
        assert!(coordinator.request_cancel());

        let completion = tokio::time::timeout(
            Duration::from_secs(1),
            request.wait(Duration::from_secs(60)),
        )
        .await
        .expect("coordinator cancellation keeps command wait bounded");

        assert!(matches!(completion, ManualInspectionCompletion::Cancelled));
        assert!(observed_cancellation.is_cancelled());
        tokio::time::timeout(Duration::from_secs(1), owner.shutdown())
            .await
            .expect("cancelled worker reaches terminal state");
        assert_eq!(owner.owned_task_count(), 0);
    }

    /// 命令 future 被撤销时，结果票据 Drop 也必须取消 worker 与协调器。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropped_request_cancels_owned_inspection() {
        let coordinator = running_manual_coordinator();
        let owner = BackgroundTaskOwner::new(Arc::clone(&coordinator));
        let cancellation = CancellationToken::new();
        let observed_cancellation = cancellation.clone();
        let request = spawn_owned_manual_inspection(
            &owner,
            Arc::clone(&coordinator),
            cancellation,
            Arc::new(()),
            move |cancellation| {
                while !cancellation.is_cancelled() {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                Ok(InspectBundle::unverified(
                    ManualSourceInspectOutcome::Cancelled,
                ))
            },
        )
        .expect("manual inspection registers with owner");

        drop(request);

        assert!(observed_cancellation.is_cancelled());
        assert!(coordinator.is_cancel_requested());
        tokio::time::timeout(Duration::from_secs(1), owner.shutdown())
            .await
            .expect("dropped request worker reaches terminal state");
        assert_eq!(owner.owned_task_count(), 0);
    }
}
