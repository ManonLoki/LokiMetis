//! 启动清理接线真实打开隔离三库，验证边界、writer 等待及取消，不扫描用户目录。

use super::*;
use loki_metris_core::{UsageCall,SessionTokenSnapshot,TokenUsage,Confidence,DiscoveryMethod,SourceClientKind,SourceParseCheckpoint};

/// 创建同一截止时刻前、等于、后各一条规范调用；checkpoint 和原始文件用于不变性断言。
async fn fixture(directory:&std::path::Path,client:SourceClientKind,cutoff:i64)->(LocalIndex,Vec<loki_metris_core::RegisteredRoot>,Option<loki_metris_core::StoredSourceFile>) {
    let directory=source_client_app_data_dir(directory,client);
    let mut index=LocalIndex::open_in_app_data(&directory,client.parser_version()).await.unwrap();
    let root=directory.join("fixture-root");std::fs::create_dir_all(&root).unwrap();std::fs::write(root.join("raw.txt"),b"preserve raw").unwrap();
    index.register_confirmed_root_fields("root-retention",&root,"Retention",DiscoveryMethod::Registered).await.unwrap();
    let ids=vec!["root-retention".to_owned()];index.claim_discovered_roots_for_current_scan(&ids).await.unwrap();
    let checkpoint=SourceParseCheckpoint{thread_key:"thread".into(),project_key:None,project_label:None,thread_label:None,model:None,reasoning_effort:None,call_sequence:3,adapter_state:None};
    index.commit_source_file_checkpoint("source-retention","root-retention","fixture.jsonl","fixture-file",false,10,cutoff+1,10,0,false,client.parser_version(),1,&checkpoint).await.unwrap();
    let mut calls=Vec::new();let mut snapshots=Vec::new();
    for (id,time) in [("old",cutoff-1),("boundary",cutoff),("new",cutoff+1)] {
        calls.push(UsageCall{logical_call_id:id.into(),occurred_at_epoch_ms:time,model:None,reasoning_effort:None,project_key:None,thread_key:"thread".into(),project_label:None,thread_label:None,usage:TokenUsage::new_with_availability(1,Some(0),None,0,Some(0),Some(1)).unwrap(),adapter_consistency_key:None,confidence:Confidence::Exact,provenance:vec![]});
        snapshots.push(SessionTokenSnapshot{thread_key:"thread".into(),occurred_at_epoch_ms:time,cumulative_total_tokens:1,logical_call_id:id.into(),model:None,reasoning_effort:None,project_key:None,provenance:vec![]});
    }
    index.insert_usage_and_snapshots("source-retention",1,&mut calls,&mut snapshots).await.unwrap();index.finish_background_index_roots(&ids).await.unwrap();
    let roots=index.all_roots().await.unwrap();let checkpoint=index.stored_source_file("source-retention").await.unwrap();(index,roots,checkpoint)
}

/// 三个物理库均按 Local cutoff 清理，截止时刻保留，原始文件和来源 checkpoint 不变。
#[test]
fn startup_cleanup_prunes_three_derived_indexes_and_preserves_sources() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");let service=UsageService::in_directory(directory.clone()).unwrap();
    let now=1_790_000_000_000;let cutoff=retention_cutoff_epoch_ms(loki_metris_core::RetentionDays::default(),now,&TimeStandard::Local,&jiff::tz::TimeZone::system()).unwrap();
    let mut indexes=service.runtime.lock().unwrap().as_ref().unwrap().block_on(async {
        let mut indexes=Vec::new();for client in super::super::super::query::physical_clients() {indexes.push((client,fixture(&directory,client,cutoff).await));}indexes
    });
    let context=service.context.clone();let(tx,rx)=oneshot::channel();
    service.background.spawn("fixture-cleanup",move|mut shutdown|async move{let _=tx.send(startup_cleanup_at(context,&mut shutdown,now).await);}).unwrap();
    rx.blocking_recv().unwrap().unwrap();
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(async {
        for (client,(index,roots,checkpoint)) in &mut indexes {
            let snapshot=index.usage_snapshot().await.unwrap();assert_eq!(snapshot.canonical.calls.len(),2);assert_eq!(snapshot.canonical.snapshots.len(),2);
            assert!(snapshot.canonical.calls.iter().all(|call|call.occurred_at_epoch_ms>=cutoff));
            assert_eq!(index.all_roots().await.unwrap(),*roots);assert_eq!(index.stored_source_file("source-retention").await.unwrap(),*checkpoint);
            assert_eq!(std::fs::read(source_client_app_data_dir(&directory,*client).join("fixture-root/raw.txt")).unwrap(),b"preserve raw");
        }
    });
    assert!(!source_client_app_data_dir(&directory,SourceClientKind::WorkBuddy).exists());service.finish();
}

/// cleanup 等待同一 writer；关闭时合作结束，不创建尚不存在的索引目录。
#[test]
fn startup_cleanup_waiting_on_writer_can_shutdown_without_creating_indexes() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");let service=UsageService::in_directory(directory.clone()).unwrap();
    let permit=service.context.writer.try_start().unwrap();let context=service.context.clone();let(tx,mut rx)=oneshot::channel();
    service.background.spawn("fixture-cleanup",move|mut shutdown|async move{let _=tx.send(startup_cleanup(context,&mut shutdown).await);}).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));assert!(matches!(rx.try_recv(),Err(oneshot::error::TryRecvError::Empty)));assert!(!directory.exists());
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(service.background.shutdown());assert_eq!(rx.blocking_recv().unwrap(),Ok(()));
    assert!(!directory.exists());drop(permit);service.finish();
}
