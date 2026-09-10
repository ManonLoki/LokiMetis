//! 数据源发现、单候选添加与后台统计状态 commands。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loki_metis_core::{
    DiscoveryMethod, LocalIndex, RootActivationState, RootCandidate, RootCandidateEvidence,
    RootDiscoveryLifecycle, RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStatus,
    RootDiscoveryStrategy, SourceClientKind, path_key, source_client_app_data_dir,
    source_root_alias_from_path, stable_id,
};
use tauri::{Emitter, State};

use crate::backend::local_index::metadata_discovery::{
    MetadataDiscoverySummary, discover_metadata_roots_with_callback,
};
use crate::backend::local_index::validate_local_plain_directory;
use crate::dto::{
    AddRootCandidateDto, AgentClientKindDto, RootActivationStateDto, RootCandidateDto,
    RootDiscoveryPlatformDto, RootDiscoveryScopeDto, RootDiscoveryStateDto, RootDiscoveryStatusDto,
    RootDiscoveryStrategyDto, ScanStatusDto,
};
use crate::runtime::AppRuntimeState;

/// 发现命中候选时向前端推送的 Tauri 事件名。
pub(crate) const ROOT_DISCOVERY_CANDIDATE_EVENT: &str = "root-discovery-candidate";

/// 自动发现从命令接受任务起共享的总墙钟预算，包含 UserPriority 的显式根前置核对。
const ROOT_DISCOVERY_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// 监督线程复核取消、deadline 与私有进度的短轮询间隔。
const ROOT_DISCOVERY_BOUNDARY_POLL: Duration = Duration::from_millis(10);
/// 自动发现超过共同 deadline 时公开的稳定脱敏错误码。
const ROOT_DISCOVERY_DEADLINE_ERROR: &str = "metadata_discovery_deadline_exceeded";
/// 自动发现 worker panic 或提前失效时公开的稳定脱敏错误码。
const ROOT_DISCOVERY_WORKER_ERROR: &str = "metadata_discovery_worker_unavailable";

/// 把一次后台发现的私有协调器与当前共享状态绑定；关闭后所有迟到写入都被拒绝。
#[derive(Clone)]
struct RootDiscoveryRunControl {
    shared: Arc<loki_metis_core::RootDiscoveryCoordinator>,
    worker: Arc<loki_metis_core::RootDiscoveryCoordinator>,
    accepting: Arc<Mutex<bool>>,
}

impl RootDiscoveryRunControl {
    /// 建立仍可交付结果的一次性运行门。
    fn new(
        shared: Arc<loki_metis_core::RootDiscoveryCoordinator>,
        worker: Arc<loki_metis_core::RootDiscoveryCoordinator>,
    ) -> Self {
        Self {
            shared,
            worker,
            accepting: Arc::new(Mutex::new(true)),
        }
    }

    /// 同步观察取消或 deadline；返回 false 后本次任务永远不能再写共享状态。
    fn poll(&self, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        self.check_boundary(&mut accepting, Instant::now(), deadline)
    }

    /// 在线性化门内提交候选与进度，避免 deadline 与“最后一次写入”竞态。
    fn deliver_candidate(&self, candidate: RootCandidate, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        if !self.check_boundary(&mut accepting, Instant::now(), deadline) {
            return false;
        }
        self.shared.submit_candidate(candidate)
    }

    /// 在正常 worker 终态处关闭门并把私有状态一次性归约到共享协调器。
    fn complete(&self, result: Result<MetadataDiscoverySummary, ()>, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        if !self.check_boundary(&mut accepting, Instant::now(), deadline) {
            return false;
        }
        *accepting = false;
        let worker_status = self.worker.snapshot();
        match result {
            Err(()) => self.shared.fail(ROOT_DISCOVERY_WORKER_ERROR),
            Ok(_) if worker_status.lifecycle == RootDiscoveryLifecycle::Failed => self.shared.fail(
                worker_status
                    .error_code
                    .as_deref()
                    .unwrap_or(ROOT_DISCOVERY_WORKER_ERROR),
            ),
            Ok(summary) => {
                if worker_status.lifecycle == RootDiscoveryLifecycle::Cancelled {
                    let _ = self.shared.request_cancel();
                }
                self.shared
                    .finish(summary.system_index_available, summary.fallback_performed);
            }
        }
        true
    }

    /// worker 未能启动或 panic 时关闭交付门，不让后续调度保留 Running。
    fn fail_worker(&self, deadline: Instant) {
        let mut accepting = self.lock_accepting();
        if self.check_boundary(&mut accepting, Instant::now(), deadline) {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.fail(ROOT_DISCOVERY_WORKER_ERROR);
        }
    }

    /// owner 关闭时同步终结公开状态，并持续向可能已经启动的私有 worker 发出取消。
    fn cancel(&self) {
        let mut accepting = self.lock_accepting();
        let _ = self.worker.request_cancel();
        if !*accepting {
            return;
        }
        *accepting = false;
        let _ = self.shared.request_cancel();
        self.shared.finish(false, false);
    }

    /// 在同一锁内决定本次运行能否继续，确保关闭后没有迟到状态写入窗口。
    fn check_boundary(&self, accepting: &mut bool, now: Instant, deadline: Instant) -> bool {
        if !*accepting {
            let _ = self.worker.request_cancel();
            return false;
        }
        if self.shared.is_cancel_requested() {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.finish(false, false);
            return false;
        }
        if now >= deadline {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.fail(ROOT_DISCOVERY_DEADLINE_ERROR);
            return false;
        }
        self.sync_worker_progress();
        true
    }

    /// 只在交付门内把私有 worker 的无路径进度同步到共享协调器。
    fn sync_worker_progress(&self) {
        let status = self.worker.snapshot();
        if status.fallback_performed {
            self.shared.begin_fallback(status.system_index_available);
        }
        self.shared.update_progress(status.progress);
    }

    /// 取得运行门；测试 panic 污染后仍保持超时关闭能力。
    fn lock_accepting(&self) -> std::sync::MutexGuard<'_, bool> {
        self.accepting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 独立于 blocking 线程池调度监督绝对 deadline，队列饱和时仍能收敛公开状态。
async fn supervise_root_discovery_boundaries<ShutdownRequested>(
    control: RootDiscoveryRunControl,
    deadline: Instant,
    shutdown_requested: ShutdownRequested,
) where
    ShutdownRequested: Fn() -> bool,
{
    loop {
        if shutdown_requested() {
            control.cancel();
            return;
        }
        if !control.poll(deadline) {
            return;
        }
        tokio::time::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(ROOT_DISCOVERY_BOUNDARY_POLL),
        )
        .await;
    }
}

/// 在现有 BackgroundTaskOwner 持有的 blocking 岛内执行私有发现。
fn run_owned_root_discovery(
    app: tauri::AppHandle,
    control: RootDiscoveryRunControl,
    discovery_scope: RootDiscoveryScope,
    deadline: Instant,
) {
    if !control.poll(deadline) {
        return;
    }
    let callback_control = control.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        discover_metadata_roots_with_callback(&control.worker, discovery_scope, move |candidate| {
            let Some(dto) = candidate_to_dto(candidate.clone()) else {
                return;
            };
            if callback_control.deliver_candidate(candidate, deadline)
                && app.emit(ROOT_DISCOVERY_CANDIDATE_EVENT, dto).is_err()
            {
                tracing::warn!("root discovery candidate event delivery failed");
            }
        })
    }))
    .map_err(|_| ());
    if let Ok(summary) = &result {
        tracing::info!(
            system_index_available = summary.system_index_available,
            fallback_performed = summary.fallback_performed,
            "root discovery finished"
        );
    }
    let _ = control.complete(result, deadline);
}

/// 启动用户目录或全本地卷数据源发现；候选仅保存在当前进程内存。
#[tauri::command]
pub(crate) async fn start_root_discovery(
    app: tauri::AppHandle,
    state: State<'_, AppRuntimeState>,
    scope: RootDiscoveryScopeDto,
) -> Result<RootDiscoveryStatusDto, String> {
    let task_started = Instant::now();
    let deadline = task_started + ROOT_DISCOVERY_TOTAL_TIMEOUT;
    let coordinator = Arc::clone(&state.root_discovery);
    if coordinator.snapshot().lifecycle == RootDiscoveryLifecycle::Running {
        return Err("root discovery is already running".to_owned());
    }
    let strategy = if cfg!(target_os = "windows") {
        RootDiscoveryStrategy::WindowsSearch
    } else if cfg!(target_os = "macos") {
        RootDiscoveryStrategy::MacOsSpotlight
    } else {
        RootDiscoveryStrategy::MetadataTraversal
    };
    let platform = current_platform();
    let discovery_scope = match scope {
        RootDiscoveryScopeDto::UserPriority => RootDiscoveryScope::UserPriority,
        RootDiscoveryScopeDto::FullLocalVolumes => RootDiscoveryScope::FullLocalVolumes,
        RootDiscoveryScopeDto::ManualSubtree => {
            return Err("manual subtree discovery requires manual_add_source_root".to_owned());
        }
    };
    if !coordinator.start(strategy, platform, discovery_scope, 0) {
        return Err("root discovery is already running".to_owned());
    }
    let worker_coordinator = Arc::new(loki_metis_core::RootDiscoveryCoordinator::default());
    let control = RootDiscoveryRunControl::new(Arc::clone(&coordinator), worker_coordinator);
    let monitor_control = control.clone();
    if let Err(error) =
        state
            .background_tasks
            .spawn("root-discovery-deadline", move |shutdown| async move {
                supervise_root_discovery_boundaries(monitor_control, deadline, move || {
                    shutdown.is_cancelled()
                })
                .await;
            })
    {
        control.fail_worker(deadline);
        return Err(error.to_owned());
    }
    let cancel_control = control.clone();
    let worker_control = control.clone();
    if let Err(error) = state.background_tasks.spawn_blocking_cancelable(
        "root-discovery",
        move || cancel_control.cancel(),
        move |_shutdown| {
            run_owned_root_discovery(app, worker_control, discovery_scope, deadline);
        },
    ) {
        control.fail_worker(deadline);
        return Err(error.to_owned());
    }
    for _ in 0..20 {
        let status = state.root_discovery.snapshot();
        if status.lifecycle == RootDiscoveryLifecycle::Running {
            return Ok(to_status_dto(status));
        }
        tokio::task::yield_now().await;
    }
    Ok(to_status_dto(state.root_discovery.snapshot()))
}

/// 返回当前数据源发现状态。
#[tauri::command]
pub(crate) async fn get_root_discovery_status(
    state: State<'_, AppRuntimeState>,
) -> Result<RootDiscoveryStatusDto, String> {
    Ok(to_status_dto(state.root_discovery.snapshot()))
}

/// 返回当前任务候选；这是唯一允许完整路径离开 backend 的接口。
#[tauri::command]
pub(crate) async fn list_root_candidates(
    state: State<'_, AppRuntimeState>,
) -> Result<Vec<RootCandidateDto>, String> {
    Ok(state
        .root_discovery
        .candidates()
        .into_iter()
        .filter_map(candidate_to_dto)
        .collect())
}

/// 把内存中的发现候选映射为可发往前端的 DTO；WorkBuddy 不是登记客户端，直接跳过。
pub(crate) fn candidate_to_dto(candidate: RootCandidate) -> Option<RootCandidateDto> {
    Some(RootCandidateDto {
        id: candidate.id,
        client: client_to_dto(candidate.client)?,
        absolute_path: candidate.absolute_path,
        strategy: to_strategy_dto(candidate.strategy),
        evidence: match candidate.evidence {
            RootCandidateEvidence::CodexRollout => "codexRollout",
            RootCandidateEvidence::ClaudeTranscript => "claudeTranscript",
            RootCandidateEvidence::ClaudeSubagent => "claudeSubagent",
            RootCandidateEvidence::GrokSessionUpdates => "grokSessionUpdates",
            RootCandidateEvidence::WorkbuddyProjectJsonl => return None,
        }
        .to_owned(),
    })
}

/// 把单个候选添加到对应数据源并排入后台周期统计。
#[tauri::command]
pub(crate) async fn add_root_candidate(
    state: State<'_, AppRuntimeState>,
    candidate_id: String,
) -> Result<AddRootCandidateDto, String> {
    let candidate = state
        .root_discovery
        .select_candidates(std::slice::from_ref(&candidate_id))
        .map_err(|_| "root candidate selection is invalid".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "root candidate selection is invalid".to_owned())?;
    if candidate.client == SourceClientKind::WorkBuddy {
        return Err("WorkBuddy 默认数据目录不可通过发现候选登记。".to_owned());
    }
    let client = client_to_dto(candidate.client)
        .ok_or_else(|| "WorkBuddy 默认数据目录不可通过发现候选登记。".to_owned())?;
    let path = PathBuf::from(&candidate.absolute_path);
    let checked_path = path.clone();
    tauri::async_runtime::spawn_blocking(move || validate_local_plain_directory(&checked_path))
        .await
        .map_err(|_| "candidate validation task failed".to_owned())?
        .map_err(|error| error.to_string())?;
    let _write_permit = crate::source_commands::acquire_source_root_write_permit(&state, client)?;
    let app_data = source_client_app_data_dir(&state.app_data_dir, candidate.client);
    let mut index = LocalIndex::open_in_app_data(&app_data, candidate.client.parser_version())
        .await
        .map_err(|error| error.to_string())?;
    let root_id = stable_id(candidate.client.root_id_namespace(), &path_key(&path));
    let alias = source_root_alias_from_path(&path, candidate.client);
    let added = index
        .register_confirmed_root_fields(&root_id, &path, &alias, DiscoveryMethod::MetadataDiscovery)
        .await
        .map_err(|error| error.to_string())?;
    let background_state = index
        .queue_confirmed_root_for_background_scan(&root_id)
        .await
        .map_err(|error| error.to_string())?;
    tracing::info!(added, client = ?candidate.client, "root candidate added");
    state
        .root_discovery
        .remove_candidates(std::slice::from_ref(&candidate_id));
    Ok(AddRootCandidateDto {
        client,
        root_id,
        added,
        background_state: to_activation_state_dto(background_state),
    })
}

/// 请求当前数据源发现任务取消。
#[tauri::command]
pub(crate) async fn cancel_root_discovery(
    state: State<'_, AppRuntimeState>,
) -> Result<RootDiscoveryStatusDto, String> {
    if !state.root_discovery.request_cancel() {
        return Err("root discovery is not running".to_owned());
    }
    Ok(to_status_dto(state.root_discovery.snapshot()))
}

/// 返回对应客户端的后台本机 Token 统计状态。
#[tauri::command]
pub(crate) async fn get_local_scan_status(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<ScanStatusDto, String> {
    Ok(state.scans.get(client.into()).snapshot())
}

/// 把 core 的根激活状态映射为 DTO 激活状态。
fn to_activation_state_dto(state: RootActivationState) -> RootActivationStateDto {
    match state {
        RootActivationState::ConfirmedUnindexed => RootActivationStateDto::ConfirmedUnindexed,
        RootActivationState::Indexing => RootActivationStateDto::Indexing,
        RootActivationState::Ready => RootActivationStateDto::Ready,
        RootActivationState::ValidationFailed => RootActivationStateDto::ValidationFailed,
    }
}

/// 把 core 的语义客户端类型映射为 IPC 边界的客户端枚举；WorkBuddy 不是登记客户端。
fn client_to_dto(client: SourceClientKind) -> Option<AgentClientKindDto> {
    match client {
        SourceClientKind::Codex => Some(AgentClientKindDto::Codex),
        SourceClientKind::ClaudeCode => Some(AgentClientKindDto::ClaudeCode),
        SourceClientKind::GrokBuildCli => Some(AgentClientKindDto::GrokBuildCli),
        SourceClientKind::WorkBuddy => None,
    }
}

/// 把 core 状态映射为公开 DTO。
pub(crate) fn to_status_dto(status: RootDiscoveryStatus) -> RootDiscoveryStatusDto {
    let progress = status.progress;
    RootDiscoveryStatusDto {
        state: match status.lifecycle {
            RootDiscoveryLifecycle::Idle => RootDiscoveryStateDto::Idle,
            RootDiscoveryLifecycle::Running => RootDiscoveryStateDto::Running,
            RootDiscoveryLifecycle::Complete => RootDiscoveryStateDto::Complete,
            RootDiscoveryLifecycle::Partial => RootDiscoveryStateDto::Partial,
            RootDiscoveryLifecycle::Cancelled => RootDiscoveryStateDto::Cancelled,
            RootDiscoveryLifecycle::Failed => RootDiscoveryStateDto::Failed,
        },
        strategy: to_strategy_dto(status.strategy),
        platform: to_platform_dto(if status.platform == RootDiscoveryPlatform::Other {
            current_platform()
        } else {
            status.platform
        }),
        scope: match status.scope {
            RootDiscoveryScope::UserPriority => RootDiscoveryScopeDto::UserPriority,
            RootDiscoveryScope::FullLocalVolumes => RootDiscoveryScopeDto::FullLocalVolumes,
            RootDiscoveryScope::ManualSubtree => RootDiscoveryScopeDto::ManualSubtree,
        },
        system_index_available: status.system_index_available,
        fallback_performed: status.fallback_performed,
        volumes_completed: progress.volumes_completed,
        volumes_total: progress.volumes_total,
        directories_checked: progress.directories_checked,
        file_names_checked: progress.file_names_checked,
        candidates_found: progress.candidates_found,
        permission_denied: progress.permission_denied,
        io_errors: progress.io_errors,
        skipped: progress.skipped,
        error_code: status.error_code,
    }
}

/// 探测当前编译目标所在平台，供未命中固定策略时回退展示。
const fn current_platform() -> RootDiscoveryPlatform {
    if cfg!(target_os = "windows") {
        RootDiscoveryPlatform::Windows
    } else if cfg!(target_os = "macos") {
        RootDiscoveryPlatform::MacOs
    } else {
        RootDiscoveryPlatform::Other
    }
}

/// 把 core 的运行平台枚举映射为 DTO 平台枚举。
const fn to_platform_dto(platform: RootDiscoveryPlatform) -> RootDiscoveryPlatformDto {
    match platform {
        RootDiscoveryPlatform::Windows => RootDiscoveryPlatformDto::Windows,
        RootDiscoveryPlatform::MacOs => RootDiscoveryPlatformDto::MacOs,
        RootDiscoveryPlatform::Other => RootDiscoveryPlatformDto::Other,
    }
}

/// 映射平台发现策略。
fn to_strategy_dto(strategy: RootDiscoveryStrategy) -> RootDiscoveryStrategyDto {
    match strategy {
        RootDiscoveryStrategy::WindowsSearch => RootDiscoveryStrategyDto::WindowsSearch,
        RootDiscoveryStrategy::MacOsSpotlight => RootDiscoveryStrategyDto::MacOsSpotlight,
        RootDiscoveryStrategy::MetadataTraversal => RootDiscoveryStrategyDto::MetadataTraversal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造共享/私有协调器都已启动的一次 UserPriority 运行。
    fn running_control() -> RootDiscoveryRunControl {
        let shared = Arc::new(loki_metis_core::RootDiscoveryCoordinator::default());
        let worker = Arc::new(loki_metis_core::RootDiscoveryCoordinator::default());
        assert!(shared.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        assert!(worker.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        RootDiscoveryRunControl::new(shared, worker)
    }

    /// blocking worker 尚未获得调度时，独立监督器仍必须让总 deadline 生效。
    #[tokio::test]
    async fn queued_worker_deadline_including_user_priority_prelude_rejects_late_writes() {
        let control = running_control();
        let deadline = Instant::now();

        supervise_root_discovery_boundaries(control.clone(), deadline, || false).await;
        assert!(control.worker.is_cancel_requested());
        let timed_out = control.shared.snapshot();
        assert_eq!(timed_out.lifecycle, RootDiscoveryLifecycle::Failed);
        assert_eq!(
            timed_out.error_code.as_deref(),
            Some(ROOT_DISCOVERY_DEADLINE_ERROR)
        );

        let candidate = RootCandidate {
            id: "late-candidate".to_owned(),
            client: SourceClientKind::Codex,
            absolute_path: "/late".to_owned(),
            strategy: RootDiscoveryStrategy::MetadataTraversal,
            evidence: RootCandidateEvidence::CodexRollout,
        };
        assert!(!control.deliver_candidate(candidate, deadline));
        control
            .worker
            .update_progress(loki_metis_core::RootDiscoveryProgress {
                directories_checked: 99,
                ..loki_metis_core::RootDiscoveryProgress::default()
            });
        assert!(!control.complete(
            Ok(MetadataDiscoverySummary {
                system_index_available: true,
                fallback_performed: true,
            }),
            deadline,
        ));

        assert!(control.shared.candidates().is_empty());
        let after_late_completion = control.shared.snapshot();
        assert_eq!(after_late_completion, timed_out);
    }

    /// gate 等待跨过 deadline 时必须在获锁后重新采样，迟到进度不得提交。
    #[test]
    fn root_discovery_rechecks_deadline_after_waiting_for_delivery_gate() {
        let control = running_control();
        let polling_control = control.clone();
        let held_gate = control.lock_accepting();
        let deadline = Instant::now() + Duration::from_millis(1);
        let polling = std::thread::spawn(move || polling_control.poll(deadline));

        while Instant::now() < deadline {
            std::thread::yield_now();
        }
        drop(held_gate);

        assert!(
            !polling
                .join()
                .expect("deadline poll reaches terminal state")
        );
        let status = control.shared.snapshot();
        assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Failed);
        assert_eq!(
            status.error_code.as_deref(),
            Some(ROOT_DISCOVERY_DEADLINE_ERROR)
        );
    }

    /// 用户取消与 owner 关闭使用同一单向门，迟到 worker 不能把 Cancelled 改回成功。
    #[test]
    fn cancellation_closes_delivery_gate_before_worker_completion() {
        let control = running_control();
        assert!(control.shared.request_cancel());
        let deadline = Instant::now() + Duration::from_secs(1);

        assert!(!control.poll(deadline));
        assert_eq!(
            control.shared.snapshot().lifecycle,
            RootDiscoveryLifecycle::Cancelled
        );
        assert!(!control.complete(Ok(MetadataDiscoverySummary::default()), deadline,));
        assert_eq!(
            control.shared.snapshot().lifecycle,
            RootDiscoveryLifecycle::Cancelled
        );
    }

    /// 正常完成仍应提交候选并保留私有 worker 的真实进度与阶段。
    #[test]
    fn successful_private_worker_state_is_committed_once() {
        let control = running_control();
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        control
            .worker
            .update_progress(loki_metis_core::RootDiscoveryProgress {
                volumes_completed: 1,
                volumes_total: 1,
                directories_checked: 2,
                ..loki_metis_core::RootDiscoveryProgress::default()
            });
        let candidate = RootCandidate {
            id: "candidate".to_owned(),
            client: SourceClientKind::Codex,
            absolute_path: "/candidate".to_owned(),
            strategy: RootDiscoveryStrategy::MetadataTraversal,
            evidence: RootCandidateEvidence::CodexRollout,
        };

        assert!(control.deliver_candidate(candidate, deadline));
        control.worker.finish(false, true);
        assert!(control.complete(
            Ok(MetadataDiscoverySummary {
                system_index_available: false,
                fallback_performed: true,
            }),
            deadline,
        ));

        let status = control.shared.snapshot();
        assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Complete);
        assert_eq!(status.progress.directories_checked, 2);
        assert_eq!(status.progress.candidates_found, 1);
    }
}
