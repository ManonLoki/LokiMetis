//! AIMonitor 源状态机对应场景的 LokiMetis 回归测试。

// 引入被测状态机相关类型：事件决策结果、状态机本体、转场类型
use super::{HookEventDecision, HookStateMachine, HookTransition};
// 引入 AI 工具枚举与 Hook 展示行为枚举
use crate::agent_hooks::{AiTool, HookBehavior};

// 测试：会话恢复后仍保留旧的轮次历史（不会因恢复而重置退休记录）
#[test]
/// 验证恢复既有会话时会保留旧轮次历史。
fn resumed_session_keeps_old_turn_history() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 建立 session-1 下 turn-1 的提交
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("session-1"),
        Some("turn-1"),
    );
    // 结束该会话（不携带轮次 id）
    machine.apply_event(AiTool::Codex, "SessionEnd", Some("session-1"), None);

    // 会话显式重新开始，应转发空闲展示态（墓碑被恢复）
    assert_eq!(
        machine.apply_event(AiTool::Codex, "SessionStart", Some("session-1"), None),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // 旧的 turn-1 再次出现工具调用前事件，应被忽略——
    // 说明恢复会话并未清空之前的轮次退休历史
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-1"),
        ),
        HookEventDecision::Ignore
    );
    // 全新的 turn-2 提交应被正常接受，转发运行展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}

// 测试：Codex 的 Goal 模式进度事件可以在“超越”前一个 stop 之后继续恢复
#[test]
/// 验证 Codex 目标进度越过上一停止事件后可以恢复运行。
fn codex_goal_progress_can_resume_after_overtaking_the_previous_stop() {
    // 构造一台全新的默认状态机
    let mut machine = HookStateMachine::default();
    // 建立 session-1 下 turn-1 的提交（当前记录的轮次）
    machine.apply_event(
        AiTool::Codex,
        "UserPromptSubmit",
        Some("session-1"),
        Some("turn-1"),
    );

    // turn-2 的进度事件先到达（此时 turn-1 仍是“当前”轮次），
    // 由于 turn-1 尚未结束，turn-2 被视为不匹配/陈旧，应被忽略（隔离观察）
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Ignore
    );
    // turn-1 正常停止，应转发空闲展示态
    assert_eq!(
        machine.apply_event(AiTool::Codex, "Stop", Some("session-1"), Some("turn-1")),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
    );
    // turn-1 停止之后，turn-2 的进度事件再次到达，此时应被视为隐式新轮次的
    // 起点，正常接受并转发运行展示态
    assert_eq!(
        machine.apply_event(
            AiTool::Codex,
            "PreToolUse",
            Some("session-1"),
            Some("turn-2"),
        ),
        HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
    );
}
