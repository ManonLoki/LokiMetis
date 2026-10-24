//! 一个应用生命周期节拍与一次启动清理共享索引 writer；保存设置不打断当前等待。

use super::*;
use loki_metris_host::local_index::{LocalIndex,current_epoch_ms};
use loki_metris_core::{ScanCancellation,ScanStartOrigin,TimeStandard,source_client_app_data_dir,source_client_usage_index_path,retention_cutoff_epoch_ms};
use std::sync::atomic::Ordering;

impl UsageService {
    /// 只在应用连接真实用量服务后启动一次；单元 fixture 构造服务不会读取 HOME。
    pub fn start_usage_lifecycle(self:&Arc<Self>)->Result<(),&'static str> {
        if self.lifecycle_started.swap(true,Ordering::AcqRel) {return Ok(())}
        let context=self.context.clone();
        let result=self.background.spawn("usage-periodic-lifecycle",move|mut shutdown|async move {
            if let Err(error)=startup_cleanup(context.clone(),&mut shutdown).await {
                tracing::warn!(event="usage_startup_cleanup_failed",code=error);
            }
            refresh_upgraded_indexes(context.clone(),&mut shutdown).await;
            loop {
                if shutdown.is_cancelled() {return}
                if let Err(error)=super::super::scan::run(context.clone(),super::super::queue::Intent{client:None,origin:ScanStartOrigin::PeriodicAutomatic},ScanCancellation::new()).await {tracing::warn!(event="usage_periodic_scan_failed",code=error);}
                if shutdown.is_cancelled() {return}
                // 后台节拍无法向界面报告失败且不能停摆，只有这里记录后退回默认间隔；账户查询直接失败关闭。
                let interval=match context.settings.scan_interval().await {Ok(interval)=>interval,Err(error)=>{tracing::warn!(event="usage_scan_interval_unavailable",code=error);loki_metris_core::ScanIntervalMinutes::default()}}.duration();
                // 每轮结束后读取间隔；已开始的等待不会因设置保存而被提前唤醒。
                tokio::select! {_=tokio::time::sleep(interval)=>{},_=shutdown.cancelled()=>return}
            }
        });
        if result.is_err() {self.lifecycle_started.store(false,Ordering::Release);}result
    }
}

/// 清理只打开已存在物理库，三个客户端在同一个 writer 许可内依次处理。
async fn startup_cleanup(context:Arc<super::super::ServiceContext>,shutdown:&mut super::super::background::BackgroundTaskShutdown)->Result<(),&'static str> {
    startup_cleanup_at(context,shutdown,current_epoch_ms()).await
}

/// 当前时刻作为输入让宿主接线的边界回归可重复，日期算法仍由 core 决定。
async fn startup_cleanup_at(context:Arc<super::super::ServiceContext>,shutdown:&mut super::super::background::BackgroundTaskShutdown,observed_at_epoch_ms:i64)->Result<(),&'static str> {
    let settings=context.settings.snapshot().await?;
    let cutoff=retention_cutoff_epoch_ms(settings.retention_days,observed_at_epoch_ms,&TimeStandard::Local,&jiff::tz::TimeZone::system()).map_err(|_|"usage_retention_invalid")?;
    let permit=tokio::select! {
        biased;
        _=shutdown.cancelled()=>return Ok(()),
        permit=context.writer.start_when_available()=>permit.map_err(|_|"usage_service_closed")?,
    };
    for client in super::super::query::physical_clients() {
        if shutdown.is_cancelled()||permit.cancellation_token().is_cancelled() {break}
        if !source_client_usage_index_path(&context.directory,client).is_file() {continue}
        let directory=source_client_app_data_dir(&context.directory,client);
        let result=async {
            let mut index=LocalIndex::open_in_app_data(&directory,client.parser_version()).await?;
            if shutdown.is_cancelled()||permit.cancellation_token().is_cancelled() {return Ok(())}
            index.prune_usage_before(cutoff).await
        }.await;
        if result.is_err() {tracing::warn!(event="usage_retention_client_failed",client=?client);}
    }
    let mut state=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);state.revision=state.revision.wrapping_add(1);Ok(())
}

/// 仅升级后 NeedsRescan 的已有物理索引回补近30日，普通 Ready 启动不重复 initial 扫描。
async fn refresh_upgraded_indexes(context:Arc<super::super::ServiceContext>,shutdown:&mut super::super::background::BackgroundTaskShutdown) {
    let Ok((clients, _))=context.agents.dashboard_selection().await else {return};
    for client in clients {
        if shutdown.is_cancelled() {return}
        let path=source_client_usage_index_path(&context.directory,client);if !path.is_file() {continue}
        let required=match LocalIndex::open_read_only_in_app_data(&source_client_app_data_dir(&context.directory,client),client.parser_version()).await {
            Ok(index)=>index.index_state().await.is_ok_and(loki_metris_core::immediate_reindex_required),Err(_)=>false,
        };
        if shutdown.is_cancelled() {return}
        if required&& let Err(error)=super::super::scan::run(context.clone(),super::super::queue::Intent{client:Some(client),origin:ScanStartOrigin::InitialAutomatic},ScanCancellation::new()).await {tracing::warn!(event="usage_startup_upgrade_scan_failed",code=error);}
    }
}

#[cfg(test)]
#[path="lifecycle_tests.rs"]
mod tests;
