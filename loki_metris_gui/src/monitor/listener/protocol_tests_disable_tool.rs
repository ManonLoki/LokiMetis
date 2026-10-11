// 本文件是 protocol_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：禁用工具会立即释放它占用的位置，并使旧代数事件失效。；重新启用会分配新代数，旧会话状态和旧排队事件不得复活。；旧 Hook 文件仍投递时返回成功以保持插件 fail-open，但不进入队列或状态计数。

/// 禁用工具会立即释放它占用的位置，并使旧代数事件失效。
#[test]
fn disabling_tool_releases_pet_state_and_invalidates_queued_generation() {
    let (status, control) = listener_control(&[AiTool::Codex]);
    let original_generation = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("enabled");
    {
        let mut current = status.write().expect("status");
        let mut revision = current.revision;
        super::super::listener_state::apply_pet_transition(
            &mut current.pet_states,
            &mut revision,
            AiTool::Codex,
            0,
            HookTransition::Display(HookBehavior::Running),
        );
        current.revision = revision;
    }

    assert!(control.replace_enabled_tools(&[]));
    assert!(
        status
            .read()
            .expect("status")
            .pet_states
            .iter()
            .all(|state| state.behavior.is_none())
    );
    assert!(
        !hook_listener_policy(&control.policy)
            .admits_generation(AiTool::Codex, original_generation)
    );
}

/// 重新启用会分配新代数，旧会话状态和旧排队事件不得复活。
#[test]
fn reenabled_tool_uses_a_fresh_generation() {
    let (_status, control) = listener_control(&[AiTool::Codex]);
    let original = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("enabled");
    assert!(!control.replace_enabled_tools(&[]));
    assert!(!control.replace_enabled_tools(&[AiTool::Codex]));
    let reenabled = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("reenabled");
    assert!(reenabled > original);
    assert!(!hook_listener_policy(&control.policy).admits_generation(AiTool::Codex, original));
}

/// 旧 Hook 文件仍投递时返回成功以保持插件 fail-open，但不进入队列或状态计数。
#[test]
fn disabled_tool_request_is_acknowledged_without_reaching_the_worker() {
    let (status, control) = listener_control(&[]);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let outcome = ConnectionOutcome {
        ok: true,
        authenticated: true,
        status: "202 Accepted",
        body: "Accepted",
        event: Some(codex_event("SessionStart", Some("session-1"), None)),
    };

    let response = enqueue_connection_outcome(outcome, &sender, &status, &control.policy);
    assert_eq!(response, ("202 Accepted", "Accepted", true));
    assert!(receiver.try_recv().is_err());
    let current = status.read().expect("status");
    assert_eq!(current.received_count, 0);
    assert!(current.pet_states.is_empty());
}
