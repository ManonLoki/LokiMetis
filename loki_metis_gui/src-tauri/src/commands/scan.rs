//! 扫描查询、启动、周期调度与清理命令。

use std::sync::Arc;

use loki_metis_core::{
    DiscoveryBatchIndexDecision, DiscoveryBatchKind, RootDiscoveryLifecycle, ScanStartOrigin,
    clear_local_index_success_message, clear_local_index_while_scanning_message,
    discovery_batch_index_decision, empty_coverage, ensure_periodic_quick_scan_allowed,
    immediate_reindex_required, local_scan_in_progress_error_message,
    local_scan_writer_busy_message,
};
#[cfg(test)]
use loki_metis_core::{local_scan_client_failure_message, local_scan_writer_busy_failure_detail};
use tauri::State;

#[cfg(test)]
use crate::commands::scan_orchestration::spawn_scan_task;
use crate::commands::scan_orchestration::{ScanTask, ScanTaskOperation, execute_scan_task};
use crate::dto::{
    AgentClientKindDto, ClearIndexResultDto, LocalIndexRefreshTriggerDto, ScanKindDto,
    ScanStateDto, ScanStatusDto, UiMessageCodeDto,
};
use crate::runtime::{AppRuntimeState, now_epoch_ms};

use super::access::ensure_scan_start_access_by_policy;
use super::ensure_business_access;

/// 在首次设置标记成功时安排唯一一次自动快速扫描；重复调用保持无副作用。
#[cfg(test)]
pub(crate) async fn start_initial_scan_if_needed(
    state: &AppRuntimeState,
) -> Result<Option<ScanStatusDto>, String> {
    if !state.claim_initial_scan().await? {
        return Ok(None);
    }
    let existing_scan = state
        .scans
        .get(AgentClientKindDto::Codex.into())
        .snapshot()
        .await;
    if existing_scan.started_at_epoch_ms.is_some() {
        return Ok(Some(existing_scan));
    }
    if !state
        .enabled_agents()
        .await
        .contains(AgentClientKindDto::Codex.into())
    {
        return Ok(None);
    }
    start_scan_for_client(
        state,
        AgentClientKindDto::Codex,
        ScanKindDto::Quick,
        ScanStartOrigin::InitialAutomatic,
    )
    .await
    .map(Some)
}

/// 复用同一单 writer 门禁启动扫描，并按触发来源应用初始化访问策略。
/// 生产路径的周期与批量刷新都已改为在同一许可内串行 await，本入口只剩测试夹具使用。
#[cfg(test)]
pub(crate) async fn start_scan_for_client(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    kind: ScanKindDto,
    origin: ScanStartOrigin,
) -> Result<ScanStatusDto, String> {
    ensure_scan_start_access_by_policy(state, origin, kind).await?;
    let _account_context_guard = if client == AgentClientKindDto::Codex {
        Some(state.lock_codex_account_context().await)
    } else {
        None
    };
    let scan_state = Arc::clone(state.scans.get(client.into()));
    let lease = scan_state.start(kind, now_epoch_ms()).await.map_err(|_| {
        tracing::warn!(
            client = client.display_name(),
            ?kind,
            ?origin,
            "scan start rejected: another scan is already running for this client"
        );
        local_scan_in_progress_error_message().to_owned()
    })?;
    let local_permit = match state.local_scan.get(client.into()).try_start() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(
                client = client.display_name(),
                ?kind,
                ?origin,
                "scan start rejected: local index writer is busy with another client"
            );
            scan_state
                .finish_failed(
                    now_epoch_ms(),
                    &local_scan_client_failure_message(
                        client.display_name(),
                        local_scan_writer_busy_failure_detail(),
                    ),
                )
                .await;
            return Err(local_scan_writer_busy_message().to_owned());
        }
    };
    let _scan_id = lease.scan_id;
    tracing::info!(
        client = client.display_name(),
        ?kind,
        ?origin,
        "scan started"
    );
    spawn_scan_task(
        ScanTask {
            client,
            scanner: Arc::clone(&state.agent_clients.get(client.into()).local_scanner),
            operation: ScanTaskOperation::Scan { kind, origin },
            cancellation: local_permit.cancellation_token(),
            scan: Arc::clone(&scan_state),
            coverage_state: Arc::clone(state.coverages.get(client.into())),
            roots_state: Arc::clone(state.roots.get(client.into())),
        },
        local_permit,
    );
    Ok(scan_state.snapshot().await)
}

/// 在固定客户端顺序中去重并拒绝空批次或重复输入。
fn ordered_refresh_clients(
    clients: &[AgentClientKindDto],
) -> Result<Vec<AgentClientKindDto>, String> {
    if clients.is_empty() {
        return Err("local index refresh requires at least one client".to_owned());
    }
    let ordered = AgentClientKindDto::ALL
        .into_iter()
        .filter(|client| clients.contains(client))
        .collect::<Vec<_>>();
    if ordered.len() != clients.len() {
        return Err("local index refresh clients must be unique".to_owned());
    }
    Ok(ordered)
}

/// 把受限 IPC 触发来源映射为 core 扫描来源，并执行初始化与发现终态门禁。
fn refresh_origin_for_context(
    trigger: LocalIndexRefreshTriggerDto,
    initialization_completed: bool,
    discovery_lifecycle: RootDiscoveryLifecycle,
) -> Result<ScanStartOrigin, String> {
    match trigger {
        LocalIndexRefreshTriggerDto::Initialization => {
            if initialization_completed {
                return Err("initialization index refresh is no longer available".to_owned());
            }
            if discovery_batch_index_decision(
                DiscoveryBatchKind::Initialization,
                discovery_lifecycle,
            ) != DiscoveryBatchIndexDecision::Index
            {
                return Err("initialization discovery is not ready for indexing".to_owned());
            }
            // 用户在向导确认 Agent 后才会调用本入口；core 的 `ExplicitUser`
            // 正是初始化未完成时允许的窄入口。`InitialAutomatic` 只用于完成
            // 初始化后认领的后台首次扫描，在这里使用会让本轮必然被门禁拒绝。
            Ok(ScanStartOrigin::ExplicitUser)
        }
        LocalIndexRefreshTriggerDto::DiscoveryBatch => {
            if !initialization_completed {
                return Err(
                    "initialization must complete before an explicit index refresh".to_owned(),
                );
            }
            if discovery_batch_index_decision(DiscoveryBatchKind::ExplicitUser, discovery_lifecycle)
                != DiscoveryBatchIndexDecision::Index
            {
                return Err("data source discovery is not ready for indexing".to_owned());
            }
            Ok(ScanStartOrigin::ExplicitUser)
        }
        LocalIndexRefreshTriggerDto::DirectManual => {
            if !initialization_completed {
                return Err("initialization must complete before a manual index refresh".to_owned());
            }
            Ok(ScanStartOrigin::ExplicitUser)
        }
    }
}

/// 在一个全局 writer 许可内按固定 Agent 顺序执行近 30 日统一索引。
async fn refresh_local_indexes_for_state(
    state: &AppRuntimeState,
    clients: &[AgentClientKindDto],
    trigger: LocalIndexRefreshTriggerDto,
) -> Result<Vec<ScanStatusDto>, String> {
    let clients = ordered_refresh_clients(clients)?;
    if trigger != LocalIndexRefreshTriggerDto::Initialization && clients.len() != 1 {
        return Err("an explicit data source refresh must target one client".to_owned());
    }
    let origin = refresh_origin_for_context(
        trigger,
        state.initialization_completed().await,
        state.root_discovery.snapshot().lifecycle,
    )?;
    refresh_local_indexes_with_origin(state, clients, origin).await
}

/// 在一个全局 writer 许可内按固定 Agent 顺序执行指定来源的近 30 日索引。
async fn refresh_local_indexes_with_origin(
    state: &AppRuntimeState,
    clients: Vec<AgentClientKindDto>,
    origin: ScanStartOrigin,
) -> Result<Vec<ScanStatusDto>, String> {
    for client in &clients {
        ensure_scan_start_access_by_policy(state, origin, ScanKindDto::Quick).await?;
        if state.scans.get((*client).into()).snapshot().await.state == ScanStateDto::Running {
            return Err(local_scan_in_progress_error_message().to_owned());
        }
    }

    let coordinator = state.local_scan.get(AgentClientKindDto::Codex.into());
    let permit = coordinator
        .start_when_available()
        .await
        .map_err(|_| local_scan_writer_busy_message().to_owned())?;
    let cancellation = permit.cancellation_token();
    let mut statuses = Vec::with_capacity(clients.len());
    for client in clients {
        let account_context_guard = if client == AgentClientKindDto::Codex {
            Some(state.lock_codex_account_context().await)
        } else {
            None
        };
        let scan_state = Arc::clone(state.scans.get(client.into()));
        let lease = scan_state
            .start(ScanKindDto::Quick, now_epoch_ms())
            .await
            .map_err(|_| local_scan_in_progress_error_message().to_owned())?;
        drop(account_context_guard);
        tracing::info!(
            client = client.display_name(),
            scan_id = lease.scan_id,
            ?origin,
            "batched local index refresh started"
        );
        execute_scan_task(ScanTask {
            client,
            scanner: Arc::clone(&state.agent_clients.get(client.into()).local_scanner),
            operation: ScanTaskOperation::Scan {
                kind: ScanKindDto::Quick,
                origin,
            },
            cancellation: cancellation.clone(),
            scan: Arc::clone(&scan_state),
            coverage_state: Arc::clone(state.coverages.get(client.into())),
            roots_state: Arc::clone(state.roots.get(client.into())),
        })
        .await?;
        statuses.push(scan_state.snapshot().await);
    }
    drop(permit);
    Ok(statuses)
}

/// 从已开放 Agent 的当前状态中固定排序选择升级后必须立即重建的索引。
fn upgrade_reindex_clients(
    enabled: loki_metis_core::EnabledAgents,
    states: &[(AgentClientKindDto, loki_metis_core::LocalIndexState)],
) -> Vec<AgentClientKindDto> {
    AgentClientKindDto::ALL
        .into_iter()
        .filter(|client| {
            enabled.contains((*client).into())
                && states
                    .iter()
                    .find_map(|(candidate, state)| (candidate == client).then_some(*state))
                    .is_some_and(immediate_reindex_required)
        })
        .collect()
}

/// 启动时迁移各客户端数据库；明确 NeedsRescan 的已开放索引立即逐项回补近 30 日。
pub(crate) async fn refresh_indexes_requiring_upgrade(state: &AppRuntimeState) {
    if !state.initialization_completed().await {
        return;
    }
    let enabled = state.enabled_agents().await;
    let mut states = Vec::new();
    for client in AgentClientKindDto::ALL {
        if !enabled.contains(client.into()) {
            continue;
        }
        match state
            .agent_clients
            .get(client.into())
            .local_analysis
            .index_state()
            .await
        {
            Ok(index_state) => states.push((client, index_state)),
            Err(error) => tracing::warn!(
                client = client.display_name(),
                %error,
                "failed to inspect local index after database migration"
            ),
        }
    }

    for client in upgrade_reindex_clients(enabled, &states) {
        if let Err(error) = refresh_local_indexes_with_origin(
            state,
            vec![client],
            ScanStartOrigin::InitialAutomatic,
        )
        .await
        {
            tracing::warn!(
                client = client.display_name(),
                %error,
                "immediate local reindex after database migration failed"
            );
        }
    }
}

/// 初始化或发现批次收敛后，统一刷新固定客户端集合的近 30 日本机索引。
#[tauri::command]
pub(crate) async fn refresh_local_indexes(
    state: State<'_, AppRuntimeState>,
    clients: Vec<AgentClientKindDto>,
    trigger: LocalIndexRefreshTriggerDto,
) -> Result<Vec<ScanStatusDto>, String> {
    refresh_local_indexes_for_state(&state, &clients, trigger).await
}

/// 周期性调度入口：在一个共享 writer 许可内按给定顺序依次执行各客户端的
/// 当天快速索引，返回实际执行的客户端数量。writer 忙碌时整拍放弃，不排队。
pub(crate) async fn run_periodic_quick_scans(
    state: &AppRuntimeState,
    clients: &[AgentClientKindDto],
) -> usize {
    let Ok(permit) = state
        .local_scan
        .get(AgentClientKindDto::Codex.into())
        .try_start()
    else {
        return 0;
    };
    let cancellation = permit.cancellation_token();
    let mut executed = 0_usize;
    for client in clients {
        if execute_periodic_quick_scan(state, *client, cancellation.clone()).await {
            executed = executed.saturating_add(1);
        }
    }
    drop(permit);
    executed
}

/// 在已持有共享 writer 许可的前提下执行单个客户端的周期快速索引；
/// 未完成初始化、客户端仍在扫描或未开放时直接跳过并返回 false。
async fn execute_periodic_quick_scan(
    state: &AppRuntimeState,
    client: AgentClientKindDto,
    cancellation: loki_metis_core::ScanCancellation,
) -> bool {
    let scan_running =
        state.scans.get(client.into()).snapshot().await.state == ScanStateDto::Running;
    if ensure_periodic_quick_scan_allowed(state.initialization_completed().await, scan_running)
        .is_err()
    {
        return false;
    }
    if !state.enabled_agents().await.contains(client.into()) {
        return false;
    }
    let account_context_guard = if client == AgentClientKindDto::Codex {
        Some(state.lock_codex_account_context().await)
    } else {
        None
    };
    let scan_state = Arc::clone(state.scans.get(client.into()));
    let Ok(lease) = scan_state.start(ScanKindDto::Quick, now_epoch_ms()).await else {
        return false;
    };
    drop(account_context_guard);
    tracing::info!(
        client = client.display_name(),
        scan_id = lease.scan_id,
        kind = ?ScanKindDto::Quick,
        origin = ?ScanStartOrigin::PeriodicAutomatic,
        "scan started"
    );
    let _ = execute_scan_task(ScanTask {
        client,
        scanner: Arc::clone(&state.agent_clients.get(client.into()).local_scanner),
        operation: ScanTaskOperation::Scan {
            kind: ScanKindDto::Quick,
            origin: ScanStartOrigin::PeriodicAutomatic,
        },
        cancellation,
        scan: Arc::clone(&scan_state),
        coverage_state: Arc::clone(state.coverages.get(client.into())),
        roots_state: Arc::clone(state.roots.get(client.into())),
    })
    .await;
    true
}

/// 清空当前客户端的本产品派生索引，保留数据根登记与原始客户端文件。
#[tauri::command]
pub(crate) async fn clear_local_index(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
) -> Result<ClearIndexResultDto, String> {
    ensure_business_access(&state).await?;
    let _account_context_guard = if client == AgentClientKindDto::Codex {
        Some(state.lock_codex_account_context().await)
    } else {
        None
    };
    if state.local_scan.get(client.into()).is_running() {
        tracing::warn!(
            client = client.display_name(),
            "index clear rejected: a scan is currently running for this client"
        );
        return Err(clear_local_index_while_scanning_message().to_owned());
    }
    let local_analysis = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    if let Err(error) = local_analysis.clear_index().await {
        tracing::error!(client = client.display_name(), %error, "index clear failed");
        return Err(error);
    }
    tracing::info!(client = client.display_name(), "index cleared");

    state.mark_index_cleared(client).await;
    *state.coverages.get(client.into()).write().await = empty_coverage();
    Ok(ClearIndexResultDto {
        cleared: true,
        message: clear_local_index_success_message(client.display_name()),
        message_code: UiMessageCodeDto::IndexCleared,
    })
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod batch_tests;
