//! 元数据发现和手动目录检查不持 writer；确认候选才进入独立的原子登记。

mod automatic;
mod manual_inspection;
mod manual;
mod subtree;
mod batch;
#[cfg(test)]
mod batch_tests;
pub use manual::ManualAddResult;

use super::{UsageService,ServiceContext,background::BackgroundTaskOwner};
use crate::backend::local_index::{LocalIndex,validate_local_plain_directory};
use loki_metris_core::*;
use std::{collections::BTreeSet,path::PathBuf,sync::{Arc,Mutex},time::{Duration,Instant}};
use automatic::{RootDiscoveryRunControl,supervise_root_discovery_boundaries,run_owned_root_discovery};

/// 发现的局部批次投影独立于扫描状态；候选保留到确认成功或新批次开始。
#[derive(Default)]
pub(super) struct DiscoveryBatch {
    /// 所有批次投影共用的锁，检查与修改在同一临界区内完成。
    state:Mutex<batch::BatchState>,
}

/// GUI 可读公开状态，不返回库句柄或原始记录。
#[derive(Clone,Default,PartialEq,Eq)]
pub struct DiscoveryView {
    /// 发现任务的生命周期与进度。
    pub status:RootDiscoveryStatus,
    /// 尚未确认的候选目录。
    pub candidates:Vec<RootCandidate>,
    /// 登记失败的候选 ID。
    pub failed_ids:BTreeSet<String>,
    /// 正在执行批次索引的 Agent。
    pub indexing:BTreeSet<SourceClientKind>,
    /// 用户手动重试中的候选 ID。
    pub retrying_ids:BTreeSet<String>,
    /// 最近一次批次索引的失败类别。
    pub error:Option<&'static str>,
}

impl UsageService {
    /// 发起当前页面绑定客户端的自动登记批次，全局发现仍同时识别三种客户端。
    pub fn start_discovery(self:&Arc<Self>,client:SourceClientKind,scope:RootDiscoveryScope)->Result<(),&'static str> {
        if client==SourceClientKind::WorkBuddy || scope==RootDiscoveryScope::ManualSubtree {return Err("source_discovery_unsupported")}
        if subtree::manual_add_busy() {return Err("source_discovery_busy")}
        let runtime=self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _runtime=runtime.as_ref().ok_or("source_service_closed")?;
        let deadline=Instant::now()+automatic::ROOT_DISCOVERY_TOTAL_TIMEOUT;
        let generation=self.discovery_batch.begin_discovery(&self.discovery,RootDiscoveryStrategy::current(),scope,0)?;
        let control=RootDiscoveryRunControl::new(self.discovery.clone(),Arc::new(RootDiscoveryCoordinator::default()));
        let monitor=control.clone();
        if let Err(error)=self.background.spawn("discovery-deadline",move|shutdown|async move {
            supervise_root_discovery_boundaries(monitor,deadline,move||shutdown.is_cancelled()).await;
        }) {control.fail_worker(deadline);return Err(error)}
        let cancel=control.clone();let worker=control.clone();
        if let Err(error)=self.background.spawn_blocking_cancelable("source-discovery",move||cancel.cancel(),move|_| {
            run_owned_root_discovery(worker,scope,deadline);
        }) {control.fail_worker(deadline);return Err(error)}
        if let Err(error)=self.spawn_discovery_batch(client,generation) {control.cancel();return Err(error)}
        Ok(())
    }

    /// 独立取消只作用于发现，绝不把 Token 索引误当作元数据遍历。
    pub fn cancel_discovery(&self)->Result<(),&'static str> {
        if self.discovery.request_cancel() {Ok(())}else{Err("source_discovery_not_running")}
    }

    /// 公开的候选绝对路径只由数据源候选区域使用。
    pub fn discovery_view(&self)->DiscoveryView {
        let state=self.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        DiscoveryView {status:self.discovery.snapshot(),candidates:self.discovery.candidates(),
            failed_ids:state.failed.clone(),indexing:state.indexing.clone(),retrying_ids:state.retrying.clone(),error:state.error}
    }
}

/// 验证由 owner 持有的 blocking 任务执行，取消页面 future 不会遗失实际任务句柄。
async fn register_candidate(context:&Arc<ServiceContext>,owner:&Arc<BackgroundTaskOwner>,coordinator:&Arc<RootDiscoveryCoordinator>,batch:&DiscoveryBatch,generation:u64,id:&str)
    ->Result<(),&'static str> {
    if !batch.current(generation) {return Err("source_candidate_stale")}
    let candidate=coordinator.select_candidates(&[id.to_owned()]).map_err(|_|"source_candidate_invalid")?.into_iter().next().ok_or("source_candidate_invalid")?;
    if candidate.client==SourceClientKind::WorkBuddy {return Err("source_candidate_invalid")}
    let path=PathBuf::from(&candidate.absolute_path);let checked=path.clone();let(tx,rx)=tokio::sync::oneshot::channel();
    owner.spawn_blocking("source-candidate-validation",move|_|{let _=tx.send(validate_local_plain_directory(&checked));})?;
    rx.await.map_err(|_|"source_candidate_invalid")?.map_err(|_|"source_candidate_invalid")?;
    if !batch.current(generation) {return Err("source_candidate_stale")}
    let permit=context.writer.try_start().map_err(|_|"source_writer_busy")?;
    let directory=source_client_app_data_dir(&context.directory,candidate.client);
    let mut index=LocalIndex::open_in_app_data(&directory,candidate.client.parser_version()).await.map_err(|_|"source_update_failed")?;
    if permit.cancellation_token().is_cancelled()||!batch.current(generation) {return Err("source_operation_cancelled")}
    let root_id=stable_id(candidate.client.root_id_namespace(),&path_key(&path));
    let inserted=index.register_confirmed_root_fields(&root_id,&path,&source_root_alias_from_path(&path,candidate.client),DiscoveryMethod::MetadataDiscovery)
        .await.map_err(|_|"source_update_failed")?;
    // 同根重复发现仍扫描新用量；只有新增来源才让通知抑制待发项并重建基线。
    if inserted {context.notification_source_epoch.fetch_add(1,std::sync::atomic::Ordering::Release);}
    // 登记已提交，立即更新覆盖和可见代数；后续取消不能掩盖已经发生的变更。
    context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner).entry(candidate.client).or_insert_with(empty_coverage).state=CoverageState::Partial;
    {let mut status=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);status.revision=status.revision.wrapping_add(1);}
    context.today_refresh.notify_one();
    if permit.cancellation_token().is_cancelled()||!batch.current(generation) {return Err("source_operation_cancelled")}
    index.queue_confirmed_root_for_background_scan(&root_id).await.map_err(|_|"source_update_failed")?;
    // 新批次可在 SQLite 提交期间开始，旧回执不能清理新批次同 ID 候选。
    batch.update(generation,|state| {
        coordinator.remove_candidates(&[id.to_owned()]);
        let revision=state.registrations_revision.entry(candidate.client).or_default();*revision=revision.wrapping_add(1);
    });
    Ok(())
}
