//! 发现换代回归制造真实的迟到回执，验证新批次候选与忙态不被旧任务污染。

use super::*;
use crate::usage_service::{SourceOperation,UsageQuery,UsageResponse};

/// coordinator 候选由固定元数据证据构造，不访问真实用户目录。
fn candidate(id:&str)->RootCandidate {
    RootCandidate{id:id.into(),client:SourceClientKind::Codex,absolute_path:"/synthetic/source".into(),
        strategy:RootDiscoveryStrategy::MetadataTraversal,evidence:RootCandidateEvidence::CodexRollout}
}

/// 旧提交迟到时必须既不能删新候选，也不能追加失败或清空新代索引。
#[test]
fn late_old_batch_cannot_mutate_new_candidate_or_indexing_projection() {
    let batch=Arc::new(DiscoveryBatch::default());let coordinator=Arc::new(RootDiscoveryCoordinator::default());
    let old=batch.begin_discovery(&coordinator,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::UserPriority,1).unwrap();
    coordinator.submit_candidate(candidate("same-id"));coordinator.finish(false,false);
    let barrier=Arc::new(std::sync::Barrier::new(2));
    let worker={let batch=batch.clone();let coordinator=coordinator.clone();let barrier=barrier.clone();std::thread::spawn(move|| {
        barrier.wait();
        assert!(!batch.update(old,|state| {state.failed.insert("old-failure".into());state.indexing.clear();coordinator.remove_candidates(&["same-id".into()]);}));
    })};
    let new=batch.begin_discovery(&coordinator,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::FullLocalVolumes,1).unwrap();
    coordinator.submit_candidate(candidate("same-id"));
    batch.update(new,|state|{state.indexing.insert(SourceClientKind::Codex);});
    barrier.wait();worker.join().unwrap();
    assert_eq!(coordinator.candidates(),vec![candidate("same-id")]);
    let state=batch.state.lock().unwrap();assert!(state.failed.is_empty());assert!(state.indexing.contains(&SourceClientKind::Codex));
}

/// 运行中重复启动失败，不得清空候选或推进宿主代数。
#[test]
fn rejected_start_preserves_existing_batch_identity_and_candidates() {
    let batch=DiscoveryBatch::default();let coordinator=RootDiscoveryCoordinator::default();
    let generation=batch.begin_discovery(&coordinator,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::UserPriority,1).unwrap();
    coordinator.submit_candidate(candidate("existing"));
    assert_eq!(batch.begin_discovery(&coordinator,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::FullLocalVolumes,1),Err("source_discovery_busy"));
    assert!(batch.current(generation));assert_eq!(coordinator.candidates(),vec![candidate("existing")]);
}

/// 每批走真实目录验证和 SQLite 登记，不启动用户目录发现或原生通知。
fn register_fixture_candidate(service:&UsageService,path:&std::path::Path,id:&str) {
    let generation=service.discovery_batch.begin_discovery(&service.discovery,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::UserPriority,1).unwrap();
    let mut value=candidate(id);value.absolute_path=path.to_string_lossy().into_owned();
    service.discovery.submit_candidate(value);service.discovery.finish(false,false);
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(register_candidate(
        &service.context,&service.background,&service.discovery,&service.discovery_batch,generation,id)).unwrap();
}

/// 同根再次发现仍推进扫描批次，但不能重建通知基线；新增来源才推进来源代。
#[test]
fn repeated_root_discovery_preserves_notification_epoch_and_ready_root() {
    let temp=tempfile::tempdir().unwrap();
    let source=crate::usage_service::source_tests::fixture_root(temp.path());
    let other=crate::usage_service::source_tests::fixture_root(&temp.path().join("other"));
    let service=UsageService::in_test_directory(temp.path().join("data")).unwrap();
    register_fixture_candidate(&service,&source,"first");
    assert_eq!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire),1);
    let id=stable_id(SourceClientKind::Codex.root_id_namespace(),&path_key(&source));
    service.mutate_source(SourceClientKind::Codex,SourceOperation::Reindex{root_id:id.clone()}).unwrap().blocking_recv().unwrap().unwrap();
    let epoch=service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire);
    let revision=service.scan_status().revision;
    register_fixture_candidate(&service,&source,"duplicate");
    assert_eq!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire),epoch);
    assert!(service.scan_status().revision>revision);
    assert!(service.discovery.candidates().is_empty());
    let rows=service.query(UsageQuery::Sources(SourceClientKind::Codex)).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(rows,UsageResponse::Sources(roots) if roots.len()==1&&roots[0].id==id&&roots[0].activation_state==RootActivationState::Ready));
    register_fixture_candidate(&service,&other,"new-source");
    assert_eq!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire),epoch+1);
    let rows=service.query(UsageQuery::Sources(SourceClientKind::Codex)).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(rows,UsageResponse::Sources(roots) if roots.len()==2));
    service.finish();
}
