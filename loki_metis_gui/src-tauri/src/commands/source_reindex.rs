//! 单个已启用数据根的显式重新索引 command。

use std::sync::Arc;

use loki_metis_core::{
    ScanStartOrigin, local_scan_in_progress_error_message,
    source_root_operations_blocked_by_scan_message,
};
use tauri::{AppHandle, State};

use super::access::ensure_scan_start_access_by_policy;
use super::ensure_business_access;
use super::scan_orchestration::{ScanTask, ScanTaskOperation, spawn_scan_task};
use crate::dto::{AgentClientKindDto, ScanKindDto, ScanStateDto, ScanStatusDto};
use crate::runtime::{AppRuntimeState, now_epoch_ms};
use crate::source_commands::validated_source_root_reindex_request;

/// 针对一个已启用数据根启动强制重建；不做全设备发现，也不预删旧索引。
async fn reindex_source_root_for_state(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    root_id: &str,
    app_handle: Option<AppHandle>,
) -> Result<ScanStatusDto, String> {
    let _scan_reservation = state
        .scan_tasks
        .register_direct_scan()
        .map_err(str::to_owned)?;
    ensure_business_access(state).await?;
    ensure_scan_start_access_by_policy(state, ScanStartOrigin::ExplicitUser, ScanKindDto::Quick)
        .await?;
    let scan_state = Arc::clone(state.scans.get(client.into()));
    if scan_state.snapshot().await.state == ScanStateDto::Running {
        return Err(local_scan_in_progress_error_message().to_owned());
    }
    let permit = state
        .local_scan
        .get(client.into())
        .try_start()
        .map_err(|_| source_root_operations_blocked_by_scan_message().to_owned())?;
    let cancellation = permit.cancellation_token();
    let request = validated_source_root_reindex_request(state, client, root_id).await?;
    let lease = scan_state
        .start(ScanKindDto::Quick, now_epoch_ms(), cancellation.clone())
        .await
        .map_err(|_| local_scan_in_progress_error_message().to_owned())?;
    let scan_id = lease.scan_id;
    tracing::info!(
        client = client.display_name(),
        scan_id,
        root_id,
        "source root reindex started"
    );
    if let Err(error) = spawn_scan_task(
        &state.scan_tasks,
        ScanTask {
            scan_id: scan_id.clone(),
            client,
            scanner: Arc::clone(&state.agent_clients.get(client.into()).local_scanner),
            operation: ScanTaskOperation::ReindexSourceRoot { request },
            cancellation,
            scan: Arc::clone(&scan_state),
            coverage_state: Arc::clone(state.coverages.get(client.into())),
            roots_state: Arc::clone(state.roots.get(client.into())),
        },
        permit,
        app_handle,
    ) {
        scan_state.finish_cancelled(&scan_id, now_epoch_ms()).await;
        return Err(error.to_owned());
    }
    Ok(scan_state.snapshot().await)
}

/// IPC 入口：前端只提交客户端与稳定根 ID。
#[tauri::command]
pub(crate) async fn reindex_source_root(
    app: AppHandle,
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    root_id: String,
) -> Result<ScanStatusDto, String> {
    reindex_source_root_for_state(&state, client, &root_id, Some(app)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 非稳定根 ID 在启动扫描状态前被拒绝，临时取得的 writer 许可必须释放。
    #[tokio::test]
    async fn rejects_invalid_id_and_releases_writer() {
        let temp = tempfile::tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        state
            .set_initialization_completed(true)
            .await
            .expect("fixture initialization completes");

        assert_eq!(
            reindex_source_root_for_state(
                &state,
                AgentClientKindDto::Codex,
                "/private/not-a-root-id",
                None,
            )
            .await,
            Err(loki_metis_core::source_root_id_invalid_message().to_owned())
        );
        let permit = state
            .local_scan
            .get(AgentClientKindDto::Codex.into())
            .try_start()
            .expect("failed request releases the global writer");
        drop(permit);
    }

    /// 关闭门禁先到达时，命令必须在打开或迁移索引之前由入口预约拒绝。
    #[tokio::test]
    async fn shutdown_rejects_reindex_before_database_preflight() {
        let temp = tempfile::tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        state.scan_tasks.shutdown().await;

        let error = reindex_source_root_for_state(
            &state,
            AgentClientKindDto::Codex,
            "root-stable-test",
            None,
        )
        .await
        .expect_err("shutdown rejects command at its owner reservation");

        assert_eq!(error, "scan-task-owner-shutting-down");
        assert!(
            !loki_metis_core::source_client_usage_index_path(
                temp.path(),
                loki_metis_core::SourceClientKind::Codex,
            )
            .exists(),
            "rejected command must not open or migrate its SQLite index"
        );
    }
}
