// 本文件是 tests_lifecycle.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证 Grok 会话轮次覆盖开始、进度、停止和会话结束。；验证会话跟踪保持有界并按预定优先级淘汰。

// Grok Build 走会话/轮次 latest-wins：start → progress → stop / session-end。
#[test]
/// 验证 Grok 会话轮次覆盖开始、进度、停止和会话结束。
fn grok_session_turn_machine_covers_start_progress_stop_and_session_end() {
    let mut grok = HookStateMachine::default();
    assert_eq!(
        grok.apply_event(AiTool::Grok, "SessionStart", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        grok.apply_event(
            AiTool::Grok,
            "UserPromptSubmit",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    assert_eq!(
        grok.apply_event(
            AiTool::Grok,
            "PreToolUse",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        grok.apply_event(AiTool::Grok, "Stop", Some("session-1"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    assert_eq!(
        grok.apply_event(
            AiTool::Grok,
            "PostToolUse",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Ignore
    );
    assert_eq!(
        grok.apply_event(AiTool::Grok, "SessionEnd", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试：会话追踪数量保持在上限内，并遵循既定的淘汰优先级
#[test]
/// 验证会话跟踪保持有界并按预定优先级淘汰。
fn session_tracking_stays_bounded_and_uses_eviction_priority() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 先把追踪槽位填满到上限：依次创建 MAX_TRACKED_HOOK_SESSIONS 个活跃会话
    for index in 0..MAX_TRACKED_HOOK_SESSIONS {
        // 构造形如 "session-000" 的会话 id（补零对齐，便于排序观察）
        let session_id = format!("session-{index:03}");
        // 以递增的秒数作为观测时间，逐个建立会话
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some(&session_id),
            Some("turn"),
            None,
            Duration::from_secs(index as u64),
        );
    }
    // 断言此时追踪的会话数恰好达到上限
    assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);

    // 即使墓碑较新，也应先于任何活跃会话淘汰。
    // 先让 session-000 结束，使其成为“墓碑”
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionEnd",
        Some("session-000"),
        None,
        None,
        Duration::from_mins(5),
    );
    // 再新增一个会话，触发容量淘汰逻辑
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-1"),
        Some("turn"),
        None,
        Duration::from_secs(301),
    );
    // 断言被淘汰的正是墓碑会话 session-000
    assert!(!machine.sessions.contains_key("session-000"));

    // 非活跃会话其次；只有两类都不存在时才淘汰最旧活跃会话。
    // 让 session-001 停止（进入非活跃但未结束的状态）
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "Stop",
        Some("session-001"),
        Some("turn"),
        None,
        Duration::from_secs(302),
    );
    // 再新增一个会话，触发容量淘汰逻辑
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-2"),
        Some("turn"),
        None,
        Duration::from_secs(303),
    );
    // 断言被淘汰的正是非活跃会话 session-001（次优先级）
    assert!(!machine.sessions.contains_key("session-001"));
    // 此时既无墓碑也无非活跃会话，再新增一个会话应淘汰最旧的活跃会话
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("overflow-3"),
        Some("turn"),
        None,
        Duration::from_secs(304),
    );
    // 断言被淘汰的是最旧的活跃会话 session-002
    assert!(!machine.sessions.contains_key("session-002"));
    // 断言追踪数量始终保持在上限，未超出
    assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);
}
