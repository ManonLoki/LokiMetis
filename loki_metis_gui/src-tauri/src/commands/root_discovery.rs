//! 数据源发现、单候选添加与后台统计状态 commands。

use std::path::PathBuf;
use std::sync::Arc;

use loki_metis_core::{
    DiscoveryMethod, LocalIndex, RootActivationState, RootCandidate, RootCandidateEvidence,
    RootDiscoveryLifecycle, RootDiscoveryPlatform, RootDiscoveryScope, RootDiscoveryStatus,
    RootDiscoveryStrategy, SourceClientKind, path_key, source_client_app_data_dir,
    source_root_alias_from_path, stable_id,
};
use tauri::{Emitter, State};

use crate::backend::local_index::metadata_discovery::discover_metadata_roots_with_callback;
use crate::backend::local_index::validate_local_plain_directory;
use crate::dto::{
    AddRootCandidateDto, AgentClientKindDto, RootActivationStateDto, RootCandidateDto,
    RootDiscoveryPlatformDto, RootDiscoveryScopeDto, RootDiscoveryStateDto, RootDiscoveryStatusDto,
    RootDiscoveryStrategyDto, ScanStatusDto,
};
use crate::runtime::AppRuntimeState;

/// 发现命中候选时向前端推送的 Tauri 事件名。
pub(crate) const ROOT_DISCOVERY_CANDIDATE_EVENT: &str = "root-discovery-candidate";

/// 启动用户目录或全本地卷数据源发现；候选仅保存在当前进程内存。
#[tauri::command]
pub(crate) async fn start_root_discovery(
    app: tauri::AppHandle,
    state: State<'_, AppRuntimeState>,
    scope: RootDiscoveryScopeDto,
) -> Result<RootDiscoveryStatusDto, String> {
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
    tauri::async_runtime::spawn_blocking(move || {
        let summary =
            discover_metadata_roots_with_callback(&coordinator, discovery_scope, |candidate| {
                let Some(dto) = candidate_to_dto(candidate) else {
                    return;
                };
                if app.emit(ROOT_DISCOVERY_CANDIDATE_EVENT, dto).is_err() {
                    tracing::warn!("root discovery candidate event delivery failed");
                }
            });
        tracing::info!(
            system_index_available = summary.system_index_available,
            fallback_performed = summary.fallback_performed,
            "root discovery finished"
        );
    });
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
    let path = PathBuf::from(&candidate.absolute_path);
    let checked_path = path.clone();
    tauri::async_runtime::spawn_blocking(move || validate_local_plain_directory(&checked_path))
        .await
        .map_err(|_| "candidate validation task failed".to_owned())?
        .map_err(|error| error.to_string())?;
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
        client: client_to_dto(candidate.client)
            .ok_or_else(|| "WorkBuddy 默认数据目录不可通过发现候选登记。".to_owned())?,
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
