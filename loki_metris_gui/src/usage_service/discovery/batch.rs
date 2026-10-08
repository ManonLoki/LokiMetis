//! 发现批次按代数和客户端拥有登记、重试及终态索引，旧回执不得修改新批次。

use super::*;
use std::collections::BTreeMap;
use tokio::sync::oneshot;

/// 所有批次投影在同一锁内换代，检查和修改之间不存在可被新批次插入的空隙。
#[derive(Default)]
pub(super) struct BatchState {
    pub generation:u64,
    pub failed:BTreeSet<String>,
    pub indexing:BTreeSet<SourceClientKind>,
    pub retrying:BTreeSet<String>,
    pub error:Option<&'static str>,
    pub(super) manual_inspecting:bool,
    pub(super) registrations_revision:BTreeMap<SourceClientKind,u64>,
    active:BTreeSet<SourceClientKind>,
    finalized:BTreeSet<SourceClientKind>,
    registrations:BTreeMap<String,SourceClientKind>,
    index_requests:BTreeMap<SourceClientKind,ScanCancellation>,
}

impl DiscoveryBatch {
    /// coordinator 清候选与宿主换代处于同一临界区，旧回执不能删除新发现候选。
    pub(in crate::usage_service) fn begin_discovery(&self,coordinator:&RootDiscoveryCoordinator,strategy:RootDiscoveryStrategy,scope:RootDiscoveryScope,volumes:u64)->Result<u64,&'static str> {
        let mut state=self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !coordinator.start(strategy,RootDiscoveryPlatform::current(),scope,volumes) {return Err("source_discovery_busy")}
        for token in state.index_requests.values() {token.cancel();}
        let generation=state.generation.wrapping_add(1);
        *state=BatchState{generation,manual_inspecting:scope==RootDiscoveryScope::ManualSubtree,..Default::default()};Ok(generation)
    }

    /// 只读身份检查用于每次 await 前后，实际投影写入仍由 update 再次核对。
    pub(super) fn current(&self,generation:u64)->bool {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).generation==generation
    }

    /// 有归属的原子投影更新；旧代数的闭包完全不执行。
    pub(super) fn update(&self,generation:u64,apply:impl FnOnce(&mut BatchState))->bool {
        let mut state=self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation!=generation {return false}
        apply(&mut state);true
    }

    /// 同一候选最多有一个登记任务，批次循环和手动重试共享实际执行归属。
    fn claim_registration(self:&Arc<Self>,generation:u64,id:&str,client:SourceClientKind,retry:bool)->Result<Registration,&'static str> {
        let mut state=self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation!=generation {return Err("source_candidate_stale")}
        if state.registrations.contains_key(id) {return Err("source_candidate_busy")}
        state.registrations.insert(id.to_owned(),client);
        if retry {state.retrying.insert(id.to_owned());}
        Ok(Registration{batch:self.clone(),generation,id:id.to_owned()})
    }
}

/// 登记真实 future 结束才释放去重身份；新批次同 ID 不受旧 guard 影响。
struct Registration {batch:Arc<DiscoveryBatch>,generation:u64,id:String}
impl Drop for Registration {
    /// 失败、取消及 unwind 都释放本代数拥有的候选槽。
    fn drop(&mut self) {
        self.batch.update(self.generation,|state|{state.registrations.remove(&self.id);state.retrying.remove(&self.id);});
    }
}

/// 批次任务拥有 active 与 indexing，退出时只清理自己的代数和客户端。
struct BatchLease {service:Arc<UsageService>,generation:u64,client:SourceClientKind,armed:bool}
impl Drop for BatchLease {
    /// runtime 取消与正常返回共用清理，旧任务不会清空新批次的忙态。
    fn drop(&mut self) {
        if self.armed {self.service.discovery_batch.update(self.generation,|state|{state.active.remove(&self.client);state.indexing.remove(&self.client);state.index_requests.remove(&self.client);});}
    }
}

impl UsageService {
    /// 页面挂载或切换 Agent 后接续当前客户端候选，不重复已完成的终态索引。
    pub fn resume_discovery(self:&Arc<Self>,client:SourceClientKind)->Result<(),&'static str> {
        if client==SourceClientKind::WorkBuddy {return Ok(())}
        let lifecycle=self.discovery.snapshot().lifecycle;
        if matches!(lifecycle,RootDiscoveryLifecycle::Idle|RootDiscoveryLifecycle::Failed) {return Ok(())}
        let state=self.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.manual_inspecting {return Ok(())}
        let generation=state.generation;
        let pending=self.discovery.candidates().iter().any(|candidate|candidate.client==client&&!state.failed.contains(&candidate.id));
        if state.active.contains(&client)||state.finalized.contains(&client)||(!pending&&lifecycle!=RootDiscoveryLifecycle::Running) {return Ok(())}
        drop(state);self.spawn_discovery_batch(client,generation)
    }

    /// 重试返回实际登记回执；成功后恢复终态批次，统一索引不会停留在待激活状态。
    pub fn retry_candidate(self:&Arc<Self>,id:String)->Result<oneshot::Receiver<Result<(),&'static str>>,&'static str> {
        let (candidate,generation)={let state=self.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let candidate=self.discovery.select_candidates(&[id.clone()]).map_err(|_|"source_candidate_invalid")?.into_iter().next().ok_or("source_candidate_invalid")?;
            (candidate,state.generation)
        };
        if candidate.client==SourceClientKind::WorkBuddy {return Err("source_candidate_invalid")}
        // 同步预约只用于拒绝重复点击，guard 在应用拥有的任务里持续到实际结果。
        let lease=self.discovery_batch.claim_registration(generation,&id,candidate.client,true)?;
        let service=self.clone();let(tx,rx)=oneshot::channel();
        self.background.spawn("source-candidate-retry",move|_|async move {
            let result=register_candidate(&service.context,&service.background,&service.discovery,&service.discovery_batch,generation,&id).await;
            service.discovery_batch.update(generation,|state| {
                if result.is_ok() {state.failed.remove(&id);state.finalized.remove(&candidate.client);}
                else {state.failed.insert(id.clone());}
            });drop(lease);
            let result=if result.is_ok()&&service.discovery_batch.current(generation) {service.spawn_discovery_batch(candidate.client,generation)}else{result};
            let _=tx.send(result);
        })?;
        Ok(rx)
    }

    /// 每客户端仅保留一个批次任务；登记期间不索引，终态等待全部登记真实收尾。
    pub(super) fn spawn_discovery_batch(self:&Arc<Self>,client:SourceClientKind,generation:u64)->Result<(),&'static str> {
        let mut state=self.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.generation!=generation {return Err("source_candidate_stale")}
        if state.active.contains(&client)||state.finalized.contains(&client) {return Ok(())}
        state.active.insert(client);drop(state);
        let lease=BatchLease{service:self.clone(),generation,client,armed:true};let service=self.clone();
        self.background.spawn("source-discovery-batch",move|shutdown|async move {
            let mut lease=lease;
            loop {
                if shutdown.is_cancelled()||!service.discovery_batch.current(generation) {return}
                for candidate in service.discovery.candidates().into_iter().filter(|candidate|candidate.client==client) {
                    {let state=service.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if state.generation!=generation {return}
                        if state.failed.contains(&candidate.id)||state.registrations.contains_key(&candidate.id)||state.retrying.contains(&candidate.id) {continue}}
                    let Ok(registration)=service.discovery_batch.claim_registration(generation,&candidate.id,client,false) else{continue};
                    let result=register_candidate(&service.context,&service.background,&service.discovery,&service.discovery_batch,generation,&candidate.id).await;
                    service.discovery_batch.update(generation,|state|{if result.is_err(){state.failed.insert(candidate.id.clone());}});
                    drop(registration);
                    if !service.discovery_batch.current(generation) {return}
                }
                if !service.discovery_batch.current(generation) {return}
                let lifecycle=service.discovery.snapshot().lifecycle;
                match discovery_batch_index_decision(DiscoveryBatchKind::ExplicitUser,lifecycle) {
                    DiscoveryBatchIndexDecision::Skip=>return,
                    DiscoveryBatchIndexDecision::Wait=>{},
                    DiscoveryBatchIndexDecision::Index=>{
                        let candidates=service.discovery.candidates();
                        let cancellation=ScanCancellation::new();
                        let watermark={
                            let mut state=service.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                            if state.generation!=generation {return}
                            let pending=state.registrations.values().any(|registered|*registered==client)
                                ||candidates.iter().any(|candidate|candidate.client==client&&!state.failed.contains(&candidate.id));
                            if !pending {
                                state.indexing.insert(client);state.index_requests.insert(client,cancellation.clone());
                                Some(state.registrations_revision.get(&client).copied().unwrap_or_default())
                            }else{None}
                        };
                        if let Some(watermark)=watermark {
                            if !service.discovery_batch.current(generation)||shutdown.is_cancelled() {return}
                            let result=super::super::scan::run(service.context.clone(),super::super::queue::Intent{client:Some(client),origin:ScanStartOrigin::ExplicitUser},cancellation).await;
                            let finished={
                                let mut state=service.discovery_batch.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                                if state.generation!=generation {return}
                                state.error=result.err();
                                if state.registrations_revision.get(&client).copied().unwrap_or_default()!=watermark {
                                    false
                                }else{
                                    // 完成、去重和释放 active 同步提交；旧 guard 不得清理后续重试新任务。
                                    state.finalized.insert(client);state.active.remove(&client);state.indexing.remove(&client);state.index_requests.remove(&client);
                                    lease.armed=false;true
                                }
                            };
                            if finished {return}
                        }
                    },
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    }
}
