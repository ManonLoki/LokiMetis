//! AIMonitor 源状态机对应场景的 LokiMetis 回归测试。

// 引入标准库的 Duration，用于构造带时间戳的测试事件
use std::time::Duration;

// 引入被测状态机相关类型：事件决策结果、状态机本体、转场类型、
// 状态机允许追踪的最大会话数常量
use super::{HookEventDecision, HookStateMachine, HookTransition, MAX_TRACKED_HOOK_SESSIONS};
// 引入 AI 工具枚举与 Hook 展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// 测试：孤立的完成事件被忽略，且不会留下幽灵会话
#[test]
fn orphan_completion_is_ignored_without_leaving_a_ghost_session() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();

    // Monitor 中途启动，直接收到一个从未见过会话的 PostToolUse（完成事件），
    // 应被忽略
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "PostToolUse",
            Some("late-session"),
            Some("turn-1"),
            None,
            Duration::from_secs(10),
        ),
        HookEventDecision::Ignore
    );
    // 断言状态机中没有为该孤立事件创建任何会话记录
    assert_eq!(machine.tracked_session_count(), 0);

    // 与完成事件不同，真实工作进展可以作为 Monitor 中途启动后的首个事件。
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "PreToolUse",
            Some("live-session"),
            Some("turn-1"),
            None,
            Duration::from_secs(11),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 断言这次确实创建了一个会话记录
    assert_eq!(machine.tracked_session_count(), 1);
}

// 测试：已结束的墓碑拒绝迟到事件，但显式 SessionStart 可以恢复会话
#[test]
fn ended_tombstone_rejects_late_events_but_explicit_session_start_can_resume() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 在 t=1s 时刻，s1 会话下 t1 轮次提交
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("t1"),
        None,
        Duration::from_secs(1),
    );
    // 在 t=2s 时刻结束该会话，应转发释放态，形成墓碑
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("s1"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );

    // 遍历一系列常见事件类型，验证在 t=100s（墓碑期间）它们均应被忽略
    for event in [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "Stop",
    ] {
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                event,
                Some("s1"),
                Some("t1"),
                None,
                Duration::from_secs(100),
            ),
            HookEventDecision::Ignore,
            "墓碑应拒绝迟到事件 {event}"
        );
    }
    // 断言最后一次活跃时间仍停留在结束时刻（t=2s），未被上面的迟到事件刷新
    assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(2));

    // 在 t=101s 时刻显式 SessionStart，应能恢复墓碑，转发空闲展示态
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("s1"),
            None,
            None,
            Duration::from_secs(101),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 断言墓碑标记已被清除
    assert!(!machine.sessions["s1"].ended);
    // 断言最后活跃时间已刷新为恢复时刻
    assert_eq!(
        machine.sessions["s1"].last_seen_at,
        Duration::from_secs(101)
    );
}

// 测试：未知会话的结束事件只释放一次，且不会覆盖其他存活会话
#[test]
fn unknown_session_end_releases_once_without_overriding_other_live_sessions() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();

    // 从未见过的会话直接收到 SessionEnd，第一次应被接受并转发释放态
    // （视为“至少通知一次已结束”）
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("unknown"),
            None,
            None,
            Duration::from_secs(1),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );
    // 同一未知会话的重复结束事件，第二次应被忽略
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("unknown"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Ignore
    );
    // 断言最后活跃时间停留在第一次结束的时刻，未被第二次刷新
    assert_eq!(
        machine.sessions["unknown"].last_seen_at,
        Duration::from_secs(1)
    );

    // 建立另一个正常存活的会话
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("live"),
        Some("turn"),
        None,
        Duration::from_secs(3),
    );
    // 再一个未知会话的结束事件到来，不应影响或覆盖上面存活会话的状态，
    // 该事件自身也应被忽略（因为“未知会话释放一次”的配额已被上面用掉）
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("another-unknown"),
            None,
            None,
            Duration::from_secs(4),
        ),
        HookEventDecision::Ignore
    );
}

// 测试：批量过期会话时，多个变化会被合并为一次最终的聚合转场
#[test]
fn expiring_sessions_batches_changes_into_one_final_aggregate_transition() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 会话 "asking"：在 t=0 提交后进入等待权限（询问态）
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("asking"),
        Some("t1"),
        None,
        Duration::ZERO,
    );
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "PermissionRequest",
        Some("asking"),
        Some("t1"),
        None,
        Duration::ZERO,
    );
    // 会话 "running"：在 t=5s 提交，处于运行态
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("running"),
        Some("t2"),
        None,
        Duration::from_secs(5),
    );
    // 会话 "idle"：在 t=15s 开始，处于空闲态
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionStart",
        Some("idle"),
        None,
        None,
        Duration::from_secs(15),
    );

    // Asking 和 Running 同批到期；对外只暴露最终仍存活的 Idle 聚合态。
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(20), Duration::from_secs(10),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 断言此时只剩下 "idle" 这一个会话仍被追踪
    assert_eq!(machine.tracked_session_count(), 1);
    // 再次以更晚的时间触发过期清理，此时连 "idle" 也应过期，
    // 由于没有更多存活会话，聚合结果应为 Ignore
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(26), Duration::from_secs(10),),
        HookEventDecision::Ignore
    );
    // 断言此时已没有任何会话被追踪
    assert_eq!(machine.tracked_session_count(), 0);
}

// 测试：最后一个活跃会话过期时应回落为 Idle 展示，而不是直接释放槽位
#[test]
fn expiring_last_active_session_falls_back_to_idle_without_releasing_slot() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 建立唯一一个活跃会话（Claude Code 工具，t=0 提交）
    machine.apply_event_with_status_at(
        AiTool::ClaudeCode,
        "UserPromptSubmit",
        Some("active"),
        Some("turn"),
        None,
        Duration::ZERO,
    );

    // 在超过非活跃阈值后触发过期清理，应转发空闲展示态
    // （即使最后一个会话过期，也先展示为 Idle 而不是直接消失）
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(11), Duration::from_secs(10)),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 断言该会话记录本身已被清理，不再占用追踪槽位
    assert_eq!(machine.tracked_session_count(), 0);
}

// 测试：第二阶段接入的多个工具（Qwen/Qoder/Gemini/Copilot）符合预期的
// 事件映射，且最新轮次会覆盖旧轮次的进度判断
#[test]
fn phase2_tools_use_expected_mapping_and_latest_turn_wins() {
    // --- Qwen Code 工具的行为验证 ---
    let mut qwen = HookStateMachine::default();
    // 会话开始，应转发空闲展示态
    assert_eq!(
        qwen.apply_event(AiTool::QwenCode, "SessionStart", Some("s"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 提交 turn-1，应转发运行展示态
    assert_eq!(
        qwen.apply_event(
            AiTool::QwenCode,
            "UserPromptSubmit",
            Some("s"),
            Some("turn-1")
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // turn-1 停止，应转发空闲展示态
    assert_eq!(
        qwen.apply_event(AiTool::QwenCode, "Stop", Some("s"), Some("turn-1"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 已停止的 turn-1 收到 PreToolUse（旧轮次的进度），应被忽略
    assert_eq!(
        qwen.apply_event(AiTool::QwenCode, "PreToolUse", Some("s"), Some("turn-1")),
        HookEventDecision::Ignore
    );
    // 全新的 turn-2 收到 PreToolUse，应作为隐式新轮次起点，转发运行展示态
    assert_eq!(
        qwen.apply_event(AiTool::QwenCode, "PreToolUse", Some("s"), Some("turn-2")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );

    // --- Qoder 工具的行为验证 ---
    let mut qoder = HookStateMachine::default();
    // 提交 turn-1，应转发运行展示态
    assert_eq!(
        qoder.apply_event(AiTool::Qoder, "UserPromptSubmit", Some("q"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // turn-1 停止，应转发空闲展示态
    assert_eq!(
        qoder.apply_event(AiTool::Qoder, "Stop", Some("q"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 已停止的 turn-1 收到 PreToolUse，应被忽略
    assert_eq!(
        qoder.apply_event(AiTool::Qoder, "PreToolUse", Some("q"), Some("turn-1")),
        HookEventDecision::Ignore
    );

    // --- Gemini CLI 工具的行为验证 ---
    let mut gemini = HookStateMachine::default();
    // 会话开始，应转发空闲展示态
    assert_eq!(
        gemini.apply_event(AiTool::GeminiCli, "SessionStart", Some("g"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // Agent 开始前（工作开始），应转发运行展示态
    assert_eq!(
        gemini.apply_event(AiTool::GeminiCli, "BeforeAgent", Some("g"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // Agent 结束后（停止事件），应转发空闲展示态
    assert_eq!(
        gemini.apply_event(AiTool::GeminiCli, "AfterAgent", Some("g"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 已停止后收到压缩前事件（旧轮次进度），应被忽略
    assert_eq!(
        gemini.apply_event(AiTool::GeminiCli, "PreCompress", Some("g"), Some("turn-1"),),
        HookEventDecision::Ignore
    );

    // --- GitHub Copilot 工具的行为验证 ---
    let mut copilot = HookStateMachine::default();
    // 会话开始，应转发空闲展示态
    assert_eq!(
        copilot.apply_event(AiTool::GitHubCopilot, "sessionStart", Some("c"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 提交 turn-1，应转发运行展示态
    assert_eq!(
        copilot.apply_event(
            AiTool::GitHubCopilot,
            "userPromptSubmitted",
            Some("c"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // Agent 停止，应转发空闲展示态
    assert_eq!(
        copilot.apply_event(
            AiTool::GitHubCopilot,
            "agentStop",
            Some("c"),
            Some("turn-1")
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 已停止后收到工具调用前事件（旧轮次进度），应被忽略
    assert_eq!(
        copilot.apply_event(
            AiTool::GitHubCopilot,
            "preToolUse",
            Some("c"),
            Some("turn-1")
        ),
        HookEventDecision::Ignore
    );
    // 这些公开 payload 不保证提供 turn_id；下一次真实的工作开始事件仍须在
    // 同一 session 内开启新轮次，不能永久停留在 Idle。
    assert_eq!(
        copilot.apply_event(
            AiTool::GitHubCopilot,
            "userPromptSubmitted",
            Some("c"),
            None,
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

// Grok Build 走会话/轮次 latest-wins：start → progress → stop / session-end。
#[test]
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
