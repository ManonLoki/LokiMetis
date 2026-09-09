//! 用单一扫描间隔触发器驱动全部本机客户端的周期快速扫描。

use std::collections::BTreeMap;
use std::time::Duration;

use loki_metis_core::{
    PeriodicScanTick, ScanCancellation, SourceClientKind, decide_periodic_scan_tick,
    next_local_day_start_epoch_ms,
};
use tauri::{AppHandle, Manager};

use crate::dto::AgentClientKindDto;
use crate::runtime::{AppRuntimeState, now_epoch_ms};
use crate::tray::refresh_tray_daily_token_title;

use super::scan_orchestration::ScanTaskShutdown;
use super::{refresh_indexes_requiring_upgrade, run_periodic_quick_scans};

/// 在 GUI 生命周期内启动一条共享节拍循环，到点后一次覆盖全部已开放客户端。
// 不再为每个客户端独立 sleep，也不再每拍只轮转一个客户端：一条循环读取同一
// 扫描间隔，到点后在同一个 writer 许可内按 Codex / Claude Code / Grok 顺序
// 串行执行各自的当天快速索引，所以每个客户端的刷新周期就是用户设置的间隔。
pub(crate) fn spawn_periodic_local_scans(app_handle: AppHandle) {
    let cancellation = ScanCancellation::new();
    let loop_app_handle = app_handle.clone();
    let state = app_handle.state::<AppRuntimeState>();
    if let Err(error) = state
        .scan_tasks
        .spawn(cancellation, move |shutdown| async move {
            run_shared_scan_interval_loop(loop_app_handle, shutdown).await;
        })
    {
        tracing::warn!(error, "periodic local scan owner rejected startup");
    }
}

/// 按当前扫描间隔循环触发一轮共享节拍。
async fn run_shared_scan_interval_loop(app_handle: AppHandle, mut shutdown: ScanTaskShutdown) {
    if shutdown.is_cancelled() {
        return;
    }
    refresh_tray_daily_token_title(&app_handle).await;
    if shutdown.is_cancelled() {
        return;
    }
    {
        let state = app_handle.state::<AppRuntimeState>();
        refresh_indexes_requiring_upgrade(state.inner()).await;
    }
    if shutdown.is_cancelled() {
        return;
    }
    loop {
        {
            let state = app_handle.state::<AppRuntimeState>();
            run_shared_scan_interval_tick(state.inner()).await;
        }
        if shutdown.is_cancelled() {
            return;
        }
        refresh_tray_daily_token_title(&app_handle).await;
        let interval = {
            let state = app_handle.state::<AppRuntimeState>();
            state.inner().scan_interval().await.duration()
        };
        if !wait_for_scan_deadline(&app_handle, interval, &mut shutdown).await {
            return;
        }
    }
}

/// 在不新增第二条后台循环的前提下等待下一次扫描，并在跨当地日期时先清空旧日标题。
async fn wait_for_scan_deadline(
    app_handle: &AppHandle,
    interval: Duration,
    shutdown: &mut ScanTaskShutdown,
) -> bool {
    let deadline = tokio::time::Instant::now() + interval;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let until_day_change = duration_until_next_local_day(now_epoch_ms()).unwrap_or(remaining);
        if until_day_change >= remaining {
            return sleep_unless_shutdown(remaining, shutdown).await;
        }
        if !sleep_unless_shutdown(until_day_change, shutdown).await {
            return false;
        }
        refresh_tray_daily_token_title(app_handle).await;
    }
}

/// 睡满给定时长返回 true；关闭信号先到则立即返回 false。
async fn sleep_unless_shutdown(duration: Duration, shutdown: &mut ScanTaskShutdown) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => true,
        _ = shutdown.cancelled() => false,
    }
}

/// 把 core 给出的下一当地自然日起点转换为单调时钟等待时长。
fn duration_until_next_local_day(observed_at_epoch_ms: i64) -> Option<Duration> {
    let next_day = next_local_day_start_epoch_ms(observed_at_epoch_ms)?;
    let milliseconds = next_day.checked_sub(observed_at_epoch_ms)?;
    u64::try_from(milliseconds).ok().map(Duration::from_millis)
}

/// 一次间隔触发：共享 writer 空闲时按固定顺序依次扫描全部空闲客户端；忙碌则本拍放弃。
pub(crate) async fn run_shared_scan_interval_tick(state: &AppRuntimeState) {
    let enabled: Vec<SourceClientKind> = state.enabled_agents().await.iter().collect();
    let writer_busy = state
        .local_scan
        .get(AgentClientKindDto::Codex.into())
        .is_running();
    let mut running: BTreeMap<SourceClientKind, bool> = BTreeMap::new();
    for client in AgentClientKindDto::ALL {
        let kind = SourceClientKind::from(client);
        running.insert(kind, state.scans.get(kind).is_running());
    }
    let client_running = |client: SourceClientKind| {
        running
            .get(&client)
            .copied()
            .unwrap_or_else(|| unreachable!("EnabledAgents 从不产出 WorkBuddy，本闭包不会被它调用"))
    };
    match decide_periodic_scan_tick(&enabled, client_running, writer_busy) {
        PeriodicScanTick::Skip | PeriodicScanTick::Wait => {}
        PeriodicScanTick::Start { clients } => {
            let clients: Vec<AgentClientKindDto> =
                clients.into_iter().map(AgentClientKindDto::from).collect();
            run_periodic_quick_scans(state, &clients).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::{ScanKindDto, ScanStateDto};
    use crate::scan_state::ScanCoordinator;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// 把隔离 runtime 推进到完成向导后的状态，供周期扫描成功路径复用。
    async fn complete_initialization(state: &AppRuntimeState) {
        state
            .set_initialization_completed(true)
            .await
            .expect("fixture initialization completes");
    }

    /// 在测试中显式开放指定本机 Agent。
    async fn enable_agents(state: &AppRuntimeState, agents: &[crate::dto::UsageClientKindDto]) {
        state
            .set_enabled_agents(agents)
            .await
            .expect("fixture enables agents");
    }

    /// 读取指定客户端当前可见扫描状态。
    async fn scan_state(state: &AppRuntimeState, client: AgentClientKindDto) -> ScanStateDto {
        state.scans.get(client.into()).snapshot().state
    }

    /// 验证未完成向导时任何客户端都不会认领周期扫描，writer 也在返回前释放。
    #[tokio::test]
    async fn skips_all_periodic_scans_before_initialization_completes() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());

        let executed = run_periodic_quick_scans(&state, &AgentClientKindDto::ALL).await;
        assert_eq!(executed, 0);
        for client in AgentClientKindDto::ALL {
            assert_eq!(scan_state(&state, client).await, ScanStateDto::Idle);
            assert!(!state.local_scan.get(client.into()).is_running());
        }
    }

    /// 全关时周期节拍不得给任一客户端认领扫描。
    #[tokio::test]
    async fn shared_tick_skips_when_no_agents_enabled() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        run_shared_scan_interval_tick(&state).await;
        for client in AgentClientKindDto::ALL {
            assert_eq!(scan_state(&state, client).await, ScanStateDto::Idle);
        }
    }

    /// 验证忙碌客户端不会通过周期性入口重复认领扫描，也不会覆盖其现有 scan_id。
    #[tokio::test]
    async fn skips_periodic_start_while_client_scan_is_running() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        enable_agents(&state, &[crate::dto::UsageClientKindDto::ClaudeCode]).await;
        let coordinator: &Arc<ScanCoordinator> =
            state.scans.get(AgentClientKindDto::ClaudeCode.into());
        coordinator
            .start(ScanKindDto::Quick, 1, ScanCancellation::new())
            .expect("fixture starts a running scan");

        let executed = run_periodic_quick_scans(&state, &[AgentClientKindDto::ClaudeCode]).await;

        assert_eq!(executed, 0);
        let status = coordinator.snapshot();
        assert_eq!(status.state, ScanStateDto::Running);
        assert_eq!(status.scan_id.as_deref(), Some("scan-1"));
    }

    /// 验证空闲客户端可通过周期性入口完成各自的 Quick 扫描，并在返回前释放 writer。
    #[tokio::test]
    async fn runs_periodic_quick_scan_when_client_is_idle() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        enable_agents(&state, &[crate::dto::UsageClientKindDto::ClaudeCode]).await;

        let executed = run_periodic_quick_scans(&state, &[AgentClientKindDto::ClaudeCode]).await;

        assert_eq!(executed, 1);
        let status = state
            .scans
            .get(AgentClientKindDto::ClaudeCode.into())
            .snapshot();
        assert!(!matches!(
            status.state,
            ScanStateDto::Idle | ScanStateDto::Running
        ));
        assert_eq!(status.kind, ScanKindDto::Quick);
        assert!(status.started_at_epoch_ms.is_some());
        assert!(
            !state
                .local_scan
                .get(AgentClientKindDto::ClaudeCode.into())
                .is_running()
        );
    }

    /// 一拍必须覆盖全部已开放客户端：三个客户端都在同一拍内完成，而不是每拍只轮到一个。
    #[tokio::test]
    async fn shared_tick_scans_every_enabled_client_in_one_pass() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        enable_agents(&state, &crate::dto::UsageClientKindDto::ALL).await;

        run_shared_scan_interval_tick(&state).await;

        for client in AgentClientKindDto::ALL {
            let status = state.scans.get(client.into()).snapshot();
            assert!(
                !matches!(status.state, ScanStateDto::Idle | ScanStateDto::Running),
                "{} must finish within the same tick",
                client.display_name()
            );
            assert_eq!(status.kind, ScanKindDto::Quick);
        }
        assert!(
            !state
                .local_scan
                .get(AgentClientKindDto::Codex.into())
                .is_running()
        );
    }

    /// 只开放 Claude Code 与 Grok 时，Codex 保持 Idle，其余两者仍在同一拍完成。
    #[tokio::test]
    async fn shared_tick_only_scans_enabled_clients() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        enable_agents(
            &state,
            &[
                crate::dto::UsageClientKindDto::ClaudeCode,
                crate::dto::UsageClientKindDto::GrokBuildCli,
            ],
        )
        .await;

        run_shared_scan_interval_tick(&state).await;

        assert_eq!(
            scan_state(&state, AgentClientKindDto::Codex).await,
            ScanStateDto::Idle
        );
        for client in [
            AgentClientKindDto::ClaudeCode,
            AgentClientKindDto::GrokBuildCli,
        ] {
            assert!(!matches!(
                scan_state(&state, client).await,
                ScanStateDto::Idle | ScanStateDto::Running
            ));
        }
    }

    /// 共享 writer 被占用时整拍放弃且不排队；writer 释放后的下一拍覆盖全部客户端。
    #[tokio::test]
    async fn busy_writer_tick_waits_and_next_tick_scans_everyone() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        complete_initialization(&state).await;
        enable_agents(&state, &crate::dto::UsageClientKindDto::ALL).await;
        let writer = state
            .local_scan
            .get(AgentClientKindDto::Codex.into())
            .try_start()
            .expect("occupy the shared index writer as an explicit scan would");

        run_shared_scan_interval_tick(&state).await;
        for client in AgentClientKindDto::ALL {
            assert_eq!(scan_state(&state, client).await, ScanStateDto::Idle);
        }

        drop(writer);
        run_shared_scan_interval_tick(&state).await;
        for client in AgentClientKindDto::ALL {
            assert!(!matches!(
                scan_state(&state, client).await,
                ScanStateDto::Idle | ScanStateDto::Running
            ));
        }
    }

    /// 验证本机周期扫描与设置快照读取的是同一个扫描间隔。
    #[tokio::test]
    async fn local_scheduler_and_settings_share_the_same_scan_interval() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());
        state
            .set_scan_interval(7)
            .await
            .expect("shared scan interval is stored");
        let interval = state.scan_interval().await;
        assert_eq!(interval.get(), 7);
        let settings = state
            .privacy_settings(crate::dto::UsageClientKindDto::Codex)
            .await;
        assert_eq!(settings.scan_interval_minutes, 7);
        let stored = crate::privacy_store::load_settings(temp.path()).expect("settings reload");
        assert_eq!(stored.scan_interval.get(), 7);
    }
}
