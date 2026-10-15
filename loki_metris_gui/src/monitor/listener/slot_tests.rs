//! Hook 迁移发生时的位置快照回归。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use loki_metris_core::{AiTool, HookBehavior, HookStateMachine};
use tempfile::tempdir;

use super::{
    HookRelayStatus, IncomingHookEvent, expire_inactive_hook_sessions, process_hook_event,
};

/// 构造一条带会话上下文的 Codex 事件。
fn codex_event(hook_type: &str, turn_id: Option<&str>) -> IncomingHookEvent {
    IncomingHookEvent {
        tool: AiTool::Codex,
        hook_type: hook_type.to_owned(),
        session_id: Some("session-1".to_owned()),
        turn_id: turn_id.map(str::to_owned),
        status: None,
    }
}

/// 展示位置在迁移发生时捕获，后续修改 profile 不会搬动旧位置记录。
#[test]
fn profile_slot_is_captured_at_transition_time() {
    let root = tempdir().expect("temp");
    let config = root.path().join("config");
    let data = root.path().join("data");
    std::fs::create_dir_all(&config).expect("config");
    std::fs::create_dir_all(&data).expect("data");
    let mut draft = loki_metris_core::AiProfileDraft::default_for(AiTool::Codex);
    draft.slot = 2;
    save_profile_fixture(&config, &data, draft.clone()).expect("first slot");
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut machines = HashMap::<AiTool, HookStateMachine>::new();
    assert!(process_hook_event(
        codex_event("SessionStart", None),
        Duration::from_secs(1),
        &mut machines,
        &status,
        &config,
    ));
    draft.slot = 5;
    save_profile_fixture(&config, &data, draft).expect("second slot");
    assert!(process_hook_event(
        codex_event("UserPromptSubmit", Some("turn-1")),
        Duration::from_secs(2),
        &mut machines,
        &status,
        &config,
    ));
    {
        let current = status.read().expect("status");
        assert_eq!(current.pet_states.len(), 2);
        assert!(current.pet_states.iter().any(|state| {
            state.slot_index == 1 && state.behavior.is_none() && state.revision == 2
        }));
        assert!(current.pet_states.iter().any(|state| {
            state.slot_index == 4
                && state.behavior == Some(HookBehavior::Running)
                && state.revision == 2
        }));
    }
    assert!(process_hook_event(
        codex_event("SessionEnd", None),
        Duration::from_secs(3),
        &mut machines,
        &status,
        &config,
    ));
    let current = status.read().expect("status");
    assert_eq!(current.revision, 3);
    assert!(
        current
            .pet_states
            .iter()
            .all(|state| state.behavior.is_none())
    );
}

/// profile 损坏时 Release 不读取位置，仍清空已有状态并提交状态机。
#[test]
fn release_clears_active_state_even_when_profile_is_corrupt() {
    let root = tempdir().expect("temp");
    let config = root.path().join("config");
    let data = root.path().join("data");
    std::fs::create_dir_all(&config).expect("config");
    std::fs::create_dir_all(&data).expect("data");
    let draft = loki_metris_core::AiProfileDraft::default_for(AiTool::Codex);
    save_profile_fixture(&config, &data, draft).expect("profile");
    let profile_path = config.join("monitor-profiles.json");
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut machines = HashMap::<AiTool, HookStateMachine>::new();
    assert!(process_hook_event(
        codex_event("SessionStart", None),
        Duration::from_secs(1),
        &mut machines,
        &status,
        &config,
    ));
    std::fs::write(&profile_path, b"{").expect("corrupt profile");

    assert!(process_hook_event(
        codex_event("SessionEnd", None),
        Duration::from_secs(2),
        &mut machines,
        &status,
        &config,
    ));
    {
        let current = status.read().expect("released status");
        assert_eq!(current.revision, 2);
        assert_eq!(current.failed_count, 0);
        assert!(
            current
                .pet_states
                .iter()
                .all(|state| state.behavior.is_none())
        );
    }

    assert!(!process_hook_event(
        codex_event("SessionEnd", None),
        Duration::from_secs(3),
        &mut machines,
        &status,
        &config,
    ));
    let current = status.read().expect("duplicate status");
    assert_eq!(current.revision, 2);
    assert!(
        current
            .pet_states
            .iter()
            .all(|state| state.behavior.is_none())
    );
}

/// profile 暂时不可读时不消费超时回落，恢复后仍可把 Running 正确降为 Idle。
#[test]
fn failed_slot_read_does_not_consume_expiry_transition() {
    let root = tempdir().expect("temp");
    let config = root.path().join("config");
    let data = root.path().join("data");
    std::fs::create_dir_all(&config).expect("config");
    std::fs::create_dir_all(&data).expect("data");
    let draft = loki_metris_core::AiProfileDraft::default_for(AiTool::Codex);
    save_profile_fixture(&config, &data, draft).expect("profile");
    let profile_path = config.join("monitor-profiles.json");
    let valid_profile = std::fs::read(&profile_path).expect("valid profile");
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut machines = HashMap::<AiTool, HookStateMachine>::new();
    assert!(process_hook_event(
        codex_event("SessionStart", None),
        Duration::from_secs(1),
        &mut machines,
        &status,
        &config,
    ));
    assert!(process_hook_event(
        codex_event("UserPromptSubmit", Some("turn-1")),
        Duration::from_secs(2),
        &mut machines,
        &status,
        &config,
    ));
    std::fs::write(&profile_path, b"{").expect("corrupt profile");

    assert!(!expire_inactive_hook_sessions(
        &mut machines,
        Duration::from_secs(30 * 60 + 3),
        &status,
        &config,
    ));
    {
        let current = status.read().expect("failed status");
        assert_eq!(current.revision, 2);
        assert_eq!(current.failed_count, 1);
        assert!(current.pet_states.iter().any(|state| {
            state.tool == AiTool::Codex && state.behavior == Some(HookBehavior::Running)
        }));
    }
    std::fs::write(&profile_path, valid_profile).expect("restore profile");

    assert!(expire_inactive_hook_sessions(
        &mut machines,
        Duration::from_secs(30 * 60 + 4),
        &status,
        &config,
    ));
    let current = status.read().expect("retried status");
    assert_eq!(current.revision, 3);
    assert!(current.pet_states.iter().any(|state| {
        state.tool == AiTool::Codex && state.behavior == Some(HookBehavior::Idle)
    }));
}

/// 测试仅写隔离目录的草稿 fixture，不提供产品保存桩。
fn save_profile_fixture(config:&std::path::Path,_data:&std::path::Path,draft:loki_metris_core::AiProfileDraft)->std::io::Result<()> {std::fs::write(config.join("monitor-profiles.json"),serde_json::to_vec(&vec![draft]).unwrap())}

/// 同槽工具互相覆盖不能丢失通知前态；启动、重复询问与结束后的释放保持静默。
#[test]
fn hook_notifications_survive_shared_pet_slot_and_deduplicate_states() {
    use crate::notification_events::{Activity, ActivityQueue};
    use loki_metris_core::HookNotification;
    let root = tempdir().unwrap();
    let mut drafts = [loki_metris_core::AiProfileDraft::default_for(AiTool::Codex), loki_metris_core::AiProfileDraft::default_for(AiTool::ClaudeCode)];
    for draft in &mut drafts { draft.slot = 1; }
    std::fs::write(root.path().join("monitor-profiles.json"), serde_json::to_vec(&drafts).unwrap()).unwrap();
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut machines = HashMap::new();
    let queue = ActivityQueue::default();
    queue.configure(true);
    let mut process = |event: IncomingHookEvent| {
        super::lifecycle::process_hook_event_checked(event, Duration::from_secs(1), &mut machines, &status, root.path(), None, Some(&queue))
    };
    process(codex_event("SessionStart", None));
    assert!(queue.take().is_none());
    process(codex_event("UserPromptSubmit", Some("turn-1")));
    let mut other = codex_event("UserPromptSubmit", Some("turn-other"));
    other.tool = AiTool::ClaudeCode;
    process(other);
    assert!(!status.read().unwrap().pet_states.iter().any(|state| state.tool == AiTool::Codex && state.behavior.is_some()));
    process(codex_event("PermissionRequest", Some("turn-1")));
    process(codex_event("PermissionRequest", Some("turn-1")));
    assert_eq!(queue.take().unwrap().activity, Activity::Hook { tool: AiTool::Codex, state: HookNotification::Asking });
    assert!(queue.take().is_none());
    process(codex_event("Stop", Some("turn-1")));
    process(codex_event("SessionEnd", None));
    assert_eq!(queue.take().unwrap().activity, Activity::Hook { tool: AiTool::Codex, state: HookNotification::Ended });
    assert!(queue.take().is_none());
    let mut failure = codex_event("PostToolUseFailure", Some("turn-other"));
    failure.tool = AiTool::ClaudeCode;
    process(failure.clone());
    process(failure);
    assert_eq!(queue.take().unwrap().activity, Activity::Hook { tool: AiTool::ClaudeCode, state: HookNotification::Error });
    let mut stop = codex_event("Stop", Some("turn-other"));
    stop.tool = AiTool::ClaudeCode;
    process(stop);
    assert_eq!(queue.take().unwrap().activity, Activity::Hook { tool: AiTool::ClaudeCode, state: HookNotification::Ended });
    assert!(queue.take().is_none());
}
