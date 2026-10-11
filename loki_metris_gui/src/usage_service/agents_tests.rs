//! 隔离配置与真实查询接口验证统一选择、失败保持和各消费点的短路行为。

use super::*;
use super::super::{UsageQuery, UsageResponse};
use loki_metris_core::{LocalUsageWindow, UsageCallsQuery, UsageChartDimension, source_client_usage_index_path};

/// 新安装三项默认与明确清空具有不同语义；两个选择均跨服务重启保留。
#[test]
fn default_selection_and_explicit_empty_are_distinct() {
    let root=tempfile::tempdir().unwrap();let directory=root.path().join("data");
    let service=UsageService::in_directory(directory.clone()).unwrap();
    let read=service.monitor_settings().unwrap();
    let settings=service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap().unwrap();
    assert_eq!(settings.enabled_ai_tools,vec![AiTool::Codex,AiTool::ClaudeCode,AiTool::Cursor]);
    assert!(!directory.exists());
    let save=service.save_enabled_ai_tools(Vec::new()).unwrap();
    assert!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(save).unwrap().unwrap().enabled_ai_tools.is_empty());
    assert_eq!(service.agent_settings_revision(),1);service.finish();
    assert!(service.save_enabled_ai_tools(vec![AiTool::Grok]).is_err());
    let service=UsageService::in_directory(directory.clone()).unwrap();
    for query in [UsageQuery::Overview(None),UsageQuery::Calls(None,UsageCallsQuery::default()),UsageQuery::Chart(None,LocalUsageWindow::Today,UsageChartDimension::Agent),UsageQuery::Sources(SourceClientKind::WorkBuddy)] {
        let read=service.query(query).unwrap();
        assert!(matches!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap(),Err("usage_agents_disabled")));
    }
    service.finish();assert!(!directory.join("usage-settings.json").exists());
}

/// 停用 Grok 的损坏索引不能污染 All；启用后相同读取必须暴露真实故障。
#[test]
fn combined_queries_only_open_enabled_physical_indexes() {
    let root=tempfile::tempdir().unwrap();let directory=root.path().to_owned();
    let file=source_client_usage_index_path(&directory,SourceClientKind::GrokBuildCli);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();std::fs::write(&file,b"bad sqlite").unwrap();
    let service=UsageService::in_directory(directory.clone()).unwrap();
    let read=service.query(UsageQuery::Calls(None,UsageCallsQuery::default())).unwrap();
    assert!(matches!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap(),Err("usage_sources_not_configured")));
    let save=service.save_enabled_ai_tools(vec![AiTool::Grok]).unwrap();
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(save).unwrap().unwrap();
    let read=service.query(UsageQuery::Overview(None)).unwrap();
    let result=service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap();
    assert!(matches!(result,Err("usage_index_unavailable"|"usage_read_failed")));
    service.finish();assert_eq!(std::fs::read(file).unwrap(),b"bad sqlite");
}

/// 格式损坏拒绝读取与覆盖；修复后能重试，失败不会递增提交代数。
#[test]
fn corrupt_settings_are_preserved_and_read_can_retry() {
    let root=tempfile::tempdir().unwrap();let directory=root.path().join("data");
    let config=directory.join("monitor");std::fs::create_dir_all(&config).unwrap();
    let file=config.join("monitor-settings.json");std::fs::write(&file,b"{").unwrap();
    let service=UsageService::in_directory(directory).unwrap();
    let read=service.monitor_settings().unwrap();assert!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap().is_err());
    let save=service.save_enabled_ai_tools(vec![AiTool::Grok]).unwrap();assert!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(save).unwrap().is_err());
    assert_eq!(service.agent_settings_revision(),0);assert_eq!(std::fs::read(&file).unwrap(),b"{");
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(super::super::scan::run(service.context.clone(),super::super::queue::Intent{client:Some(SourceClientKind::Codex),origin:loki_metris_core::ScanStartOrigin::ExplicitUser},loki_metris_core::ScanCancellation::new())).unwrap_err();
    assert_eq!(service.scan_status().error,Some("usage_agent_settings_unavailable"));assert!(!service.scan_status().running);assert_eq!(service.scan_status().revision,1);
    std::fs::remove_file(file).unwrap();let read=service.monitor_settings().unwrap();
    assert_eq!(service.runtime.lock().unwrap().as_ref().unwrap().block_on(read).unwrap().unwrap().enabled_ai_tools.len(),3);
    service.finish();
}

/// 页面丢弃回执仍完成已接受保存，其他监控字段不能被启用集合覆盖。
#[test]
fn dropped_save_receipt_still_commits_and_preserves_other_monitor_fields() {
    let root=tempfile::tempdir().unwrap();let config=root.path().join("monitor");
    update_monitor_settings(&config,|settings|{settings.pet_window.locked=true;settings.hook_directories.codex="/example/codex".into();}).unwrap();
    let service=UsageService::in_directory(root.path().to_owned()).unwrap();
    drop(service.save_enabled_ai_tools(vec![AiTool::WorkBuddy,AiTool::OpenCode,AiTool::Codex,AiTool::Codex]).unwrap());
    service.finish();let settings=load_monitor_settings(&config).unwrap();
    assert_eq!(settings.enabled_ai_tools,vec![AiTool::Codex,AiTool::WorkBuddy]);
    assert!(settings.pet_window.locked);assert_eq!(settings.hook_directories.codex,"/example/codex");
}

/// 自动批次即使显式携带停用客户端也不能创建该索引，显式物理查询仍可读取旧库。
#[test]
fn disabled_automatic_scan_is_a_noop_and_direct_physical_read_remains_available() {
    let root=tempfile::tempdir().unwrap();let directory=root.path().to_owned();
    let service=UsageService::in_directory(directory.clone()).unwrap();
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(async {
        super::super::scan::run(service.context.clone(),super::super::queue::Intent{client:Some(SourceClientKind::GrokBuildCli),origin:loki_metris_core::ScanStartOrigin::InitialAutomatic},loki_metris_core::ScanCancellation::new()).await.unwrap();
        let response=service.query(UsageQuery::Overview(Some(SourceClientKind::GrokBuildCli))).unwrap().await.unwrap().unwrap();
        assert!(matches!(response,UsageResponse::Overview(_)));
    });
    service.finish();assert!(!directory.join("monitor").exists());assert!(!source_client_usage_index_path(&directory,SourceClientKind::GrokBuildCli).exists());
}
