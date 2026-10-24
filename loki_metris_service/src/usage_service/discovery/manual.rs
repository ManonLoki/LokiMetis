//! 原生选择完成后按历史规则先核对五秒，未确认根则进行六十秒子树发现。

use super::*;
use super::{manual_inspection::{inspect_selected_path_owned,ManualInspectionCompletion,MANUAL_INSPECTION_TIMEOUT},
    subtree::{ManualAddReservation,ManualSubtreeRunControl,supervise_manual_subtree_boundaries,run_manual_subtree_discovery}};
use loki_metris_host::source_catalog::SourceRootCatalogAdapter;

/// 选择器打开前取得的进程内唯一预约；取消选择即释放，不持 SQLite writer。
pub struct ManualAddLease(Arc<ManualAddReservation>);

/// 回执区分取消、直接登记和已启动深搜，不把取消显示为失败。
pub enum ManualAddResult {
    /// 用户取消了目录选择。
    Cancelled,
    /// 目录已登记为数据根，附变更结果与随后刷新的结果。
    Registered{outcome:SourceRootMutationOutcome,refresh:Result<(),&'static str>},
    /// 目录不是根，已开始在其子树中深度搜索。
    DeepSearchStarted,
}

impl UsageService {
    /// 在展示原生选择器前同步拒绝重复流程和不支持的客户端。
    pub fn begin_manual_add(&self,client:SourceClientKind)->Result<ManualAddLease,&'static str> {
        if client==SourceClientKind::WorkBuddy {return Err("source_client_unsupported")}
        if self.discovery.snapshot().lifecycle==RootDiscoveryLifecycle::Running {return Err("source_discovery_busy")}
        self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().ok_or("source_service_closed")?;
        Ok(ManualAddLease(Arc::new(ManualAddReservation::acquire()?)))
    }

    /// 路径只来自原生目录选择结果，操作始终绑定选择器打开时的客户端。
    pub fn accept_manual_directory(self:&Arc<Self>,client:SourceClientKind,lease:ManualAddLease,selected:Option<PathBuf>)
        ->Result<tokio::sync::oneshot::Receiver<Result<ManualAddResult,&'static str>>,&'static str> {
        if client==SourceClientKind::WorkBuddy {return Err("source_client_unsupported")}
        let service=self.clone();let(tx,rx)=tokio::sync::oneshot::channel();
        self.background.spawn("manual-source-add",move|_|async move {let result=accept(service,client,lease,selected).await;let _=tx.send(result);})?;
        Ok(rx)
    }
}

/// 首次检查所有返回都释放选择预约；阻塞检查另有共享预约直至真实结束。
async fn accept(service:Arc<UsageService>,client:SourceClientKind,lease:ManualAddLease,selected:Option<PathBuf>)->Result<ManualAddResult,&'static str> {
    let Some(selected)=selected else{return Ok(ManualAddResult::Cancelled)};
    let coordinator=&service.discovery;
    let generation=service.discovery_batch.begin_discovery(coordinator,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::ManualSubtree,1)?;
    let inspected=match inspect_selected_path_owned(&service.background,coordinator.clone(),client,selected.clone(),lease.0.clone(),MANUAL_INSPECTION_TIMEOUT).await {
        ManualInspectionCompletion::Finished(Ok(bundle))=>bundle,
        ManualInspectionCompletion::Cancelled|ManualInspectionCompletion::DeadlineExceeded=>{finish_cancelled(coordinator);return Ok(ManualAddResult::Cancelled)},
        ManualInspectionCompletion::Finished(Err(_))=>{
            if coordinator.is_cancel_requested(){finish_cancelled(coordinator);return Ok(ManualAddResult::Cancelled)}
            coordinator.fail("manual_source_inspection_unreadable");return Err("manual_source_inspection_unreadable")
        },
        ManualInspectionCompletion::WorkerUnavailable=>{
            if coordinator.is_cancel_requested(){finish_cancelled(coordinator);return Ok(ManualAddResult::Cancelled)}
            coordinator.fail("manual_source_inspection_worker_unavailable");return Err("manual_source_inspection_worker_unavailable")
        },
    };
    if coordinator.is_cancel_requested() {finish_cancelled(coordinator);return Ok(ManualAddResult::Cancelled)}
    match decide_manual_source_add(inspected.decision_input) {
        ManualSourceAddDecision::AbortCancelled=>{finish_cancelled(coordinator);Ok(ManualAddResult::Cancelled)},
        ManualSourceAddDecision::Register=>{
            let path=inspected.found_path.ok_or("manual_source_inspection_unreadable")?;
            let candidate=SourceRootCandidate{root_id:stable_id(client.root_id_namespace(),&path_key(&path)),alias:source_root_alias_from_path(&path,client),path};
            let permit=service.context.writer.try_start().map_err(|_|{coordinator.fail("source_writer_busy");"source_writer_busy"})?;
            if coordinator.is_cancel_requested() {finish_cancelled(coordinator);return Ok(ManualAddResult::Cancelled)}
            let directory=source_client_app_data_dir(&service.context.directory,client);
            let mut catalog=SourceRootCatalogAdapter::new(directory,client).map_err(|_|"source_update_failed")?.with_cancellation(permit.cancellation_token());
            let result=source_root_add(&mut catalog,&candidate).await.map_err(|_|{coordinator.fail("source_update_failed");"source_update_failed"})?;
            if result.changed {service.context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner).entry(client).or_insert_with(empty_coverage).state=CoverageState::Partial;}
            drop(permit);
            let mut progress=coordinator.snapshot().progress;progress.volumes_completed=1;coordinator.update_progress(progress);coordinator.finish(false,false);
            // 手动已验证根无候选阶段，直接走统一近30日索引并等待真实收尾。
            let refresh=super::super::scan::run(service.context.clone(),super::super::queue::Intent{client:Some(client),origin:ScanStartOrigin::ExplicitUser},ScanCancellation::new()).await;
            Ok(ManualAddResult::Registered{outcome:result,refresh})
        },
        ManualSourceAddDecision::DeepSearch=>{
            service.discovery_batch.update(generation,|state|state.manual_inspecting=false);
            let deadline=Instant::now()+subtree::MANUAL_SUBTREE_DISCOVERY_TIMEOUT;
            let control=ManualSubtreeRunControl::new(coordinator.clone(),ScanCancellation::new());let monitor=control.clone();
            if let Err(error)=service.background.spawn("manual-subtree-deadline",move|shutdown|async move {
                supervise_manual_subtree_boundaries(monitor,deadline,move||shutdown.is_cancelled()).await;
            }) {control.fail_worker(deadline);return Err(error)}
            let cancel=control.clone();let worker=control.clone();
            if let Err(error)=service.background.spawn_blocking_cancelable("manual-subtree",move||cancel.cancel(),move|_|run_manual_subtree_discovery(worker,client,selected,deadline)) {
                control.fail_worker(deadline);return Err(error)
            }
            service.spawn_discovery_batch(client,generation)?;
            Ok(ManualAddResult::DeepSearchStarted)
        },
    }
}

/// 超时或取消首次检查不得报告已经完成所选卷。
fn finish_cancelled(coordinator:&RootDiscoveryCoordinator) {
    let _=coordinator.request_cancel();let mut progress=coordinator.snapshot().progress;progress.volumes_completed=0;coordinator.update_progress(progress);coordinator.finish(false,false);
}
