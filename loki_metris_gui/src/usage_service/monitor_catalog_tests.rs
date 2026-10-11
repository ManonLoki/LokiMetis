//! 服务接受的监控写入由 owner 保留；所有测试只操作显式临时目录。

use super::*;
use loki_metris_core::{AiTool, HookBehavior};

/// 真实文件选择路径复制、档案提交、公开过滤和服务重启均走生产入口。
#[test]
fn catalog_service_roundtrip_and_lost_receipt_keep_committed_writes() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("original.gif");
    std::fs::write(
        &original,
        include_bytes!("../../tests/fixtures/monitor-loop.gif"),
    )
    .unwrap();
    let data = root.path().join("data");
    let service = UsageService::in_directory(data.clone()).unwrap();
    let imported = service
        .import_monitor_image(original.clone())
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    let id = imported.id.clone();
    let mut draft = AiProfileDraft::default_for(AiTool::Codex);
    draft.slot = 7;
    draft
        .hooks
        .iter_mut()
        .find(|hook| hook.behavior == HookBehavior::Idle)
        .unwrap()
        .image = id.clone();
    let receipt = service.save_monitor_profile_draft(draft).unwrap();
    drop(receipt);
    service.finish();
    let restarted = UsageService::in_directory(data).unwrap();
    let set = restarted
        .list_monitor_profile_drafts()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    assert_eq!(set.drafts.len(), 5);
    assert!(
        set.drafts
            .iter()
            .all(|draft| is_public_monitor_tool(draft.tool))
    );
    let codex = set
        .drafts
        .iter()
        .find(|draft| draft.tool == AiTool::Codex)
        .unwrap();
    assert_eq!(codex.slot, 7);
    assert_eq!(codex.hooks[0].image, id);
    assert_eq!(
        restarted
            .delete_monitor_image(id)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap_err()
            .code,
        "error.monitor.imageInUse"
    );
    assert!(original.exists());
    restarted.finish();
    assert!(restarted.list_monitor_images().is_err());
}

/// 并发不同 Agent 保存合并最新完整集合，公开写接口不能修改内部隐藏协议。
#[test]
fn profile_service_merges_distinct_agents_and_rejects_hidden_tool() {
    let root = tempfile::tempdir().unwrap();
    let service = UsageService::in_directory(root.path().to_owned()).unwrap();
    let mut codex = AiProfileDraft::default_for(AiTool::Codex);
    codex.slot = 2;
    let mut claude = AiProfileDraft::default_for(AiTool::ClaudeCode);
    claude.slot = 9;
    let one = service.save_monitor_profile_draft(codex).unwrap();
    let two = service.save_monitor_profile_draft(claude).unwrap();
    one.blocking_recv().unwrap().unwrap();
    two.blocking_recv().unwrap().unwrap();
    let set = service
        .list_monitor_profile_drafts()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    assert_eq!(
        set.drafts
            .iter()
            .find(|draft| draft.tool == AiTool::Codex)
            .unwrap()
            .slot,
        2
    );
    assert_eq!(
        set.drafts
            .iter()
            .find(|draft| draft.tool == AiTool::ClaudeCode)
            .unwrap()
            .slot,
        9
    );
    assert_eq!(service.monitor_catalog_revision(), 2);
    assert_eq!(
        service
            .save_monitor_profile_draft(AiProfileDraft::default_for(AiTool::OpenCode))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap_err()
            .code,
        "error.hooks.toolUnavailable"
    );
    service.finish();
}
