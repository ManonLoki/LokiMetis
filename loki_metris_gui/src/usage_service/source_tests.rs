//! 数据目录回归只读写隔离临时树，不修改真实 HOME、参考 App 或 Agent 原始记录。

use super::*;
use crate::backend::local_index::{self,LocalIndex,DiscoveryInputs,DiscoveryMethod};
use loki_metris_core::*;

/// 合成来源仅含固定、无敏感字段的 Codex 签名；不依赖测试当天是否仍保留其调用。
pub(super) fn fixture_root(directory:&std::path::Path)->std::path::PathBuf {
    let root=directory.join("source");std::fs::create_dir_all(root.join("sessions")).unwrap();
    let bytes=concat!("{\"timestamp\":\"2026-10-09T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"source-test-session\"}}\n",
        "{\"timestamp\":\"2026-10-09T00:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":100,\"cached_input_tokens\":0,\"output_tokens\":10,\"total_tokens\":110}}}}\n");
    std::fs::write(root.join("sessions/rollout-source.jsonl"),bytes).unwrap();root
}

/// 通过生产签名发现生成真实稳定 ID，再登记为待首次索引状态。
pub(super) fn register_fixture(service:&UsageService,root:&std::path::Path)->String {
    let discovered=local_index::discover_quick(&DiscoveryInputs{home_dir:None,codex_home:Some(root.to_path_buf()),registered_roots:Vec::new()},&ScanCancellation::new());
    let id=discovered.roots.first().expect("合成签名可被发现").root_id.clone();
    let runtime=service.runtime.lock().unwrap();
    runtime.as_ref().unwrap().block_on(async {
        let directory=source_client_app_data_dir(&service.context.directory,SourceClientKind::Codex);
        let mut index=LocalIndex::open_in_app_data(&directory,SourceClientKind::Codex.parser_version()).await.unwrap();
        index.register_confirmed_root_fields(&id,root,"测试目录",DiscoveryMethod::Registered).await.unwrap();
    });id
}

/// 新安装 Sources 查询零写入；扫描占写槽时显式变更明确 busy，WorkBuddy 在开库前拒绝。
#[test]
fn sources_read_missing_and_mutation_busy_do_not_create_storage() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("not-created");
    let service=UsageService::in_directory(directory.clone()).unwrap();
    let result=service.query(UsageQuery::Sources(SourceClientKind::Codex)).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(result,UsageResponse::Sources(roots) if roots.is_empty()));assert!(!directory.exists());
    let permit=service.context.writer.try_start().unwrap();
    assert!(matches!(service.mutate_source(SourceClientKind::Codex,SourceOperation::SetEnabled{root_id:"root-0123456789abcdef".into(),enabled:false}),Err("source_writer_busy")));
    assert!(matches!(service.mutate_source(SourceClientKind::WorkBuddy,SourceOperation::Remove{root_id:"ignored".into()}),Err("source_client_unsupported")));
    assert!(matches!(service.mutate_source(SourceClientKind::ClaudeCode,SourceOperation::SetPrimary{root_id:None}),Err("source_client_unsupported")));
    assert!(!directory.exists());drop(permit);service.finish();
}

/// 接受的改名和启停各自返回真实结果，未打开的页面回执不会影响索引提交。
#[test]
fn source_mutations_commit_independently_and_mark_coverage_partial() {
    let temp=tempfile::tempdir().unwrap();let source=fixture_root(temp.path());
    let service=UsageService::in_directory(temp.path().join("data")).unwrap();let id=register_fixture(&service,&source);
    let result=service.mutate_source(SourceClientKind::Codex,SourceOperation::Rename{root_id:id.clone(),alias:"  新别名  ".into()}).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(result,SourceOperationResult::Mutation(outcome) if outcome.changed));
    let result=service.mutate_source(SourceClientKind::Codex,SourceOperation::SetEnabled{root_id:id.clone(),enabled:false}).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(result,SourceOperationResult::Mutation(outcome) if outcome.changed));
    let rows=match service.query(UsageQuery::Sources(SourceClientKind::Codex)).unwrap().blocking_recv().unwrap().unwrap() {
        UsageResponse::Sources(rows)=>rows,_=>panic!("返回来源摘要"),
    };
    assert_eq!(rows.len(),1);assert_eq!(rows[0].alias,"新别名");assert!(!rows[0].enabled);
    assert_eq!(service.context.coverage.lock().unwrap()[&SourceClientKind::Codex].state,CoverageState::Partial);
    assert_eq!(service.scan_status().revision,2);assert!(!service.scan_status().running);
    assert!(source.join("sessions/rollout-source.jsonl").is_file());service.finish();
}

/// 单根强制重建会完成待索引根的激活，取消重试保留先前来源代和原始字节。
#[test]
fn reindex_activates_exact_target_and_cancel_preserves_previous_generation() {
    let temp=tempfile::tempdir().unwrap();let source=fixture_root(temp.path());let file=source.join("sessions/rollout-source.jsonl");
    let bytes=std::fs::read(&file).unwrap();
    let service=UsageService::in_directory(temp.path().join("data")).unwrap();let id=register_fixture(&service,&source);
    let outcome=service.mutate_source(SourceClientKind::Codex,SourceOperation::Reindex{root_id:id.clone()}).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(outcome,SourceOperationResult::Reindexed));
    let rows=service.query(UsageQuery::Sources(SourceClientKind::Codex)).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(rows,UsageResponse::Sources(roots) if roots.iter().any(|root|root.id==id&&root.activation_state==RootActivationState::Ready)));
    // 无先前 coverage 缓存的单根重建不能把整个客户端未知范围报告成完整。
    assert_eq!(service.context.coverage.lock().unwrap()[&SourceClientKind::Codex].state,CoverageState::Partial);
    {
        let runtime=service.runtime.lock().unwrap();runtime.as_ref().unwrap().block_on(async {
            let directory=source_client_app_data_dir(&service.context.directory,SourceClientKind::Codex);
            let catalog=crate::backend::source_catalog::SourceRootCatalogAdapter::new(directory.clone(),SourceClientKind::Codex).unwrap();
            let request=source_root_reindex_request(&catalog,&id).await.unwrap();
            let token=ScanCancellation::new();token.cancel();
            let result=crate::backend::scan_orchestration::reindex(directory.clone(),SourceClientKind::Codex,RetentionDays::default(),request,token,Box::new(|_|{})).await;
            assert!(result.is_err());
            let index=LocalIndex::open_read_only_in_app_data(&directory,SourceClientKind::Codex.parser_version()).await.unwrap();
            let rows=index.root_usage_summary_records().await.unwrap();
            assert_eq!(rows.len(),1);assert!(build_indexed_source_roots(&rows,"test")[0].file_count>0);
        });
    }
    assert_eq!(std::fs::read(file).unwrap(),bytes);service.finish();
}

/// 原生选择器取消释放全进程预约，且不开始发现、不创建任何派生库。
#[test]
fn cancelled_manual_picker_releases_reservation_without_storage() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("never-created");
    let service=UsageService::in_directory(directory.clone()).unwrap();
    assert!(service.begin_manual_add(SourceClientKind::WorkBuddy).is_err());
    let lease=service.begin_manual_add(SourceClientKind::Codex).unwrap();
    assert!(service.begin_manual_add(SourceClientKind::ClaudeCode).is_err());
    let result=service.accept_manual_directory(SourceClientKind::Codex,lease,None).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(result,ManualAddResult::Cancelled));assert!(!directory.exists());
    assert_eq!(service.discovery_view().status.lifecycle,RootDiscoveryLifecycle::Idle);
    drop(service.begin_manual_add(SourceClientKind::ClaudeCode).unwrap());service.finish();
}

/// writer 取消令牌在 catalog 开库之前生效，拒绝写入不会悄悄初始化数据库。
#[test]
fn cancelled_catalog_write_does_not_open_or_initialize_database() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("never-created");
    let runtime=tokio::runtime::Runtime::new().unwrap();runtime.block_on(async {
        let token=ScanCancellation::new();token.cancel();
        let mut catalog=crate::backend::source_catalog::SourceRootCatalogAdapter::new(directory.clone(),SourceClientKind::Codex).unwrap().with_cancellation(token);
        assert!(catalog.set_root_alias("root-0123456789abcdef","取消测试").await.is_err());
    });assert!(!directory.exists());
}

/// 切换到另一个 Agent 接续真实候选登记，Running 阶段不提前读取 Token 内容。
#[test]
fn resuming_other_client_registers_its_candidates_without_early_indexing() {
    let temp=tempfile::tempdir().unwrap();let source=fixture_root(temp.path());
    let service=UsageService::in_directory(temp.path().join("data")).unwrap();
    service.discovery_batch.begin_discovery(&service.discovery,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::UserPriority,1).unwrap();
    service.discovery.submit_candidate(RootCandidate{id:"codex-candidate".into(),client:SourceClientKind::Codex,
        absolute_path:source.to_string_lossy().into_owned(),strategy:RootDiscoveryStrategy::MetadataTraversal,evidence:RootCandidateEvidence::CodexRollout});
    let other=temp.path().join("other-agent");std::fs::create_dir(&other).unwrap();
    service.discovery.submit_candidate(RootCandidate{id:"grok-candidate".into(),client:SourceClientKind::GrokBuildCli,
        absolute_path:other.to_string_lossy().into_owned(),strategy:RootDiscoveryStrategy::MetadataTraversal,evidence:RootCandidateEvidence::GrokSessionUpdates});
    service.resume_discovery(SourceClientKind::GrokBuildCli).unwrap();
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    while service.discovery.candidates().iter().any(|candidate|candidate.id=="grok-candidate") {
        assert!(std::time::Instant::now()<deadline,"另一个客户端候选应被接续登记");std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(service.discovery.candidates().len(),1);assert_eq!(service.discovery.candidates()[0].client,SourceClientKind::Codex);
    let rows=service.query(UsageQuery::Sources(SourceClientKind::GrokBuildCli)).unwrap().blocking_recv().unwrap().unwrap();
    assert!(matches!(rows,UsageResponse::Sources(roots) if roots.len()==1&&roots[0].activation_state==RootActivationState::ConfirmedUnindexed&&roots[0].file_count==0));
    assert!(!service.scan_status().running);assert!(service.discovery_view().indexing.is_empty());service.finish();
}

/// Busy 重试返回实际失败且保留候选，不能把任务已接受误作登记完成。
#[test]
fn candidate_retry_reports_writer_busy_and_retains_candidate() {
    let temp=tempfile::tempdir().unwrap();let source=fixture_root(temp.path());
    let service=UsageService::in_directory(temp.path().join("data")).unwrap();
    service.discovery_batch.begin_discovery(&service.discovery,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::UserPriority,1).unwrap();
    service.discovery.submit_candidate(RootCandidate{id:"retry-me".into(),client:SourceClientKind::Codex,
        absolute_path:source.to_string_lossy().into_owned(),strategy:RootDiscoveryStrategy::MetadataTraversal,evidence:RootCandidateEvidence::CodexRollout});
    service.discovery.finish(false,false);let permit=service.context.writer.try_start().unwrap();
    assert_eq!(service.retry_candidate("retry-me".into()).unwrap().blocking_recv().unwrap(),Err("source_writer_busy"));
    assert_eq!(service.discovery.candidates().len(),1);assert!(service.discovery_view().failed_ids.contains("retry-me"));
    assert!(service.discovery_view().retrying_ids.is_empty());assert!(!service.context.directory.exists());drop(permit);service.finish();
}

/// 手动首次检查由选择流程独占，重进页面或切 Agent 不派生第二个索引批次。
#[test]
fn manual_inspection_does_not_resume_a_second_batch_on_page_remount() {
    let temp=tempfile::tempdir().unwrap();let service=UsageService::in_directory(temp.path().join("data")).unwrap();
    service.discovery_batch.begin_discovery(&service.discovery,RootDiscoveryStrategy::MetadataTraversal,RootDiscoveryScope::ManualSubtree,1).unwrap();
    let owned=service.background.owned_task_count();
    service.resume_discovery(SourceClientKind::Codex).unwrap();service.resume_discovery(SourceClientKind::GrokBuildCli).unwrap();
    assert_eq!(service.background.owned_task_count(),owned);assert!(!service.context.directory.exists());service.finish();
}

/// 今日摘要真实复用SQLite All聚合；页面查询取消令牌不拥有应用摘要，Partial不发送。
#[test]
fn notification_today_real_index_all_matches_and_sources_reset() {
    let temp=tempfile::tempdir().unwrap();let source=fixture_root(temp.path());
    let service=UsageService::in_directory(temp.path().join("data")).unwrap();let id=register_fixture(&service,&source);
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    service.mutate_source(SourceClientKind::Codex,SourceOperation::Reindex{root_id:id.clone()}).unwrap().blocking_recv().unwrap().unwrap();
    let now="2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let (data,first)={let runtime=service.runtime.lock().unwrap();runtime.as_ref().unwrap().block_on(today::load(service.context.clone(),now,jiff::tz::TimeZone::UTC,ScanCancellation::new())).unwrap()};
    assert_eq!(first.total_tokens,110);assert_eq!(data.records.windows.iter().find(|w|w.window==LocalUsageWindow::Today).unwrap().fact.value.tokens.total_tokens,110);
    assert!(!first.eligible(now,&first.day,&first.source));
    let initial=service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire);
    service.mutate_source(SourceClientKind::Codex,SourceOperation::Rename{root_id:id.clone(),alias:"新别名".into()}).unwrap().blocking_recv().unwrap().unwrap();
    assert_eq!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire),initial);
    let second={let runtime=service.runtime.lock().unwrap();runtime.as_ref().unwrap().block_on(today::load(service.context.clone(),now,jiff::tz::TimeZone::UTC,ScanCancellation::new())).unwrap().1};
    assert_eq!(first.source,second.source);
    service.save_enabled_ai_tools(vec![AiTool::Codex,AiTool::ClaudeCode]).unwrap().blocking_recv().unwrap().unwrap();
    assert!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire)>initial);
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    service.mutate_source(SourceClientKind::Codex,SourceOperation::SetEnabled{root_id:id,enabled:false}).unwrap().blocking_recv().unwrap().unwrap();
    assert!(service.context.notification_source_epoch.load(std::sync::atomic::Ordering::Acquire)>initial);
    let disabled={let runtime=service.runtime.lock().unwrap();runtime.as_ref().unwrap().block_on(today::load(service.context.clone(),now,jiff::tz::TimeZone::UTC,ScanCancellation::new()))};
    assert!(disabled.is_err()||disabled.is_ok_and(|(_,v)|!v.eligible(now,&v.day,&v.source)));service.finish();
}

/// 新总量与旧 Complete 缓存并存时，数据库最新扫描事实仍必须阻止通知资格。
#[test]
fn notification_today_rejects_in_progress_failed_and_partial_index_writes() {
    use local_index::{ScanConfig, scan_discovered_roots};

    /// 使用真实 Codex last 用量事件，两个不同增量不会互相去重。
    fn token_line(input: u64, minute: u8) -> String {
        serde_json::json!({
            "timestamp":format!("2026-10-09T00:{minute:02}:00Z"), "type":"event_msg",
            "payload":{"type":"token_count","info":{"last_token_usage":{
                "input_tokens":input,"cached_input_tokens":0,"output_tokens":0,"total_tokens":input
            }}}
        }).to_string() + "\n"
    }
    /// 摘要始终从独立只读连接取得，不能复用测试 writer 的内存状态。
    async fn read_today(context: Arc<ServiceContext>, now: i64) -> TodayUsageSnapshot {
        today::load(context, now, jiff::tz::TimeZone::UTC, ScanCancellation::new()).await.unwrap().1
    }
    let temp = tempfile::tempdir().unwrap();
    let source = fixture_root(temp.path());
    let file = source.join("sessions/rollout-source.jsonl");
    let metadata = std::fs::read_to_string(&file).unwrap().lines().next().unwrap().to_owned();
    std::fs::write(&file, metadata + "\n" + &token_line(90_000_000, 1)).unwrap();
    let service = UsageService::in_directory(temp.path().join("data")).unwrap();
    let root_id = register_fixture(&service, &source);
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    let now = "2026-10-09T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
    {
        let runtime = service.runtime.lock().unwrap();
        runtime.as_ref().unwrap().block_on(async {
            let _permit = service.context.writer.try_start().unwrap();
            let directory = source_client_app_data_dir(&service.context.directory, SourceClientKind::Codex);
            let mut index = LocalIndex::open_in_app_data(&directory, SourceClientKind::Codex.parser_version()).await.unwrap();
            let roots = local_index::discover_quick(&DiscoveryInputs {home_dir:None, codex_home:Some(source.clone()), registered_roots:Vec::new()}, &ScanCancellation::new()).roots;
            let baseline = scan_discovered_roots(&mut index, &roots, ScanConfig::default(), &ScanCancellation::new(), |_| {}).await.unwrap();
            assert_eq!(baseline.coverage.state, CoverageState::Complete);
            service.context.coverage.lock().unwrap().insert(SourceClientKind::Codex, baseline.coverage);
            let first = read_today(service.context.clone(), now).await;
            assert_eq!(first.total_tokens, 90_000_000);
            assert!(first.eligible(now, &first.day, &first.source));

            let running = index.begin_scan("quick", now).await.unwrap();
            let mut call = index.canonical_calls().await.unwrap().calls[0].clone();
            let source_id = call.provenance[0].source_id.clone();
            let generation = index.stored_source_file(&source_id).await.unwrap().unwrap().generation;
            call.logical_call_id = "notification-readiness-appended-call".into();
            call.occurred_at_epoch_ms += 60_000;
            call.usage = TokenUsage::new(20_000_000, 0, None, 0, 0, Some(20_000_000)).unwrap();
            // 生产解析器同样在 checkpoint/finish_scan 之前逐批提交当前 generation。
            assert_eq!(index.insert_usage_batch(&source_id, generation, &mut vec![call]).await.unwrap(), 1);
            let in_progress = read_today(service.context.clone(), now).await;
            assert_eq!(in_progress.total_tokens, 110_000_000);
            assert!(!in_progress.eligible(now, &in_progress.day, &in_progress.source));
            index.finish_scan(&running, now + 1, "failed", false, 1, 1, 0).await.unwrap();
            let failed = read_today(service.context.clone(), now).await;
            assert!(!failed.eligible(now, &failed.day, &failed.source));
            for (cancelled, warning_count) in [(true, 0), (false, 1)] {
                let run = index.begin_scan("quick", now).await.unwrap();
                index.finish_scan(&run, now + 1, "completed", cancelled, 1, 0, warning_count).await.unwrap();
                let rejected = read_today(service.context.clone(), now).await;
                assert!(!rejected.eligible(now, &rejected.day, &rejected.source));
            }
            let successful = index.begin_scan("quick", now).await.unwrap();
            index.finish_scan(&successful, now + 1, "completed", false, 1, 0, 0).await.unwrap();
            index.record_roots_coverage(std::slice::from_ref(&root_id), CoverageState::Partial).await.unwrap();
            let partial = read_today(service.context.clone(), now).await;
            assert!(!partial.eligible(now, &partial.day, &partial.source));
            index.record_roots_coverage(&[root_id], CoverageState::Complete).await.unwrap();
            let recovered = read_today(service.context.clone(), now).await;
            assert_eq!(recovered.total_tokens, 110_000_000);
            assert_eq!(recovered.source, first.source);
            assert!(recovered.eligible(now, &recovered.day, &recovered.source));
        });
    }
    service.finish();
}

/// 外层扫描在 begin_scan 前失败，也不能留下先前 Complete 的通知资格。
#[test]
fn failed_scan_invalidates_cached_coverage_before_opening_index() {
    let temp = tempfile::tempdir().unwrap();
    let service = UsageService::in_directory(temp.path().join("data")).unwrap();
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    let directory = source_client_app_data_dir(&service.context.directory, SourceClientKind::Codex);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(USAGE_INDEX_FILE_NAME), b"invalid sqlite fixture").unwrap();
    let mut old = empty_coverage();
    old.state = CoverageState::Complete;
    service.context.coverage.lock().unwrap().insert(SourceClientKind::Codex, old);
    {
        let runtime = service.runtime.lock().unwrap();
        let result = runtime.as_ref().unwrap().block_on(scan::run(service.context.clone(),
            Intent {client:Some(SourceClientKind::Codex), origin:ScanStartOrigin::ExplicitUser}, ScanCancellation::new()));
        assert!(result.is_err());
    }
    assert_eq!(service.context.coverage.lock().unwrap()[&SourceClientKind::Codex].state, CoverageState::Partial);
    assert!(!service.scan_status().running);
    service.finish();
}
