//! 异步编排发现与增量索引；进度换算收在 [`progress`] 子模块内，
//! Tauri command 层只负责调用入口。

mod adapters;
mod claude_execute;
mod client;
mod progress;
mod run;
mod support;

use std::sync::Arc;

#[cfg(all(test, unix))]
use loki_metis_core::SourceClientKind;
use loki_metis_core::{
    CoverageReport, CoverageState, LocalScanFuture, LocalScanOutput, LocalScanProgress,
    LocalUsageScanner, ScanCancellation, ScanKind, ScanStartOrigin, SourceRootReindexRequest,
    local_scan_client_failure_message, scan_progress_finished_message,
};
#[cfg(test)]
use loki_metis_core::{
    DiscoveredRootIdentity, RegisteredRootIdentity, merge_coverage_reports,
    merge_discovery_results, reuse_registered_root_identities,
};

use crate::agent_client::{ClaudeLocalUsageScanner, CodexLocalUsageScanner, GrokLocalUsageScanner};
#[cfg(all(test, unix))]
use crate::backend::local_index::LocalIndex;
#[cfg(test)]
use crate::backend::local_index::{DiscoveryResult, RegisteredRoot};
use crate::dto::{AgentClientKindDto, ScanKindDto, ScanScopeCodeDto};
use crate::local_view::to_source_root_dto;
use crate::runtime::now_epoch_ms;
use crate::tray::refresh_tray_daily_token_title;
use client::CodexClient;
use progress::publish_scan_progress;
#[cfg(test)]
pub(crate) use support::{
    full_device_options_from_local_volume_roots, local_scan_error_message_from_local,
};

/// 已经取得全局 writer 的本机扫描或单根重建任务。
pub(crate) enum ScanTaskOperation {
    /// 按扫描范围与触发来源执行既有发现/索引。
    Scan {
        /// 快速或全设备。
        kind: ScanKindDto,
        /// 初始化、周期或用户显式触发。
        origin: ScanStartOrigin,
    },
    /// 只强制重建一个已校验数据根。
    ReindexSourceRoot {
        /// 已经过 catalog 启用校验的稳定根 ID。
        request: SourceRootReindexRequest,
    },
}

/// 在 Tauri runtime 中启动异步扫描任务；结束后回写轻量状态。
pub(crate) struct ScanTask {
    pub client: AgentClientKindDto,
    pub scanner: Arc<dyn LocalUsageScanner>,
    pub operation: ScanTaskOperation,
    pub cancellation: ScanCancellation,
    pub scan: Arc<crate::scan_state::ScanCoordinator>,
    pub coverage_state: Arc<tokio::sync::RwLock<CoverageReport>>,
    pub roots_state: Arc<tokio::sync::RwLock<Vec<crate::dto::SourceRootDto>>>,
}

/// 把已取得的 writer 许可持有到后台扫描结束；释放 writer 后再按需刷新托盘。
pub(crate) fn spawn_scan_task(
    task: ScanTask,
    permit: crate::backend::local_index::ScanPermit,
    app_handle: Option<tauri::AppHandle>,
) {
    tauri::async_runtime::spawn(async move {
        let outcome = {
            let _permit = permit;
            execute_scan_task(task).await
        };
        if outcome.is_ok()
            && let Some(app_handle) = app_handle
        {
            refresh_tray_daily_token_title(&app_handle).await;
        }
    });
}

/// 执行一项已经取得全局 writer 的扫描，并把成功、取消或失败写回状态。
pub(crate) async fn execute_scan_task(task: ScanTask) -> Result<(), String> {
    let ScanTask {
        client,
        scanner,
        operation,
        cancellation,
        scan,
        coverage_state,
        roots_state,
    } = task;
    let scan_for_progress = Arc::clone(&scan);
    let result = match operation {
        ScanTaskOperation::Scan { kind, origin } => {
            scanner
                .execute(
                    to_core_scan_kind(kind),
                    origin,
                    cancellation,
                    Box::new(move |progress| {
                        publish_scan_progress(&scan_for_progress, progress);
                    }),
                )
                .await
        }
        ScanTaskOperation::ReindexSourceRoot { request } => {
            scanner
                .reindex_source_root(
                    request,
                    cancellation,
                    Box::new(move |progress| {
                        publish_scan_progress(&scan_for_progress, progress);
                    }),
                )
                .await
        }
    };

    match result {
        Ok(output) => {
            tracing::info!(
                client = client.display_name(),
                files_scanned = output.files_scanned,
                call_count = output.call_count,
                coverage_state = ?output.coverage.state,
                "local scan finished"
            );
            scan.update_progress(
                output.files_scanned,
                output.call_count,
                10_000,
                ScanScopeCodeDto::IndexingRoots,
                None,
                scan_progress_finished_message().to_owned(),
            )
            .await;
            *coverage_state.write().await = output.coverage.clone();
            *roots_state.write().await = output.roots.into_iter().map(to_source_root_dto).collect();
            if output.coverage.state == CoverageState::Cancelled {
                scan.finish_cancelled(now_epoch_ms()).await;
            } else {
                scan.finish_completed(now_epoch_ms()).await;
            }
            Ok(())
        }
        Err(error) => {
            tracing::warn!(
                client = client.display_name(),
                %error,
                "local scan failed"
            );
            scan.finish_failed(
                now_epoch_ms(),
                &local_scan_client_failure_message(client.display_name(), &error),
            )
            .await;
            Err(error)
        }
    }
}

impl LocalUsageScanner for CodexLocalUsageScanner {
    /// 使用 Codex rollout 结构签名、现有预算和 SQLite 增量索引执行扫描。
    fn execute(
        &self,
        kind: ScanKind,
        origin: ScanStartOrigin,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        let app_data_dir = self.app_data_dir().to_path_buf();
        let gate = self.account_context_gate_arc();
        Box::pin(run::run_scan::<CodexClient>(
            app_data_dir.clone(),
            app_data_dir,
            Some(gate),
            kind,
            origin,
            cancellation,
            on_progress,
        ))
    }

    /// 只重验证并强制重建请求指定的 Codex 数据根。
    fn reindex_source_root(
        &self,
        request: SourceRootReindexRequest,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        let app_data_dir = self.app_data_dir().to_path_buf();
        let gate = self.account_context_gate_arc();
        Box::pin(run::run_reindex::<CodexClient>(
            app_data_dir.clone(),
            app_data_dir,
            Some(gate),
            request,
            cancellation,
            on_progress,
        ))
    }
}

impl LocalUsageScanner for GrokLocalUsageScanner {
    /// 使用 Grok sessions/updates.jsonl 签名和独立 SQLite 执行本机扫描。
    fn execute(
        &self,
        kind: ScanKind,
        origin: ScanStartOrigin,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        let app_data_dir = self.app_data_dir().to_path_buf();
        let settings_app_data_dir = self.product_app_data_dir().to_path_buf();
        Box::pin(run::run_scan::<client::GrokClient>(
            app_data_dir,
            settings_app_data_dir,
            None,
            kind,
            origin,
            cancellation,
            on_progress,
        ))
    }

    /// 只重验证并强制重建请求指定的 Grok 数据根。
    fn reindex_source_root(
        &self,
        request: SourceRootReindexRequest,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(run::run_reindex::<client::GrokClient>(
            self.app_data_dir().to_path_buf(),
            self.product_app_data_dir().to_path_buf(),
            None,
            request,
            cancellation,
            on_progress,
        ))
    }
}

impl LocalUsageScanner for ClaudeLocalUsageScanner {
    /// 使用 Claude projects transcript 签名和独立 SQLite 执行本机扫描。
    fn execute(
        &self,
        kind: ScanKind,
        origin: ScanStartOrigin,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        let app_data_dir = self.app_data_dir().to_path_buf();
        let settings_app_data_dir = self.product_app_data_dir().to_path_buf();
        Box::pin(execute_claude_scan(
            app_data_dir,
            settings_app_data_dir,
            kind,
            origin,
            cancellation,
            on_progress,
        ))
    }

    /// 只重验证并强制重建请求指定的 Claude 数据根。
    fn reindex_source_root(
        &self,
        request: SourceRootReindexRequest,
        cancellation: ScanCancellation,
        on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
    ) -> LocalScanFuture<'_, Result<LocalScanOutput, String>> {
        Box::pin(run::run_reindex::<client::ClaudeClient>(
            self.app_data_dir().to_path_buf(),
            self.product_app_data_dir().to_path_buf(),
            None,
            request,
            cancellation,
            on_progress,
        ))
    }
}

use claude_execute::execute_claude_scan;

/// 把 DTO 扫描范围映射为 core 的扫描范围枚举。
fn to_core_scan_kind(kind: ScanKindDto) -> ScanKind {
    match kind {
        ScanKindDto::Quick => ScanKind::Quick,
        ScanKindDto::FullDevice => ScanKind::FullDevice,
    }
}

#[cfg(test)]
mod current_scan_tests;
#[cfg(test)]
mod reindex_tests;
#[cfg(test)]
mod retention_scan_tests;
#[cfg(test)]
mod tests;
