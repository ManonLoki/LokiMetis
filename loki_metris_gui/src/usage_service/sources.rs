//! 数据目录显式写请求各自拥有任务和 writer 许可，不进入会覆盖 pending 的查询邮箱。

use super::{UsageService,ServiceContext};
use crate::backend::{source_catalog::SourceRootCatalogAdapter,scan_orchestration,local_index::{LocalIndex,ScanPermit}};
use loki_metris_core::*;
use std::sync::Arc;
use tokio::sync::oneshot;

/// 页面只提交稳定根身份；原始目录路径不由启停、改名或重建控件重复传入。
pub enum SourceOperation {
    /// 启用或停用一个数据根。
    SetEnabled {root_id:String,enabled:bool},
    /// 修改数据根展示别名。
    Rename {root_id:String,alias:String},
    /// 移除数据根登记，不删除原始文件。
    Remove {root_id:String},
    /// 设定主数据根；None 表示取消主数据根。
    SetPrimary {root_id:Option<String>},
    /// 重建一个数据根的索引。
    Reindex {root_id:String},
}

/// 已提交的变更结果保持独立，后续刷新失败不能把已发生的修改说成未执行。
pub enum SourceOperationResult {
    /// 数据根变更的结果。
    Mutation(SourceRootMutationOutcome),
    /// 重建索引已完成。
    Reindexed,
}

impl SourceOperation {
    /// ID 格式、别名值域及主目录能力全部通过 core 校验，数据库尚未打开。
    fn validate(&mut self,client:SourceClientKind)->Result<(),&'static str> {
        if client==SourceClientKind::WorkBuddy {return Err("source_client_unsupported")}
        let root_id=match self {
            Self::SetEnabled{root_id,..}|Self::Rename{root_id,..}|Self::Remove{root_id}|Self::Reindex{root_id}=>Some(root_id.as_str()),
            Self::SetPrimary{root_id}=>{ensure_primary_source_root_supported(client).map_err(|_|"source_client_unsupported")?;root_id.as_deref()},
        };
        if let Some(root_id)=root_id {validate_source_root_id(client,root_id).map_err(|_|"source_id_invalid")?;}
        if let Self::Rename{alias,..}=self {*alias=normalize_source_root_alias(alias).map_err(|_|"source_alias_invalid")?;}
        Ok(())
    }
}

impl UsageService {
    /// 接受的每个显式变更均保留独立回执；busy 立即拒绝，绝不静默丢弃第二个请求。
    pub fn mutate_source(&self,client:SourceClientKind,mut operation:SourceOperation)
        ->Result<oneshot::Receiver<Result<SourceOperationResult,&'static str>>,&'static str> {
        operation.validate(client)?;
        let runtime=self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime=runtime.as_ref().ok_or("source_service_closed")?;
        let permit=self.context.writer.try_start().map_err(|_|"source_writer_busy")?;
        let context=self.context.clone();let owner=self.background.clone();let(sender,receiver)=oneshot::channel();
        self.background.spawn("source-operation",move|_|async move {
            let result=apply(context,owner,client,operation,permit).await;
            let _=sender.send(result);
        })?;
        let _=runtime;
        Ok(receiver)
    }
}

/// 许可令牌贯穿文件系统验证和 SQLite 收尾；丢弃页面回执不会取消已接受的写任务。
async fn apply(context:Arc<ServiceContext>,owner:Arc<super::background::BackgroundTaskOwner>,client:SourceClientKind,operation:SourceOperation,permit:ScanPermit)
    ->Result<SourceOperationResult,&'static str> {
    let cancellation=permit.cancellation_token();
    let _finish=Finish(context.clone());
    {
        let mut state=context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.running=true;state.client=Some(client);state.progress=None;state.error=None;
    }
    if cancellation.is_cancelled() {return Err("source_operation_cancelled")}
    let directory=source_client_app_data_dir(&context.directory,client);
    let mut catalog=SourceRootCatalogAdapter::new(directory.clone(),client).map_err(|_|"source_update_failed")?.with_cancellation(cancellation.clone());
    let resets_notification=!matches!(&operation,SourceOperation::Rename{..}|SourceOperation::SetPrimary{..});
    let outcome=match operation {
        SourceOperation::SetEnabled{root_id,enabled}=>source_root_set_enabled(&mut catalog,&root_id,enabled).await,
        SourceOperation::Rename{root_id,alias}=>source_root_rename(&mut catalog,&root_id,&alias).await,
        SourceOperation::Remove{root_id}=>source_root_remove(&mut catalog,&root_id).await,
        SourceOperation::SetPrimary{root_id}=>{
            if let Some(root_id)=root_id.as_deref() {validate_primary(&directory,root_id,cancellation.clone(),&owner).await?;}
            if cancellation.is_cancelled() {return Err("source_operation_cancelled")}
            source_root_set_primary(&mut catalog,root_id.as_deref()).await
        }
        SourceOperation::Reindex{root_id}=>{
            let request=source_root_reindex_request(&catalog,&root_id).await.map_err(|_|"source_update_failed")?;
            let retention=context.settings.snapshot().await?.retention_days;
            let previous_coverage={
                let mut reports=context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let previous=reports.get(&client).cloned().unwrap_or_else(empty_coverage);
                reports.entry(client).or_insert_with(empty_coverage).state=CoverageState::Partial;
                previous
            };
            let progress_context=context.clone();
            let output=scan_orchestration::reindex(directory,client,retention,request,cancellation,Box::new(move|progress| {
                if let LocalScanProgress::Indexing{current_root_id,roots_completed,roots_total,files_scanned,calls_added,..}=progress {
                    progress_context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).progress=Some(crate::backend::local_index::ScanProgress {
                        scan_id:String::new(),current_root_id:current_root_id.unwrap_or_default(),roots_completed,roots_total,files_scanned,calls_added,warning_count:0});
                }
            })).await.map_err(|_|if permit.cancellation_token().is_cancelled(){"source_operation_cancelled"}else{"source_reindex_failed"})?;
            context.notification_source_epoch.fetch_add(1,std::sync::atomic::Ordering::Release);
            let cancelled=output.coverage.state==CoverageState::Cancelled;
            let mut reports=context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let coverage=extend_coverage_with_partial_scan(previous_coverage,&output.coverage);
            reports.insert(client,coverage);
            if cancelled {return Err("source_operation_cancelled")}
            return Ok(SourceOperationResult::Reindexed);
        }
    }.map_err(|_|if cancellation.is_cancelled(){"source_operation_cancelled"}else{"source_update_failed"})?;
    if outcome.changed {
        if resets_notification {context.notification_source_epoch.fetch_add(1,std::sync::atomic::Ordering::Release);}
        let mut reports=context.coverage.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reports.entry(client).or_insert_with(empty_coverage).state=CoverageState::Partial;
    }
    Ok(SourceOperationResult::Mutation(outcome))
}

/// WorkBuddy 固定来源只投影结构证据，不创建登记库或读取统计记录正文。
pub(super) fn workbuddy_sources()->Vec<SourceRootSummary> {
    use crate::backend::workbuddy::{self,WorkBuddyReadError};
    workbuddy::inspect_workbuddy_project_sources(&workbuddy::discover_workbuddy_sources()).into_iter().map(|inspection| {
        let (file_count,skipped_count,error_count)=match inspection.evidence {
            Ok(evidence)=>(evidence.file_count,evidence.skipped_count.saturating_add(u64::from(evidence.budget_exhausted)),evidence.permission_denied_count),
            Err(WorkBuddyReadError::Read)=>(0,0,1),Err(WorkBuddyReadError::SourceUnavailable)=>(0,0,0),
        };
        let activation_state=if file_count>0&&skipped_count==0&&error_count==0 {RootActivationState::Ready}
            else if skipped_count>0||error_count>0 {RootActivationState::ValidationFailed}else{RootActivationState::ConfirmedUnindexed};
        SourceRootSummary {id:inspection.source.root_id,alias:inspection.source.alias,enabled:true,activation_state,is_primary:false,
            discovery_label:String::new(),discovery_code:SourceDiscoveryCode::DefaultRoot,file_count,skipped_count,error_count,duplicate_count:0}
    }).collect()
}

/// 主目录提交前只重验证精确登记的 Codex 根，禁用、失效和未确认候选均拒绝。
async fn validate_primary(directory:&std::path::Path,root_id:&str,cancellation:ScanCancellation,owner:&Arc<super::background::BackgroundTaskOwner>)->Result<(),&'static str> {
    if cancellation.is_cancelled() {return Err("source_operation_cancelled")}
    let index=LocalIndex::open_read_only_in_app_data(directory,SourceClientKind::Codex.parser_version()).await.map_err(|_|"source_update_failed")?;
    let root=index.all_roots().await.map_err(|_|"source_update_failed")?.into_iter()
        .find(|root|root.root_id.as_deref()==Some(root_id)&&root.enabled).ok_or("source_primary_invalid")?;
    if cancellation.is_cancelled() {return Err("source_operation_cancelled")}
    let (tx,rx)=oneshot::channel();let token=cancellation.clone();let cancel=token.clone();
    owner.spawn_blocking_cancelable("source-primary-validation",move||cancel.cancel(),move|_| {
        let _=tx.send(crate::backend::local_index::discover_quick(&crate::backend::local_index::DiscoveryInputs{home_dir:None,codex_home:None,registered_roots:vec![root]},&token));
    })?;
    let discovered=rx.await.map_err(|_|"source_primary_invalid")?;
    if cancellation.is_cancelled() {return Err("source_operation_cancelled")}
    extract_single_verified_source_root(&discovered.roots,|root|root.root_id.as_str(),
        &discovered.confirmed_invalid_root_ids,&discovered.unconfirmed_root_ids,Some(root_id)).map_err(|_|"source_primary_invalid")?;
    Ok(())
}

/// 收尾代数在 SQLite future 完成后递增，写锁仍持有到此 guard 析构。
struct Finish(Arc<ServiceContext>);
impl Drop for Finish {
    /// 失败和取消同样释放展示忙态，panic 只保存固定类别。
    fn drop(&mut self) {
        self.0.today_refresh.notify_one();
        let mut state=self.0.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if std::thread::panicking() {state.error=Some("usage_worker_failed");}
        state.running=false;state.revision=state.revision.wrapping_add(1);
    }
}
