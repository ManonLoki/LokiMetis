//! 原生目录选择器手动添加：先核对所选路径，失败则子树签名深搜。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use loki_metis_core::{
    DiscoveryMethod, ManualSourceAddDecision, ManualSourceInspectOutcome, RootCandidate,
    RootCandidateEvidence, RootDiscoveryCoordinator, RootDiscoveryLifecycle, RootDiscoveryPlatform,
    RootDiscoveryProgress, RootDiscoveryScope, RootDiscoveryStrategy, SourceClientKind,
    SourceRootCandidate, decide_manual_source_add, path_key, source_client_app_data_dir,
    source_root_add, source_root_alias_from_path,
    source_root_selected_directory_unreadable_message, source_root_store_error_message, stable_id,
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use super::support::{
    SourceRootCatalogAdapter, build_source_root_mutation_response, ensure_local_scan_not_running,
    refresh_source_roots_snapshot, to_source_client_kind,
};
use crate::backend::local_index::{
    CancellationToken, ClaudeDiscoveredRoot, ClaudeRootInspection, ClaudeSignatureBudget,
    DiscoveryProgress, FullDiscoveryOptions, RootInspection, SignatureProbeContext,
    discover_claude_full_device_with_progress, discover_full_device_with_progress,
    inspect_claude_root, inspect_root, validate_local_plain_directory,
};
use crate::commands::{
    ROOT_DISCOVERY_CANDIDATE_EVENT, candidate_to_dto, root_discovery_status_dto,
};
use crate::dto::{
    AgentClientKindDto, ManualAddOutcomeDto, ManualAddSourceRootDto, UiMessageCodeDto,
};
use crate::runtime::AppRuntimeState;

/// 确保同一时刻只有一次手动添加流程在运行，避免并发目录选择互相干扰。
static MANUAL_ADD_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 打开原生目录选择器，核对所选路径；合格则幂等登记，否则启动子树深搜。
///
/// 初始化完成前可用；前端不得传入文件系统路径。
#[tauri::command]
pub(crate) async fn manual_add_source_root(
    app: AppHandle,
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<ManualAddSourceRootDto, String> {
    let _gate = MANUAL_ADD_GATE
        .try_lock()
        .map_err(|_| "manual add is already in progress".to_owned())?;
    ensure_local_scan_not_running(&state, client)?;
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
        return Ok(ManualAddSourceRootDto {
            outcome: ManualAddOutcomeDto::Cancelled,
            changed: false,
            message_code: UiMessageCodeDto::SourceAddCancelled,
            discovery: None,
        });
    };
    let selected_path = selection
        .into_path()
        .map_err(|_| source_root_selected_directory_unreadable_message().to_owned())?;

    let source_client = to_source_client_kind(client);
    let checked_path = selected_path.clone();
    let inspect_outcome = tauri::async_runtime::spawn_blocking(move || {
        validate_local_plain_directory(&checked_path)
            .map(|()| inspect_selected_path(source_client, &checked_path))
    })
    .await
    .map_err(|_| source_root_selected_directory_unreadable_message().to_owned())?
    .map_err(|_| source_root_selected_directory_unreadable_message().to_owned())?;

    match decide_manual_source_add(inspect_outcome.decision_input) {
        ManualSourceAddDecision::AbortCancelled => Ok(ManualAddSourceRootDto {
            outcome: ManualAddOutcomeDto::Cancelled,
            changed: false,
            message_code: UiMessageCodeDto::SourceAddCancelled,
            discovery: None,
        }),
        ManualSourceAddDecision::Register => {
            register_verified_root(&state, client, source_client, inspect_outcome.found_path).await
        }
        ManualSourceAddDecision::DeepSearch => {
            start_manual_subtree_discovery(app, &state, source_client, selected_path).await
        }
    }
}

/// 单次结构探测的结果：核对结论与（找到时）实际根路径。
struct InspectBundle {
    /// core 归约核对结论所需的输入。
    decision_input: ManualSourceInspectOutcome,
    /// 探测命中时的实际根路径。
    found_path: Option<PathBuf>,
}

impl InspectBundle {
    /// 构造探测确认命中的结果。
    fn found(path: PathBuf) -> Self {
        Self {
            decision_input: ManualSourceInspectOutcome::Found,
            found_path: Some(path),
        }
    }

    /// 构造未能确认为合格根的结果。
    fn unverified(decision_input: ManualSourceInspectOutcome) -> Self {
        Self {
            decision_input,
            found_path: None,
        }
    }
}

/// 按客户端类型选择结构签名探测策略，核对用户所选路径；WorkBuddy 不经手动添加
/// 入口（前端只能选 `AgentClientKindDto` 三个批准客户端），不可达。
fn inspect_selected_path(client: SourceClientKind, path: &Path) -> InspectBundle {
    let cancellation = CancellationToken::new();
    let options = FullDiscoveryOptions::default();
    match client {
        SourceClientKind::Codex => {
            let alias = source_root_alias_from_path(path, client);
            let mut probe = SignatureProbeContext::new(&options, &cancellation);
            classify_codex_inspection(inspect_root(
                path,
                alias,
                DiscoveryMethod::Registered,
                None,
                Some(&mut probe),
            ))
        }
        SourceClientKind::ClaudeCode => {
            let mut budget = ClaudeSignatureBudget::new(&options, &cancellation);
            classify_claude_inspection(path, inspect_claude_root(path, &mut budget))
        }
        SourceClientKind::GrokBuildCli => {
            let mut budget =
                crate::backend::local_index::GrokSignatureBudget::new(&options, &cancellation);
            classify_grok_inspection(
                path,
                crate::backend::local_index::inspect_grok_root(path, &mut budget),
            )
        }
        SourceClientKind::WorkBuddy => {
            unreachable!("手动添加入口只接受 AgentClientKindDto 的三个批准客户端")
        }
    }
}

/// 把 Codex 结构探测结论映射为统一的核对结果。
fn classify_codex_inspection(inspection: RootInspection) -> InspectBundle {
    match inspection {
        RootInspection::Found(root) => InspectBundle::found(root.path),
        RootInspection::NotRoot => InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot),
        RootInspection::RejectedSignature => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        RootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        RootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        RootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

/// 把 Grok 结构探测结论映射为统一的核对结果。
fn classify_grok_inspection(
    path: &Path,
    inspection: crate::backend::local_index::GrokRootInspection,
) -> InspectBundle {
    use crate::backend::local_index::GrokRootInspection;
    match inspection {
        GrokRootInspection::Found(_) => InspectBundle::found(path.to_path_buf()),
        GrokRootInspection::NotRoot => {
            InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot)
        }
        GrokRootInspection::Rejected => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        GrokRootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        GrokRootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        GrokRootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

/// 把 Claude 结构探测结论映射为统一的核对结果。
fn classify_claude_inspection(path: &Path, inspection: ClaudeRootInspection) -> InspectBundle {
    match inspection {
        ClaudeRootInspection::Found(_) => InspectBundle::found(path.to_path_buf()),
        ClaudeRootInspection::NotRoot => {
            InspectBundle::unverified(ManualSourceInspectOutcome::NotRoot)
        }
        ClaudeRootInspection::Rejected => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Rejected)
        }
        ClaudeRootInspection::Indeterminate => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Indeterminate)
        }
        ClaudeRootInspection::Cancelled => {
            InspectBundle::unverified(ManualSourceInspectOutcome::Cancelled)
        }
        ClaudeRootInspection::BudgetExhausted => {
            InspectBundle::unverified(ManualSourceInspectOutcome::BudgetExhausted)
        }
    }
}

/// 把已确认的路径幂等登记为数据根，并刷新数据源快照。
async fn register_verified_root(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    source_client: SourceClientKind,
    found_path: Option<PathBuf>,
) -> Result<ManualAddSourceRootDto, String> {
    let path =
        found_path.ok_or_else(|| source_root_selected_directory_unreadable_message().to_owned())?;
    let prefix = if source_client == SourceClientKind::Codex {
        "root"
    } else {
        "claude-root"
    };
    let candidate = SourceRootCandidate {
        root_id: stable_id(prefix, &path_key(&path)),
        alias: source_root_alias_from_path(&path, source_client),
        path,
    };
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
    let platform = if cfg!(target_os = "windows") {
        RootDiscoveryPlatform::Windows
    } else if cfg!(target_os = "macos") {
        RootDiscoveryPlatform::MacOs
    } else {
        RootDiscoveryPlatform::Other
    };
    if !coordinator.start(
        RootDiscoveryStrategy::MetadataTraversal,
        platform,
        RootDiscoveryScope::ManualSubtree,
        1,
    ) {
        return Err("root discovery is already running".to_owned());
    }

    let coordinator_for_task = Arc::clone(&coordinator);
    tauri::async_runtime::spawn_blocking(move || {
        run_manual_subtree_discovery(app, coordinator_for_task, source_client, selected_path);
    });

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
) {
    let cancellation = CancellationToken::new();
    let options = FullDiscoveryOptions {
        search_roots: vec![selected_path],
        ..FullDiscoveryOptions::default()
    };
    let on_progress = |progress: DiscoveryProgress| {
        if coordinator.is_cancel_requested() {
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
    for (path, evidence) in &discovered_candidates {
        submit_discovered_candidate(&app, &coordinator, source_client, *evidence, path);
    }

    let final_snapshot = coordinator.snapshot();
    coordinator.update_progress(RootDiscoveryProgress {
        volumes_completed: 1,
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

/// 保留 Claude 严格签名实际命中的主 transcript / subagent 证据。
fn claude_manual_candidates(
    roots: Vec<ClaudeDiscoveredRoot>,
) -> Vec<(PathBuf, RootCandidateEvidence)> {
    roots
        .into_iter()
        .map(|root| (root.path, root.evidence))
        .collect()
}

/// 把单个发现候选提交进协调器并向前端广播事件。
fn submit_discovered_candidate(
    app: &AppHandle,
    coordinator: &RootDiscoveryCoordinator,
    client: SourceClientKind,
    evidence: RootCandidateEvidence,
    path: &Path,
) {
    let candidate = RootCandidate {
        id: stable_id(client.candidate_namespace(), &path_key(path)),
        client,
        absolute_path: path.to_string_lossy().into_owned(),
        strategy: RootDiscoveryStrategy::MetadataTraversal,
        evidence,
    };
    if coordinator.submit_candidate(candidate.clone())
        && let Some(dto) = candidate_to_dto(candidate)
        && app.emit(ROOT_DISCOVERY_CANDIDATE_EVENT, dto).is_err()
    {
        tracing::warn!("manual subtree candidate event delivery failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
