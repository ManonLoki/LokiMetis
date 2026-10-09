//! 单 writer 扫描只提交被完成的覆盖事实；查询与切页不参与取消。

use super::{ServiceContext, queue::Intent};
use crate::backend::{scan_orchestration, local_index::ScanProgress};
use loki_metris_core::{ScanCancellation, SourceClientKind, LocalScanProgress, CoverageState, source_client_app_data_dir};
use std::sync::Arc;

/// 同一产品全局 writer 许可持有到扫描和覆盖状态提交均完成。
pub(super) async fn run(context: Arc<ServiceContext>, intent: Intent, cancellation: ScanCancellation) -> Result<(), &'static str> {
    let (enabled_clients, _) = match context.agents.dashboard_selection().await {
        Ok(selection)=>selection,Err(error)=>{
            let mut state=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.error=Some(error);state.revision=state.revision.wrapping_add(1);return Err(error);
        }
    };
    let automatic = intent.origin != loki_metris_core::ScanStartOrigin::ExplicitUser;
    let clients = intent.client.map(|client| vec![client]).unwrap_or(enabled_clients.clone())
        .into_iter().filter(|client| !automatic || enabled_clients.contains(client)).collect::<Vec<_>>();
    if clients.is_empty() {return Ok(())}
    let retention=match context.settings.snapshot().await {
        Ok(settings)=>settings.retention_days,
        Err(error)=>{
            // 预检不拥有 writer，不能清除另一个实际扫描的 running 状态。
            let mut state=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.error=Some(error);state.revision=state.revision.wrapping_add(1);return Err(error);
        }
    };
    let periodic = intent.origin == loki_metris_core::ScanStartOrigin::PeriodicAutomatic;
    let permit = if periodic {
        match context.writer.try_start() { Ok(permit) => permit, Err(_) => return Ok(()) }
    } else {
        context.writer.start_when_available().await.map_err(|_| "usage_scan_busy")?
    };
    if cancellation.is_cancelled() {return Err("usage_scan_cancelled")}
    let cancellation=permit.cancellation_token();
    let _finish = Finish(context.clone());
    {
        let mut state = context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.running = true; state.error = None; state.progress = None;
    }
    for client in clients {
        if cancellation.is_cancelled() { return fail(&context, "usage_scan_cancelled") }
        if automatic {match context.agents.dashboard_selection().await {Ok((enabled,_))=>if !enabled.contains(&client){continue},Err(error)=>return fail(&context,error)}}
        if client == SourceClientKind::WorkBuddy { continue }
        {let mut state=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);state.client=Some(client);state.progress=None;}
        let progress_state = context.clone();
        let output = scan_orchestration::scan(source_client_app_data_dir(&context.directory, client), client,
            retention, intent.origin, cancellation.clone(), Box::new(move |progress| {
                if let LocalScanProgress::Indexing { current_root_id, roots_completed, roots_total, files_scanned, calls_added, .. } = progress {
                    progress_state.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).progress = Some(ScanProgress {
                        scan_id: String::new(), current_root_id: current_root_id.unwrap_or_default(), roots_completed, roots_total,
                        files_scanned, calls_added, warning_count: 0,
                    });
                }
            })).await;
        match output {
            Ok(output) => {
                let cancelled = output.coverage.state == CoverageState::Cancelled;
                context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(client, output.coverage);
                if cancelled { return fail(&context, "usage_scan_cancelled") }
            }
            Err(_) => {
                if cancellation.is_cancelled() { return fail(&context, "usage_scan_cancelled") }
                if !periodic { return fail(&context, "usage_scan_failed") }
                let _ = fail(&context, "usage_scan_failed");
            }
        }
    }
    Ok(())
}

/// 错误只保存稳定分类；失败不替换先前可信覆盖报告。
fn fail(context: &ServiceContext, error: &'static str) -> Result<(), &'static str> {
    context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).error = Some(error);
    Err(error)
}

/// 正常、失败或 unwind 都关闭可见运行状态，唤醒页面重新读取已有索引。
struct Finish(Arc<ServiceContext>);
impl Drop for Finish {
    /// SQLite 收尾先于本 guard 析构，revision 不早于实际扫描 future 完成。
    fn drop(&mut self) {
        self.0.today_refresh.notify_one();
        let mut state = self.0.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if std::thread::panicking() { state.error = Some("usage_worker_failed"); }
        state.running = false; state.revision = state.revision.wrapping_add(1);
    }
}
