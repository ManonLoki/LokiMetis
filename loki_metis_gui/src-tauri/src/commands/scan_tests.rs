use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::dto::ScanStateDto;

use loki_metis_core::{
    LocalScanFuture, LocalScanOutput, LocalScanProgress, LocalUsageScanner, ScanCancellation,
    ScanKind, SourceRootReindexRequest,
};

/// 阻塞首个客户端直到 owner 取消，用于精确复现批次 shutdown 竞态。
struct CancellationBlockingScanner {
    calls: AtomicUsize,
    started: tokio::sync::Notify,
}

impl LocalUsageScanner for CancellationBlockingScanner {
    /// 记录调用次数，并让首个批次等待 owner 取消。
    fn execute(
        &self,
        _kind: ScanKind,
        _origin: ScanStartOrigin,
        cancellation: ScanCancellation,
        _on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            while !cancellation.is_cancelled() {
                tokio::task::yield_now().await;
            }
            Err("cancelled by fixture".to_owned())
        })
    }

    /// 此批次夹具不支持单根重建，调用即表示测试路径错误。
    fn reindex_source_root(
        &self,
        _request: SourceRootReindexRequest,
        _cancellation: ScanCancellation,
        _on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(async { unreachable!("batch fixture never reindexes one root") })
    }
}

/// 验证初始化批次只接受完成/部分完成，显式发现取消后仍可统一索引。
#[test]
fn refresh_trigger_maps_only_approved_discovery_states() {
    for lifecycle in [
        RootDiscoveryLifecycle::Idle,
        RootDiscoveryLifecycle::Running,
        RootDiscoveryLifecycle::Cancelled,
        RootDiscoveryLifecycle::Failed,
    ] {
        assert!(
            refresh_origin_for_context(
                LocalIndexRefreshTriggerDto::Initialization,
                false,
                lifecycle
            )
            .is_err()
        );
    }
    for lifecycle in [
        RootDiscoveryLifecycle::Complete,
        RootDiscoveryLifecycle::Partial,
    ] {
        let origin = refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::Initialization,
            false,
            lifecycle,
        )
        .expect("settled initialization discovery may index");
        assert_eq!(origin, ScanStartOrigin::ExplicitUser);
        assert_eq!(
            loki_metis_core::ensure_scan_start_allowed(
                origin,
                false,
                loki_metis_core::ScanKind::Quick,
            ),
            Ok(())
        );
    }
    assert_eq!(
        refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::DiscoveryBatch,
            true,
            RootDiscoveryLifecycle::Cancelled
        ),
        Ok(ScanStartOrigin::ExplicitUser)
    );
    assert!(
        refresh_origin_for_context(
            LocalIndexRefreshTriggerDto::DiscoveryBatch,
            true,
            RootDiscoveryLifecycle::Failed
        )
        .is_err()
    );
}

/// 验证批量输入固定按 Codex、Claude、Grok 排序，并拒绝空集与重复客户端。
#[test]
fn refresh_clients_are_unique_and_use_fixed_order() {
    assert_eq!(
        ordered_refresh_clients(&[
            AgentClientKindDto::GrokBuildCli,
            AgentClientKindDto::Codex,
            AgentClientKindDto::ClaudeCode,
        ]),
        Ok(vec![
            AgentClientKindDto::Codex,
            AgentClientKindDto::ClaudeCode,
            AgentClientKindDto::GrokBuildCli,
        ])
    );
    assert!(ordered_refresh_clients(&[]).is_err());
    assert!(
        ordered_refresh_clients(&[AgentClientKindDto::Codex, AgentClientKindDto::Codex]).is_err()
    );
}

/// 只有已开放且明确 NeedsRescan 的客户端进入启动后立即重建队列。
#[test]
fn upgrade_reindex_selection_is_scoped_and_ordered() {
    use loki_metis_core::{EnabledAgents, LocalIndexState, SourceClientKind};

    let enabled = EnabledAgents::empty()
        .with(SourceClientKind::Codex, true)
        .with(SourceClientKind::GrokBuildCli, true);
    let states = [
        (
            AgentClientKindDto::GrokBuildCli,
            LocalIndexState::NeedsRescan,
        ),
        (AgentClientKindDto::ClaudeCode, LocalIndexState::NeedsRescan),
        (AgentClientKindDto::Codex, LocalIndexState::Ready),
    ];
    assert_eq!(
        upgrade_reindex_clients(enabled, &states),
        vec![AgentClientKindDto::GrokBuildCli]
    );
}

/// 关闭门禁先到达时，direct Tauri refresh 必须在任何扫描状态或 writer 认领前拒绝。
#[tokio::test]
async fn direct_refresh_is_rejected_after_scan_owner_shutdown() {
    let temp = tempfile::tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());
    state.scan_tasks.shutdown().await;

    let error = refresh_local_indexes_for_command(
        &state,
        &[AgentClientKindDto::Codex],
        LocalIndexRefreshTriggerDto::DirectManual,
    )
    .await
    .expect_err("shutdown rejects direct refresh before it can claim state");

    assert_eq!(error, local_scan_writer_busy_message());
    assert_eq!(
        state
            .scans
            .get(AgentClientKindDto::Codex.into())
            .snapshot()
            .state,
        ScanStateDto::Idle
    );
    assert!(
        !state
            .local_scan
            .get(AgentClientKindDto::Codex.into())
            .is_running()
    );
}

/// shutdown 取消首个客户端后，同一显式批次不得再启动后续客户端。
#[tokio::test]
async fn shutdown_stops_explicit_batch_before_next_client() {
    let temp = tempfile::tempdir().expect("isolated app-data is available");
    let scanner = Arc::new(CancellationBlockingScanner {
        calls: AtomicUsize::new(0),
        started: tokio::sync::Notify::new(),
    });
    let mut state = AppRuntimeState::new(temp.path().to_path_buf());
    state
        .agent_clients
        .get_mut(AgentClientKindDto::Codex.into())
        .local_scanner = scanner.clone();
    state
        .agent_clients
        .get_mut(AgentClientKindDto::ClaudeCode.into())
        .local_scanner = scanner.clone();
    let state = Arc::new(state);
    let started = scanner.started.notified();
    let refresh_state = Arc::clone(&state);
    let mut refresh = tokio::spawn(async move {
        let _direct_scan = refresh_state
            .scan_tasks
            .register_direct_scan()
            .expect("fixture registers the direct batch");
        refresh_local_indexes_with_origin(
            &refresh_state,
            vec![AgentClientKindDto::Codex, AgentClientKindDto::ClaudeCode],
            ScanStartOrigin::ExplicitUser,
        )
        .await
    });
    tokio::select! {
        _ = started => {}
        result = &mut refresh => panic!("batch ended before scanner start: {result:?}"),
        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
            panic!("first client did not reach scanner start")
        }
    }

    state.scan_tasks.shutdown().await;
    let statuses = refresh
        .await
        .expect("batch task joins")
        .expect("cooperative cancellation is a terminal status");

    assert_eq!(scanner.calls.load(Ordering::SeqCst), 1);
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].state, ScanStateDto::Cancelled);
    assert_eq!(
        state
            .scans
            .get(AgentClientKindDto::ClaudeCode.into())
            .snapshot()
            .state,
        ScanStateDto::Idle
    );
}
