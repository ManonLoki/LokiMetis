//! 原生目录选择器手动添加：先核对所选路径，失败则子树签名深搜。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use loki_metis_core::{
    ManualSourceAddDecision, RootCandidate, RootCandidateEvidence, RootDiscoveryCoordinator,
    RootDiscoveryLifecycle, RootDiscoveryPlatform, RootDiscoveryProgress, RootDiscoveryScope,
    RootDiscoveryStrategy, SourceClientKind, SourceRootCandidate, decide_manual_source_add,
    path_key, source_client_app_data_dir, source_root_add, source_root_alias_from_path,
    source_root_selected_directory_unreadable_message, source_root_store_error_message, stable_id,
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use super::support::{
    SourceRootCatalogAdapter, build_source_root_mutation_response, refresh_source_roots_snapshot,
    to_source_client_kind,
};
use crate::backend::local_index::{
    CancellationToken, ClaudeDiscoveredRoot, DiscoveryProgress, FullDiscoveryOptions,
    discover_claude_full_device_with_progress, discover_full_device_with_progress,
};
use crate::commands::{
    ROOT_DISCOVERY_CANDIDATE_EVENT, candidate_to_dto, root_discovery_status_dto,
};
use crate::dto::{
    AgentClientKindDto, ManualAddOutcomeDto, ManualAddSourceRootDto, UiMessageCodeDto,
};
use crate::runtime::{AppRuntimeState, BackgroundTaskShutdown};
use crate::source_commands::acquire_source_root_write_permit;
use crate::source_commands::manual_inspection::{
    MANUAL_INSPECTION_TIMEOUT, ManualInspectionCompletion, inspect_selected_path_owned,
};

/// 只保存手动添加流程的预约状态，不用锁跨越原生目录选择或用户等待。
static MANUAL_ADD_BUSY: AtomicBool = AtomicBool::new(false);

/// 手动添加流程的短临界区预约；所有返回、错误和 panic 展开路径都会自动释放。
struct ManualAddReservation;

impl ManualAddReservation {
    /// 原子预约唯一手动添加流程，已有流程时立即失败而不等待。
    fn acquire() -> Result<Self, &'static str> {
        MANUAL_ADD_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "manual add is already in progress")
    }
}

impl Drop for ManualAddReservation {
    /// 释放预约，使目录选择取消和所有错误路径都不会永久占用入口。
    fn drop(&mut self) {
        MANUAL_ADD_BUSY.store(false, Ordering::Release);
    }
}

/// 打开原生目录选择器，核对所选路径；合格则幂等登记，否则启动子树深搜。
///
/// 初始化完成前可用；前端不得传入文件系统路径。
#[tauri::command]
pub(crate) async fn manual_add_source_root(
    app: AppHandle,
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<ManualAddSourceRootDto, String> {
    let reservation = Arc::new(ManualAddReservation::acquire().map_err(str::to_owned)?);
    if state.root_discovery.snapshot().lifecycle == RootDiscoveryLifecycle::Running {
        return Err("root discovery is already running".to_owned());
    }

    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |selection| {
        let _ = sender.send(selection);
    });
    let Some(selection) = receiver
        .await
        .map_err(|_| source_root_selected_directory_unreadable_message().to_owned())?
    else {
        return Ok(cancelled_manual_add_response());
    };
    let selected_path = selection
        .into_path()
        .map_err(|_| source_root_selected_directory_unreadable_message().to_owned())?;

    let source_client = to_source_client_kind(client);
    let coordinator = Arc::clone(&state.root_discovery);
    if !coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        manual_discovery_platform(),
        RootDiscoveryScope::ManualSubtree,
        1,
    ) {
        return Err("root discovery is already running".to_owned());
    }
    let inspect_outcome = match inspect_selected_path_owned(
        &state.background_tasks,
        Arc::clone(&coordinator),
        source_client,
        selected_path.clone(),
        Arc::clone(&reservation),
        MANUAL_INSPECTION_TIMEOUT,
    )
    .await
    {
        ManualInspectionCompletion::Finished(Ok(outcome)) => outcome,
        ManualInspectionCompletion::Finished(Err(_)) => {
            if coordinator.is_cancel_requested() {
                finish_cancelled_manual_inspection(&coordinator);
                return Ok(cancelled_manual_add_response());
            }
            coordinator.fail("manual_source_inspection_unreadable");
            return Err(source_root_selected_directory_unreadable_message().to_owned());
        }
        ManualInspectionCompletion::Cancelled => {
            finish_cancelled_manual_inspection(&coordinator);
            return Ok(cancelled_manual_add_response());
        }
        ManualInspectionCompletion::DeadlineExceeded => {
            tracing::warn!("manual source signature inspection exceeded its deadline");
            finish_cancelled_manual_inspection(&coordinator);
            return Ok(cancelled_manual_add_response());
        }
        ManualInspectionCompletion::WorkerUnavailable => {
            if coordinator.is_cancel_requested() {
                finish_cancelled_manual_inspection(&coordinator);
                return Ok(cancelled_manual_add_response());
            }
            coordinator.fail("manual_source_inspection_worker_unavailable");
            return Err(source_root_selected_directory_unreadable_message().to_owned());
        }
    };
    if coordinator.is_cancel_requested() {
        finish_cancelled_manual_inspection(&coordinator);
        return Ok(cancelled_manual_add_response());
    }

    match decide_manual_source_add(inspect_outcome.decision_input) {
        ManualSourceAddDecision::AbortCancelled => {
            finish_cancelled_manual_inspection(&coordinator);
            Ok(cancelled_manual_add_response())
        }
        ManualSourceAddDecision::Register => {
            let response = register_verified_root(
                &state,
                client,
                source_client,
                inspect_outcome.found_path,
                &coordinator,
            )
            .await?;
            if response.outcome == ManualAddOutcomeDto::Cancelled {
                finish_cancelled_manual_inspection(&coordinator);
            } else {
                finish_completed_manual_inspection(&coordinator);
            }
            Ok(response)
        }
        ManualSourceAddDecision::DeepSearch => {
            start_manual_subtree_discovery(app, &state, source_client, selected_path).await
        }
    }
}

/// 构造不携带路径或发现快照的稳定取消响应。
fn cancelled_manual_add_response() -> ManualAddSourceRootDto {
    ManualAddSourceRootDto {
        outcome: ManualAddOutcomeDto::Cancelled,
        changed: false,
        message_code: UiMessageCodeDto::SourceAddCancelled,
        discovery: None,
    }
}

/// 返回手动子树任务对应的当前宿主平台。
fn manual_discovery_platform() -> RootDiscoveryPlatform {
    if cfg!(target_os = "windows") {
        RootDiscoveryPlatform::Windows
    } else if cfg!(target_os = "macos") {
        RootDiscoveryPlatform::MacOs
    } else {
        RootDiscoveryPlatform::Other
    }
}

/// 把首次核对正常结束为完整单卷任务。
fn finish_completed_manual_inspection(coordinator: &RootDiscoveryCoordinator) {
    let mut progress = coordinator.snapshot().progress;
    progress.volumes_completed = 1;
    coordinator.update_progress(progress);
    coordinator.finish(false, false);
}

/// 把首次核对结束为取消状态，不把所选目录伪称为已完整检查。
fn finish_cancelled_manual_inspection(coordinator: &RootDiscoveryCoordinator) {
    let _ = coordinator.request_cancel();
    let mut progress = coordinator.snapshot().progress;
    progress.volumes_completed = 0;
    coordinator.update_progress(progress);
    coordinator.finish(false, false);
}

/// 把已确认的路径幂等登记为数据根，并刷新数据源快照。
async fn register_verified_root(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    source_client: SourceClientKind,
    found_path: Option<PathBuf>,
    coordinator: &RootDiscoveryCoordinator,
) -> Result<ManualAddSourceRootDto, String> {
    if coordinator.is_cancel_requested() {
        return Ok(cancelled_manual_add_response());
    }
    let path =
        found_path.ok_or_else(|| source_root_selected_directory_unreadable_message().to_owned())?;
    let candidate = SourceRootCandidate {
        root_id: stable_id(source_client.root_id_namespace(), &path_key(&path)),
        alias: source_root_alias_from_path(&path, source_client),
        path,
    };
    let _write_permit = acquire_source_root_write_permit(state, client)?;
    if coordinator.is_cancel_requested() {
        return Ok(cancelled_manual_add_response());
    }
    let app_data_dir = source_client_app_data_dir(&state.app_data_dir, source_client);
    let mut catalog = SourceRootCatalogAdapter::new(app_data_dir, source_client);
    let mutation = source_root_add(&mut catalog, &candidate)
        .await
        .map_err(|_| source_root_store_error_message().to_owned())?;
    refresh_source_roots_snapshot(state, client).await;
    let response = build_source_root_mutation_response(client, mutation);
    Ok(ManualAddSourceRootDto {
        outcome: if response.changed {
            ManualAddOutcomeDto::Registered
        } else {
            ManualAddOutcomeDto::AlreadyRegistered
        },
        changed: response.changed,
        message_code: response.message_code,
        discovery: None,
    })
}

/// 启动一次针对用户所选子树的后台深搜发现任务。
async fn start_manual_subtree_discovery(
    app: AppHandle,
    state: &AppRuntimeState,
    source_client: SourceClientKind,
    selected_path: PathBuf,
) -> Result<ManualAddSourceRootDto, String> {
    let coordinator = Arc::clone(&state.root_discovery);
    if coordinator.is_cancel_requested() {
        finish_cancelled_manual_inspection(&coordinator);
        return Ok(cancelled_manual_add_response());
    }

    let coordinator_for_task = Arc::clone(&coordinator);
    if let Err(error) =
        state
            .background_tasks
            .spawn_blocking("manual-subtree-discovery", move |shutdown| {
                if shutdown.is_cancelled() {
                    let _ = coordinator_for_task.request_cancel();
                }
                run_manual_subtree_discovery(
                    app,
                    coordinator_for_task,
                    source_client,
                    selected_path,
                    &shutdown,
                );
            })
    {
        let _ = coordinator.request_cancel();
        coordinator.finish(false, false);
        return Err(error.to_owned());
    }

    // `coordinator.start` 上面已在同步临界区把 lifecycle 置为 Running，
    // 此处快照必为 Running，无需轮询等待后台任务启动。
    Ok(ManualAddSourceRootDto {
        outcome: ManualAddOutcomeDto::DeepSearchStarted,
        changed: false,
        message_code: UiMessageCodeDto::SourceManualDeepSearchStarted,
        discovery: Some(root_discovery_status_dto(state.root_discovery.snapshot())),
    })
}

/// 阻塞岛内执行子树发现，逐步上报进度并把命中候选实时推送给前端。
fn run_manual_subtree_discovery(
    app: AppHandle,
    coordinator: Arc<RootDiscoveryCoordinator>,
    source_client: SourceClientKind,
    selected_path: PathBuf,
    shutdown: &BackgroundTaskShutdown,
) {
    let cancellation = CancellationToken::new();
    let options = FullDiscoveryOptions {
        search_roots: vec![selected_path],
        ..FullDiscoveryOptions::default()
    };
    let on_progress = |progress: DiscoveryProgress| {
        if shutdown.is_cancelled() || coordinator.is_cancel_requested() {
            let _ = coordinator.request_cancel();
            cancellation.cancel();
        }
        coordinator.update_progress(RootDiscoveryProgress {
            volumes_completed: 0,
            volumes_total: 1,
            directories_checked: progress.directories_scanned,
            file_names_checked: 0,
            candidates_found: progress.roots_discovered,
            permission_denied: 0,
            io_errors: 0,
            skipped: 0,
        });
    };
    let discovered_candidates: Vec<(PathBuf, RootCandidateEvidence)> = match source_client {
        SourceClientKind::Codex => {
            discover_full_device_with_progress(&options, &cancellation, on_progress)
                .roots
                .into_iter()
                .map(|root| (root.path, RootCandidateEvidence::CodexRollout))
                .collect()
        }
        SourceClientKind::ClaudeCode => claude_manual_candidates(
            discover_claude_full_device_with_progress(&options, &cancellation, on_progress).roots,
        ),
        SourceClientKind::GrokBuildCli => {
            crate::backend::local_index::discover_grok_full_device_with_progress(
                &options,
                &cancellation,
                on_progress,
            )
            .roots
            .into_iter()
            .map(|root| (root.path, root.evidence))
            .collect()
        }
        SourceClientKind::WorkBuddy => {
            unreachable!("手动添加入口只接受 AgentClientKindDto 的三个批准客户端")
        }
    };
    deliver_discovered_candidates(
        &coordinator,
        source_client,
        discovered_candidates,
        || shutdown.is_cancelled(),
        |candidate| {
            let Some(dto) = candidate_to_dto(candidate) else {
                return;
            };
            if manual_discovery_is_cancelled(&coordinator, &|| shutdown.is_cancelled()) {
                return;
            }
            if app.emit(ROOT_DISCOVERY_CANDIDATE_EVENT, dto).is_err() {
                tracing::warn!("manual subtree candidate event delivery failed");
            }
        },
    );
    finish_manual_subtree_discovery(&coordinator, shutdown.is_cancelled());
}

/// 保留 Claude 严格签名实际命中的主 transcript / subagent 证据。
fn claude_manual_candidates(
    roots: Vec<ClaudeDiscoveredRoot>,
) -> Vec<(PathBuf, RootCandidateEvidence)> {
    roots
        .into_iter()
        .map(|root| (root.path, root.evidence))
        .collect()
}

/// 逐项交付已发现候选；取消或关闭后不得再提交候选或调用事件回调。
fn deliver_discovered_candidates(
    coordinator: &RootDiscoveryCoordinator,
    client: SourceClientKind,
    candidates: impl IntoIterator<Item = (PathBuf, RootCandidateEvidence)>,
    is_shutdown: impl Fn() -> bool,
    mut on_candidate: impl FnMut(RootCandidate),
) {
    for (path, evidence) in candidates {
        if manual_discovery_is_cancelled(coordinator, &is_shutdown) {
            break;
        }
        submit_discovered_candidate(
            coordinator,
            client,
            evidence,
            &path,
            &is_shutdown,
            &mut on_candidate,
        );
    }
}

/// 把单个发现候选提交进协调器，并在调用事件回调前再次复核取消状态。
fn submit_discovered_candidate(
    coordinator: &RootDiscoveryCoordinator,
    client: SourceClientKind,
    evidence: RootCandidateEvidence,
    path: &Path,
    is_shutdown: &impl Fn() -> bool,
    on_candidate: &mut impl FnMut(RootCandidate),
) {
    if manual_discovery_is_cancelled(coordinator, is_shutdown) {
        return;
    }
    let candidate = RootCandidate {
        id: stable_id(client.candidate_namespace(), &path_key(path)),
        client,
        absolute_path: path.to_string_lossy().into_owned(),
        strategy: RootDiscoveryStrategy::MetadataTraversal,
        evidence,
    };
    if coordinator.submit_candidate(candidate.clone()) {
        if manual_discovery_is_cancelled(coordinator, is_shutdown) {
            coordinator.remove_candidates(std::slice::from_ref(&candidate.id));
            return;
        }
        on_candidate(candidate);
    }
}

/// 把应用关闭折叠为协调器取消，并返回当前任务是否已经不可继续交付结果。
fn manual_discovery_is_cancelled(
    coordinator: &RootDiscoveryCoordinator,
    is_shutdown: &impl Fn() -> bool,
) -> bool {
    if is_shutdown() {
        let _ = coordinator.request_cancel();
    }
    coordinator.is_cancel_requested()
}

/// 用取消感知的卷完成计数结束手动子树任务，避免取消状态伪称覆盖完整。
fn finish_manual_subtree_discovery(
    coordinator: &RootDiscoveryCoordinator,
    shutdown_requested: bool,
) {
    if shutdown_requested {
        let _ = coordinator.request_cancel();
    }
    let cancelled = coordinator.is_cancel_requested();
    let final_snapshot = coordinator.snapshot();
    coordinator.update_progress(RootDiscoveryProgress {
        volumes_completed: u64::from(!cancelled),
        volumes_total: 1,
        directories_checked: final_snapshot.progress.directories_checked,
        file_names_checked: 0,
        candidates_found: coordinator.candidates().len() as u64,
        permission_denied: final_snapshot.progress.permission_denied,
        io_errors: 0,
        skipped: final_snapshot.progress.skipped,
    });
    coordinator.finish(false, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// 原子预约必须拒绝并发流程，并在 RAII guard 销毁后立即恢复可用。
    #[test]
    fn manual_add_reservation_is_non_blocking_and_released_on_drop() {
        MANUAL_ADD_BUSY.store(false, Ordering::Release);
        let reservation = ManualAddReservation::acquire().expect("first flow reserves the picker");
        assert_eq!(
            ManualAddReservation::acquire().err(),
            Some("manual add is already in progress")
        );

        drop(reservation);

        ManualAddReservation::acquire().expect("reservation is released on every exit path");
    }

    /// 首个候选事件触发取消后，第二个结果不得再提交或调用事件回调。
    #[test]
    fn manual_subtree_stops_result_delivery_after_callback_cancellation() {
        let coordinator = RootDiscoveryCoordinator::default();
        assert!(coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::ManualSubtree,
            1,
        ));
        let delivered = Cell::new(0_usize);
        deliver_discovered_candidates(
            &coordinator,
            SourceClientKind::Codex,
            [
                (PathBuf::from("/first"), RootCandidateEvidence::CodexRollout),
                (
                    PathBuf::from("/second"),
                    RootCandidateEvidence::CodexRollout,
                ),
            ],
            || false,
            |_| {
                delivered.set(delivered.get().saturating_add(1));
                assert!(coordinator.request_cancel());
            },
        );
        finish_manual_subtree_discovery(&coordinator, false);

        assert_eq!(delivered.get(), 1);
        assert_eq!(coordinator.candidates().len(), 1);
        let status = coordinator.snapshot();
        assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Cancelled);
        assert_eq!(status.progress.volumes_completed, 0);
    }

    /// 验证子树深搜命中 subagent transcript 时保留其专属证据类型。
    #[test]
    fn manual_subtree_preserves_subagent_evidence() {
        let temp = tempfile::tempdir().expect("temporary Claude root is available");
        let root = temp.path().join("custom-claude");
        let session_id = "00000000-0000-0000-0000-000000000001";
        let transcript = root.join(format!(
            "projects/project-a/{session_id}/subagents/agent-synthetic.jsonl"
        ));
        std::fs::create_dir_all(transcript.parent().expect("transcript has a parent"))
            .expect("subagent directory is created");
        std::fs::write(
            transcript,
            format!(
                "{{\"type\":\"assistant\",\"sessionId\":\"{session_id}\",\"timestamp\":\"2026-08-08T00:00:01Z\",\"message\":{{\"id\":\"synthetic-subagent\",\"model\":\"claude-synthetic\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}}}\n"
            ),
        )
        .expect("synthetic transcript is written");
        let options = FullDiscoveryOptions {
            search_roots: vec![root],
            ..FullDiscoveryOptions::default()
        };
        let discovered =
            discover_claude_full_device_with_progress(&options, &CancellationToken::new(), |_| {});

        let candidates = claude_manual_candidates(discovered.roots);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1, RootCandidateEvidence::ClaudeSubagent);
    }
}
