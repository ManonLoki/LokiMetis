use std::sync::Arc;
use std::time::Duration;

use loki_metis_core::{
    LocalScanFuture, LocalScanOutput, LocalScanProgress, LocalUsageScanner, ScanCancellation,
    ScanKind, ScanStartOrigin, SourceRootReindexRequest, initial_coverage,
};

use super::{ScanTask, ScanTaskOperation, ScanTaskOwner, SpawnedScanFinalizer, spawn_scan_task};
use crate::backend::local_index::ScanCoordinator as LocalScanCoordinator;
use crate::dto::{AgentClientKindDto, ScanKindDto, ScanStateDto};
use crate::scan_state::ScanCoordinator;

/// 在扫描与单根重建入口稳定触发 panic 的测试扫描器。
struct PanicScanner;

impl LocalUsageScanner for PanicScanner {
    /// 返回一个轮询时立即 panic 的扫描 future。
    fn execute(
        &self,
        _kind: ScanKind,
        _origin: ScanStartOrigin,
        _cancellation: ScanCancellation,
        _on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(async { panic!("deterministic scanner panic") })
    }

    /// 返回一个轮询时立即 panic 的单根重建 future。
    fn reindex_source_root(
        &self,
        _request: SourceRootReindexRequest,
        _cancellation: ScanCancellation,
        _on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(async { panic!("deterministic reindex panic") })
    }
}

/// 有界等待测试扫描离开运行态并返回终态快照。
async fn wait_for_terminal(scan: &ScanCoordinator) -> crate::dto::ScanStatusDto {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let status = scan.snapshot();
            if status.state != ScanStateDto::Running {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("scan reaches a terminal state")
}

/// 扫描器 panic 必须在释放全局 writer 前转换为本 scan_id 的失败终态。
#[tokio::test]
async fn spawned_scan_panic_finishes_visible_state() {
    let local_scan = LocalScanCoordinator::default();
    let owner = ScanTaskOwner::new(local_scan.clone());
    let permit = local_scan.try_start().expect("writer starts");
    let cancellation = permit.cancellation_token();
    let scan = Arc::new(ScanCoordinator::default());
    let lease = scan
        .start(ScanKindDto::Quick, 1, cancellation.clone())
        .expect("visible scan starts");

    spawn_scan_task(
        &owner,
        ScanTask {
            scan_id: lease.scan_id.clone(),
            client: AgentClientKindDto::Codex,
            scanner: Arc::new(PanicScanner),
            operation: ScanTaskOperation::Scan {
                kind: ScanKindDto::Quick,
                origin: ScanStartOrigin::ExplicitUser,
            },
            cancellation,
            scan: Arc::clone(&scan),
            coverage_state: Arc::new(tokio::sync::RwLock::new(initial_coverage())),
            roots_state: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        },
        permit,
        None,
    )
    .expect("owner accepts scan task");

    let status = wait_for_terminal(&scan).await;
    assert_eq!(status.scan_id.as_deref(), Some(lease.scan_id.as_str()));
    assert_eq!(status.state, ScanStateDto::Failed);
    tokio::time::timeout(Duration::from_secs(1), async {
        while local_scan.is_running() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("panic path releases the global writer");
    owner.shutdown().await;
}

/// owner abort/drop 兜底必须把已取消任务按原 scan_id 收敛为 Cancelled。
#[tokio::test]
async fn aborted_scan_finalizer_finishes_only_its_visible_lease() {
    let scan = Arc::new(ScanCoordinator::default());
    let cancellation = ScanCancellation::new();
    let lease = scan
        .start(ScanKindDto::Quick, 1, cancellation.clone())
        .expect("visible scan starts");
    cancellation.cancel();

    drop(SpawnedScanFinalizer {
        scan_id: lease.scan_id.clone(),
        client: AgentClientKindDto::Codex,
        cancellation,
        scan: Arc::clone(&scan),
        armed: true,
    });

    let status = wait_for_terminal(&scan).await;
    assert_eq!(status.scan_id.as_deref(), Some(lease.scan_id.as_str()));
    assert_eq!(status.state, ScanStateDto::Cancelled);
}
