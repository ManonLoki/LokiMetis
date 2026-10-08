//! AIMonitor 源状态机对应场景的 LokiMetis 回归测试。

// 引入被测状态机相关类型：事件决策结果、状态机本体、转场类型
use super::{HookEventDecision, HookStateMachine, HookTransition};
// 引入 AI 工具枚举与 Hook 展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// 测试：迟到的 workspaceOpen 不能在真实会话之后遗留一个占位记录
#[test]
/// 验证迟到的工作区打开不会遗留真实会话占位符。
fn late_workspace_open_cannot_leave_a_real_session_placeholder() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();

    // 真实会话开始，应转发空闲展示态
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionStart", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 无会话归属的 workspaceOpen 事件，此时真实会话已存在，应被忽略
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Ignore
    );
    // 真实会话结束，应转发释放态
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
    // 会话结束之后再次收到迟到的 workspaceOpen，也应被忽略，
    // 不能重新占位成永久 Idle
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
        HookEventDecision::Ignore
    );
}

// 测试：真实工作事件也能替换掉工作区占位
#[test]
/// 验证真实工作事件同样会替换工作区占位符。
fn a_real_work_event_also_replaces_the_workspace_placeholder() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 先建立工作区占位（无会话归属）
    machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None);

    // 真实的提交事件到来，应接管占位并转发运行态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 会话结束应正常转发释放态
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "sessionEnd", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Release)
    );
}

// 测试：Cursor 工具调用失败在同一 generation 内是可恢复的
#[test]
/// 验证 Cursor 工具失败后同一 generation 仍可继续恢复工作。
fn cursor_tool_failure_is_recoverable_within_the_same_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始 generation-1 的工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );

    // 工具调用失败，应转发错误展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "postToolUseFailure",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    // 同一 generation 内继续调用工具，应恢复为运行展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "preToolUse",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
    // 最终正常停止，转发空闲展示态
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

// 测试：Cursor 子代理错误既不会结束也不会重新标注父 generation
#[test]
/// 验证子代理错误不会结束或改写父级 generation。
fn cursor_subagent_error_does_not_end_or_relabel_the_parent_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始父 generation-1 的工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );

    // 子代理以错误状态结束，携带的是子代理自己的 generation id，
    // 不应影响父轮次，事件应被忽略
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "subagentStop",
            Some("session-1"),
            Some("subagent-generation"),
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    // 父 generation-1 的正常停止仍应正常生效，转发空闲态
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

// 测试：不带 generation 的进度事件应沿用已知的当前 generation
#[test]
/// 验证无 generation 的 Cursor 进度沿用已知当前 generation。
fn cursor_progress_without_generation_keeps_the_known_current_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始 generation-1 的工作
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );

    // 不带 id 的进度事件本身不产生对外转发（视为对当前 generation 的延续）
    assert_eq!(
        machine.apply_event(AiTool::Cursor, "preToolUse", Some("session-1"), None),
        HookEventDecision::Ignore
    );
    // 之后对 generation-1 的显式 stop 仍应正常生效，说明进度事件
    // 没有把当前 generation 弄丢
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

// 测试：已退休的 Cursor generation 不能替换或停止最新的 generation
#[test]
/// 验证已退役 generation 不能替换或停止最新 generation。
fn retired_cursor_generations_cannot_replace_or_stop_the_latest_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始 generation-1
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    // 停止 generation-1，使其被退休
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-1"),
    );
    // 开始新的 generation-2
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    // 已退休的 generation-1 再次尝试提交，应被忽略
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    // 已退休的 generation-1 尝试以错误状态停止，也应被忽略
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    // 当前 generation-2 的正常停止应正常生效
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

// 测试：显式的 generation 替换会退休上一个 generation
#[test]
/// 验证显式 generation 替换会退役前一 generation。
fn explicit_generation_replacement_retires_the_previous_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始 generation-1
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-1"),
    );
    // 显式开始 generation-2，替换掉 generation-1
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    // 已被替换（退休）的 generation-1 再次尝试提交，应被忽略
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
}

// 测试：陈旧的终止事件只会退休其自身 generation，不会打断当前工作
#[test]
/// 验证陈旧终止事件只退役自身 generation，不停止当前工作。
fn stale_terminal_event_retires_its_generation_without_stopping_current_work() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始当前正在进行的 generation-2
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    // 针对更早的 generation-1（陈旧）的错误终止事件应被忽略，
    // 但会把 generation-1 记入已退休集合
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Ignore
    );
    // 当前 generation-2 的正常停止应不受影响，正常生效
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 已退休的 generation-1 之后再尝试提交，仍应被忽略
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
}

// 测试：不匹配的进度事件不会阻塞后续的显式工作开始事件
#[test]
/// 验证不匹配的进度不会阻止后续显式工作开始。
fn mismatched_progress_does_not_block_a_later_explicit_work_start() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 开始当前 generation-2
    machine.apply_event(
        AiTool::Cursor,
        "beforeSubmitPrompt",
        Some("session-1"),
        Some("generation-2"),
    );

    // 针对不匹配的 generation-1 的进度事件应被忽略（隔离观察）
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "preToolUse",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Ignore
    );
    // 停止当前 generation-2
    machine.apply_event(
        AiTool::Cursor,
        "stop",
        Some("session-1"),
        Some("generation-2"),
    );
    // 之后针对 generation-1 的显式提交（工作开始）应被正常接受，
    // 说明之前的隔离观察没有把它标记为不可用
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some("generation-1"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

// 测试：首次观测到的终止事件会退休其对应的 generation
#[test]
/// 验证首次观测到的终止事件会登记并退役对应 generation。
fn first_observed_terminal_event_retires_its_generation() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();

    // 状态机对该会话一无所知时，第一次直接收到错误终止事件，
    // 应被接受并转发错误展示态
    assert_eq!(
        machine.apply_event_with_status(
            AiTool::Cursor,
            "stop",
            Some("session-1"),
            Some("generation-1"),
            Some("error"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
    );
    // 遍历两种典型的“继续工作”事件类型，验证针对已退休 generation-1
    // 的迟到事件均应被忽略
    for event in ["preToolUse", "beforeSubmitPrompt"] {
        assert_eq!(
            machine.apply_event(
                AiTool::Cursor,
                event,
                Some("session-1"),
                Some("generation-1"),
            ),
            HookEventDecision::Ignore,
            "terminal generation must reject late {event}"
        );
    }
    // 全新的 generation-2 应能正常开始，转发运行展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Cursor,
            "preToolUse",
            Some("session-1"),
            Some("generation-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

// 测试：单个会话内已退休的 generation 历史记录数量是有上限的
#[test]
/// 验证每个会话保存的退役 generation 历史具有明确上限。
fn retired_generation_history_is_bounded_per_session() {
    // 引入会话模块中定义的“每会话最大已退休轮次数”常量
    use super::session::MAX_RETIRED_TURNS_PER_SESSION;

    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 循环生成超过上限数量的 generation，每个都开始后立即停止（退休）
    for index in 0..=MAX_RETIRED_TURNS_PER_SESSION {
        // 构造形如 "generation-N" 的轮次 id
        let turn = format!("generation-{index}");
        // 开始该 generation 的工作
        machine.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("session-1"),
            Some(&turn),
        );
        // 立即停止，使其进入已退休集合
        machine.apply_event(AiTool::Cursor, "stop", Some("session-1"), Some(&turn));
    }

    // 取出该会话记录的已退休轮次集合
    let retired = &machine.sessions["session-1"].retired_turn_ids;
    // 断言集合长度被限制在上限值
    assert_eq!(retired.len(), MAX_RETIRED_TURNS_PER_SESSION);
    // 断言最早的 generation-0 已被淘汰出集合
    assert!(!retired.iter().any(|turn| turn == "generation-0"));
    // 断言较新的 generation-1 仍保留在集合中
    assert!(retired.iter().any(|turn| turn == "generation-1"));
    // 断言最新的 generation-256 也保留在集合中
    assert!(retired.iter().any(|turn| turn == "generation-256"));
}

// 测试：过期的墓碑允许使用相同 id 建立一个全新的会话
#[test]
/// 验证墓碑过期后同一会话 ID 可以建立全新会话。
fn expired_tombstone_allows_a_fresh_session_with_the_same_id() {
    // 局部引入 Duration，仅本测试使用
    use std::time::Duration;
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 在 t=1s 时刻，session-1 结束，产生墓碑
    machine.apply_event_with_status_at(
        AiTool::Cursor,
        "sessionEnd",
        Some("session-1"),
        None,
        None,
        Duration::from_secs(1),
    );
    // 以“非活跃 12 秒、墓碑 10 秒”为阈值触发过期清理，
    // 由于此刻并无更多存活会话，聚合结果应为 Ignore（无对外可见变化）
    assert_eq!(
        machine.expire_inactive_sessions(Duration::from_secs(12), Duration::from_secs(10)),
        HookEventDecision::Ignore
    );

    // 墓碑已过期后，session-1 的全新 sessionStart 应被正常接受，
    // 转发空闲展示态
    assert_eq!(
        machine.apply_event_with_status_at(
            AiTool::Cursor,
            "sessionStart",
            Some("session-1"),
            None,
            None,
            Duration::from_secs(13),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
}
