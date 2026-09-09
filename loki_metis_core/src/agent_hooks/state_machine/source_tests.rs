//! AIMonitor 源状态机对应场景的 LokiMetis 回归测试。

// 标准库：Duration 用于构造带具体观察时间的测试事件
use std::time::Duration;

// 引入待测试的状态机核心类型
use super::{HookEventDecision, HookPhase, HookStateMachine, HookTransition};
// 引入领域层的工具枚举与展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// 测试用例：验证依赖 status 字段区分状态的协议（Hermes/OpenClaw）能正确映射到原生状态
#[test]
/// 验证依赖原生 status 的协议能映射成功、失败与中断状态。
fn status_driven_protocols_map_native_states() {
    // 构造一台全新的 Hermes 状态机
    let mut hermes = HookStateMachine::default();
    // 发送“大模型调用前”事件，期望切换为 Running 展示
    assert_eq!(
        hermes.apply_event(
            AiTool::Hermes,
            "pre_llm_call",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 发送“审批请求前”事件，期望切换为 Asking 展示
    assert_eq!(
        hermes.apply_event(
            AiTool::Hermes,
            "pre_approval_request",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );

    // 构造一台全新的 OpenClaw 状态机
    let mut open_claw = HookStateMachine::default();
    // 先发送会话开始事件建立会话
    open_claw.apply_event(AiTool::OpenClaw, "session_start", Some("s1"), None);
    // 再发送“代理运行前”事件开启一个轮次
    open_claw.apply_event(AiTool::OpenClaw, "before_agent_run", Some("s1"), Some("r1"));
    // 发送带 status="failed" 的“代理结束”事件，期望被识别为 Error 展示
    assert_eq!(
        open_claw.apply_event_with_status(
            AiTool::OpenClaw,
            "agent_end",
            Some("s1"),
            Some("r1"),
            Some("failed"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
}

// 测试用例：验证不在“抑制重复事件”白名单内的工具，会持续转发相同的受支持事件
#[test]
/// 验证抑制白名单外的工具会逐次转发重复的受支持事件。
fn tools_outside_suppression_allowlist_forward_repeated_supported_events() {
    // 遍历一组“每事件必转发”类型工具及其对应的触发事件
    for (tool, event) in [
        (AiTool::WorkBuddy, "PreToolUse"),
        (AiTool::Hermes, "pre_llm_call"),
        (AiTool::OpenClaw, "before_agent_run"),
        (AiTool::CodeBuddy, "PreToolUse"),
    ] {
        // 为每个工具构造一台全新的状态机
        let mut machine = HookStateMachine::default();
        // 连续发送两次相同事件，期望两次都被转发（而非第二次被去重忽略）
        for _ in 0..2 {
            assert_eq!(
                machine.apply_event(
                    tool,
                    event,
                    Some("passthrough-session"),
                    Some("arbitrary-turn"),
                ),
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running)),
                "{tool:?} event {event} should be forwarded"
            );
        }
    }
}

// 测试用例：验证 Codex 状态机覆盖“开始-中断-迟到完成-退出”整条链路
#[test]
/// 验证 Codex 状态机覆盖打开、中断、迟到完成和退出的完整序列。
fn codex_state_machine_covers_open_interrupt_late_completion_and_exit() {
    // 构造一台全新的状态机（无会话/轮次标识，走简化测试接口）
    let mut machine = HookStateMachine::default();

    // 会话开始，期望展示切换为 Idle
    assert_eq!(
        machine.apply(AiTool::Codex, "SessionStart"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 用户提交提示，期望展示切换为 Running
    assert_eq!(
        machine.apply(AiTool::Codex, "UserPromptSubmit"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 提示处理完成后 Stop，期望展示切换回 Idle
    assert_eq!(
        machine.apply(AiTool::Codex, "Stop"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // Stop 之后不再依赖两秒窗口：无论迟到多久，完成类事件都不能重新进入运行态。
    assert_eq!(
        machine.apply(AiTool::Codex, "SubagentStop"),
        HookEventDecision::Ignore
    );
    // 同理，迟到的工具调用完成事件也应被忽略
    assert_eq!(
        machine.apply(AiTool::Codex, "PostToolUse"),
        HookEventDecision::Ignore
    );
    // 会话结束，期望展示位被释放
    assert_eq!(
        machine.apply(AiTool::Codex, "SessionEnd"),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试用例：验证只有真正的“工作开始”事件才能让状态机从已停止状态恢复运行
#[test]
/// 验证停止后的状态机只会由真实工作开始事件恢复。
fn state_machine_only_resumes_after_a_real_work_start() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();

    // 直接发送 Stop（无先前活跃轮次），期望展示切换为 Idle
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "Stop"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // PostCompact 不是明确的工作起点，期望被忽略，不应恢复 Running
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PostCompact"),
        HookEventDecision::Ignore
    );
    // 真正提交用户提示才是明确工作起点，期望切换为 Running
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "UserPromptSubmit"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 活跃轮次中的完成事件保持 Running；由于展示状态未变化，无需重复打设备请求。
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PostToolUse"),
        HookEventDecision::Ignore
    );
    // 权限请求事件应切换为 Asking 展示
    assert_eq!(
        machine.apply(AiTool::ClaudeCode, "PermissionRequest"),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
}

// 测试用例：验证重复的会话开始事件不会让一个已经在活跃运行的会话状态倒退
#[test]
/// 验证重复会话开始不会把活跃会话回退为空闲态。
fn duplicate_session_start_does_not_regress_an_active_session() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();
    // 在时间 1s 时开始会话 s1
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "SessionStart",
        Some("s1"),
        None,
        None,
        Duration::from_secs(1),
    );
    // 在时间 2s 时提交用户提示，开启轮次 t1，进入 Running
    machine.apply_event_with_status_at(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("t1"),
        None,
        Duration::from_secs(2),
    );

    // 在时间 3s 时又收到一次重复的 SessionStart，期望被忽略（不影响运行中的轮次）
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("s1"),
            None,
            None,
            Duration::from_secs(3),
        ),
        HookEventDecision::Ignore
    );
    // 断言会话 s1 的展示阶段仍然是 Running，未被重复 SessionStart 打断
    assert_eq!(machine.sessions["s1"].phase, HookPhase::Running);
    // 断言会话 s1 的轮次仍处于活跃状态
    assert!(machine.sessions["s1"].turn_active);
    // 断言重复的 SessionStart 仍然刷新了最后可见时间（避免被误判为过期）
    assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(3));
}

// 测试用例：验证 Cursor 的 stop 事件通过 status 字段区分“失败”与“正常完成”
#[test]
/// 验证 Cursor 停止事件依据 status 区分失败与正常完成。
fn cursor_stop_status_distinguishes_failure_from_completion() {
    // 构造第一台状态机，用于验证失败场景
    let mut machine = HookStateMachine::default();

    // 提交前事件，开启一个 Running 轮次
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("conversation-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 携带 status="error" 的 stop 事件，期望切换为 Error 展示
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("conversation-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );

    // 构造第二台状态机，用于验证正常完成场景
    let mut completed = HookStateMachine::default();
    // 提交前事件，开启一个 Running 轮次
    completed.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("conversation-2"),
        Some("generation-2"),
    );
    // 携带 status="completed" 的 stop 事件，期望切换为 Idle 展示
    assert_eq!(
        completed.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("conversation-2"),
            Some("generation-2"),
            Some("completed"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

// 测试用例：验证 Cursor 的真实会话事件会取代之前建立的工作区占位会话
#[test]
/// 验证 Cursor 真实会话会替换先前的工作区占位会话。
fn cursor_real_session_replaces_workspace_placeholder() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();

    // 打开工作区，建立默认占位会话，期望展示切换为 Idle
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 真实会话开始接管，展示阶段未变化（仍是 Idle），期望被忽略
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("conversation-1"), None,),
        HookEventDecision::Ignore
    );
    // 真实会话结束，期望展示位被释放
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("conversation-1"), None,),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试用例：验证多个并发会话的状态被正确聚合，且互不干扰（无串扰）
#[test]
/// 验证多会话聚合彼此隔离且不会发生状态串扰。
fn state_machine_aggregates_multiple_sessions_without_cross_talk() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();

    // 会话 s1 开始，期望展示切换为 Idle
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 会话 s1 提交提示，进入 Running，期望展示切换为 Running
    assert_eq!(
        machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s1"), Some("t1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 第二个空闲会话出现时，第一个会话仍在工作，聚合状态保持 Running。
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("s2"), None),
        HookEventDecision::Ignore
    );
    // 会话 s1 结束轮次，期望聚合状态回落为 Idle（s2 仍空闲）
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("t1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 会话 s2 开始工作，期望聚合状态切换为 Running
    assert_eq!(
        machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s2"), Some("t2")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 关闭 s1 不得释放仍在运行的 s2。
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionEnd", Some("s1"), None),
        HookEventDecision::Ignore
    );
    // 关闭最后一个会话 s2，期望展示位被释放
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionEnd", Some("s2"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试用例：验证来自旧轮次的迟到事件会被拒绝，只有当前轮次的事件才生效
#[test]
/// 验证状态机拒绝来自旧轮次的迟到事件。
fn state_machine_rejects_events_from_an_older_turn() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();
    // 会话 s1 开始
    machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None);
    // 会话 s1 开启新轮次 new-turn
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("s1"),
        Some("new-turn"),
    );

    // 携带旧轮次 id 的 Stop 事件应被判定为迟到，忽略
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("old-turn")),
        HookEventDecision::Ignore
    );
    // 携带当前轮次 id 的 Stop 事件应被正常接纳，切换为 Idle
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("new-turn")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

// 测试用例：验证 Codex 的“Goal 模式”下，无需再次提交用户提示即可用新轮次恢复运行
#[test]
/// 验证 Codex 目标模式可由新轮次进度恢复而无需再次用户提示。
fn codex_goal_mode_resumes_with_a_new_turn_without_another_user_prompt() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();
    // 会话开始
    machine.apply_event(AiTool::Codex, "SessionStart", Some("goal-session"), None);
    // 提交用户提示，开启轮次 turn-1
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("goal-session"),
        Some("turn-1"),
    );
    // 轮次 turn-1 停止，期望切换为 Idle
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-1"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );

    // 已停止轮次的迟到进度和完成事件都不能重新激活展示。
    for event in ["PreToolUse", "PostToolUse"] {
        // 针对已停止的 turn-1，重放工具调用前/后事件，期望均被忽略
        assert_eq!(
            machine.apply_event(AiTool::Codex, event, Some("goal-session"), Some("turn-1"),),
            HookEventDecision::Ignore,
            "同一已停止轮次的迟到事件 {event} 应被抑制"
        );
    }

    // Goal 模式恢复不会再次提交用户 prompt；新 turn 的首个工作进度必须能
    // 建立隐式轮次并恢复 Running。
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("goal-session"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 旧轮次 turn-1 的迟到 Stop 仍应被忽略
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-1"),),
        HookEventDecision::Ignore
    );
    // 新轮次 turn-2 的 Stop 应正常生效，切换回 Idle
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-2"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 新轮次 turn-3 的权限请求，期望切换为 Asking
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PermissionRequest",
            Some("goal-session"),
            Some("turn-3"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
    );
    // 轮次 turn-3 停止，期望回到 Idle
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("goal-session"), Some("turn-3"),),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

// 测试用例：验证查询当前展示状态不会重放“释放”动作（避免误清理新建立的展示）
#[test]
/// 验证当前展示查询只暴露状态，不会重放先前的释放动作。
fn current_display_transition_exposes_state_without_replaying_release() {
    // 构造一台全新的状态机
    let mut machine = HookStateMachine::default();
    // 初始状态下（无任何会话），当前展示转换应为 None
    assert_eq!(machine.current_display_transition(), None);

    // 提交用户提示，开启一个 Running 轮次
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("replay-session"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 此时查询当前展示转换，应返回 Running（供新上线设备补齐状态）
    assert_eq!(
        machine.current_display_transition(),
        Some(HookTransition::Display(HookBehavior::Running))
    );

    // 会话结束，展示位被释放
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "SessionEnd",
            Some("replay-session"),
            Some("turn-1"),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );
    // 已释放状态下查询当前展示转换，应返回 None（不重放 Release 动作）
    assert_eq!(machine.current_display_transition(), None);
}
