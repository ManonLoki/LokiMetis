//! Grok 旧共享 Hook 文件迁移的隔离回归测试。

use super::*;
use loki_metis_core::generate_hook_config;
use serde_json::json;
use tempfile::tempdir;

/// 生成旧版 Loki 写入 Grok AIMonitor 文件的完整受管内容。
fn old_loki_grok_config() -> String {
    generate_hook_config(AiTool::Grok, Path::new("/opt/old-lokimetis"))
        .expect("old Grok preview")
        .content
}

#[test]
/// 纯 Loki 旧文件会原子清空为有效对象，重复迁移不再写入。
fn pure_loki_legacy_file_becomes_explicit_empty_json_idempotently() {
    let root = tempdir().expect("temp");
    let path = root.path().join(LEGACY_GROK_CONFIG);
    fs::create_dir_all(path.parent().expect("parent")).expect("legacy directory");
    fs::write(&path, old_loki_grok_config()).expect("legacy config");

    assert!(migrate_legacy_grok_local(root.path()).expect("first migration"));
    assert_eq!(
        fs::read_to_string(&path).expect("empty legacy config"),
        "{}"
    );
    assert!(!migrate_legacy_grok_local(root.path()).expect("second migration"));
}

#[test]
/// 首次清理后 AIMonitor 并发替换混合内容时，下一轮只移除 Loki 并保留新增字段。
fn concurrent_external_replacement_is_remerged_without_losing_user_content() {
    let root = tempdir().expect("temp");
    let path = root.path().join(LEGACY_GROK_CONFIG);
    fs::create_dir_all(path.parent().expect("parent")).expect("legacy directory");
    let old_content = old_loki_grok_config();
    fs::write(&path, &old_content).expect("legacy config");

    let mut mixed: serde_json::Value = serde_json::from_str(&old_content).expect("old JSON");
    mixed["aimonitorOwner"] = json!(true);
    mixed["hooks"]["SessionStart"]
        .as_array_mut()
        .expect("SessionStart groups")
        .push(json!({
            "hooks": [{"type": "command", "command": "aimonitor-user-handler"}]
        }));
    let mixed = serde_json::to_string_pretty(&mixed).expect("mixed JSON");
    let path_for_observer = path.clone();
    let mut injected = false;

    let changed = reconcile_legacy_grok_file(&path, move |_, _| {
        if !injected {
            super::super::atomic_file::write_monitor_file_atomically(
                &path_for_observer,
                mixed.as_bytes(),
                "error.hooks.writeFailed",
            )
            .expect("external replacement");
            injected = true;
        }
    })
    .expect("concurrent migration");

    assert!(changed);
    let final_content = fs::read_to_string(&path).expect("final legacy config");
    assert!(!final_content.contains("LokiMetis:tool=grok"));
    assert!(final_content.contains("aimonitor-user-handler"));
    let final_value: serde_json::Value = serde_json::from_str(&final_content).expect("final JSON");
    assert_eq!(final_value["aimonitorOwner"], true);
    assert!(!migrate_legacy_grok_local(root.path()).expect("idempotent migration"));
    assert_eq!(
        fs::read_to_string(path).expect("stable content"),
        final_content
    );
}
