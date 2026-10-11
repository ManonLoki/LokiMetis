//! 发现换代回归制造真实的迟到回执，验证新批次候选与忙态不被旧任务污染。

use super::*;

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
