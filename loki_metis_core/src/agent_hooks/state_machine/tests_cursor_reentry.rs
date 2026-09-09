//! AIMonitor 源状态机对应场景的 LokiMetis 回归测试。

// 引入标准库的 Duration，用于构造带时间戳的测试事件
use std::time::Duration;

// 引入被测状态机相关类型：事件决策结果、聚合阶段、状态机本体、转场类型
use super::{HookEventDecision, HookPhase, HookStateMachine, HookTransition};
// 引入 AI 工具枚举与 Hook 展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// 测试：结束墓碑拒绝迟到的旧 generation 事件，但接受全新 generation 的事件
#[test]
/// 验证 Cursor 墓碑拒绝迟到开始，但允许新的显式 generation。
fn cursor_tombstone_rejects_late_start_but_accepts_a_new_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 先建立 session-1 下 generation-1 的一次提交（工作开始）
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    // 结束该会话，应触发释放转场，从而在状态机中留下“墓碑”
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );

    // 墓碑存在期间，无 generation 的 sessionStart 应被忽略
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    // 墓碑存在期间，旧 generation-1 的迟到提交事件也应被忽略
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    // 但全新的 generation-2 提交事件应被接受，视为新 epoch 的开始并转发运行态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

// 测试：针对另一 generation 的迟到结束事件不能终止当前正在进行的工作
#[test]
/// 验证其他 generation 的迟到结束不能终止当前工作。
fn delayed_cursor_end_for_another_generation_cannot_end_current_work() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 建立 session-1 下 generation-2 的提交（当前正在进行的工作）
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    // 针对 generation-1（并非当前 generation）的迟到结束事件应被忽略
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    // generation-2 的正常 stop 事件应正常生效，转发为空闲展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

// 测试：已停止的轮次收到不带 generation id 的进度事件时应被忽略
#[test]
/// 验证已停止轮次忽略不带 generation 标识的进度事件。
fn stopped_turn_ignores_progress_without_a_generation_id() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 先开始 generation-1 的工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    // 再让该 generation 正常停止
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-1"),
    );

    // 停止后收到不带 id 的 preToolUse 进度事件应被忽略，不应重新激活轮次
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "preToolUse", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    // 断言会话仍然没有活跃轮次
    assert!(!machine.sessions["session-1"].turn_active);
    // 断言会话阶段仍为 Idle
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Idle);
}

// 测试：Cursor 子代理事件永远不能占用/改写父 generation
#[test]
/// 验证 Cursor 子代理事件不会占用父级 generation。
fn cursor_subagent_events_never_claim_the_parent_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 先建立一次会话开始事件
    machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None);

    // 尚无父轮次时，子代理开始事件作为冷启动工作信号，应转发运行态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "subagentStart",
            Some("session-1"),
            Some("conversation-id"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 断言子代理事件并未把 conversation-id 写成父轮次 id
    assert_eq!(machine.sessions["session-1"].turn_id, None);
    // 子代理以错误状态结束时，不应影响父轮次，事件本身应被忽略
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "subagentStop",
            Some("session-1"),
            Some("conversation-id"),
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    // 之后父 generation-1 的真实进度事件到来
    machine.apply_event(
        AiTool::Cursor,
        "preToolUse",
        Some("session-1"),
        Some("generation-1"),
    );
    // 断言此时会话记录的轮次 id 才是 generation-1（未被子代理抢占）
    assert_eq!(
        machine.sessions["session-1"].turn_id.as_deref(),
        Some("generation-1")
    );
    // generation-1 正常停止应转发空闲态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}

// 测试：不带 id 的 Cursor 结束事件不能超越刚刚开始的 generation
#[test]
/// 验证无 ID 的 Cursor 结束事件不能越过刚开始的新 generation。
fn idless_cursor_end_cannot_overtake_a_just_started_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 在 t=1s 时刻，session-1 的 generation-1 开始提交
    machine.apply_event_with_status_at(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
        None,
        Duration::from_secs(1),
    );

    // t=1.1s 时刻收到不带 id 的 sessionEnd，比 generation-1 的开始晚但仍视为
    // 陈旧/不可信来源，应被忽略而不是提前结束当前工作
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-1"),
            None,
            None,
            Duration::from_millis(1_100),
        ),
        HookEventDecision::Ignore
    );
    // t=1.2s 时刻带正确 id 的 stop 事件应正常生效，转发空闲态
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            None,
            Duration::from_millis(1_200),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );

    // 另起一台状态机，验证“足够晚”的不带 id 结束事件应当被接受（已充分静置）
    let mut settled = HookStateMachine::default();
    // t=1s 时刻，session-2 的 generation-2 开始提交
    settled.apply_event_with_status_at(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-2"),
        Some("generation-2"),
        None,
        Duration::from_secs(1),
    );
    // t=2s 时刻（已经过去 1 秒，超过静置延迟）收到不带 id 的 sessionEnd，
    // 此时应被接受并转发释放态
    assert_eq!(
        settled.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionEnd",
            Some("session-2"),
            None,
            None,
            Duration::from_secs(2),
        ),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试：不带 id 的重复终止事件不能重新给已结束的 generation 打上新标签（如错误态）
#[test]
/// 验证无 ID 的重复终止事件不能重新标记已完成 generation。
fn idless_duplicate_terminal_cannot_relabel_a_finished_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始 generation-1 的工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    // 正常停止 generation-1（无错误状态）
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-1"),
    );
    // 之后收到一条不带 id、状态为 error 的重复 stop，不应把已经是 Idle 的
    // generation-1 重新打成 Error，事件应被忽略
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            None,
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    // 断言会话阶段仍保持 Idle
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Idle);

    // 开始新的 generation-2 工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );
    // 这次以错误状态正常结束 generation-2
    machine.apply_event_with_status(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-2"),
        Some("error"),
    );
    // 再收到一条不带 id 的普通 stop（无错误状态），不应把已经是 Error 的
    // generation-2 重新覆盖为 Idle，事件应被忽略
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "stop", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    // 断言会话阶段仍保持 Error
    assert_eq!(machine.sessions["session-1"].phase, HookPhase::Error);
}
