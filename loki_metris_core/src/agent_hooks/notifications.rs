//! 通知仅消费已经被生命周期状态机接受的真实迁移，不把启动或失活回收视作结束。

use super::{AiTool, HookBehavior, HookEventDecision, HookEventKind, HookTransition, event_kind};

/// 用户关注的三种 Hook 状态变化；运行中与初始空闲不产生通知。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookNotification {
    /// 工具进入等待用户答复的状态。
    Asking,
    /// 工具实际完成或结束当前工作。
    Ended,
    /// 工具进入错误状态。
    Error,
}

/// 对比工具的实际展示前态与已接受事件；重复状态、忽略事件及错误后的释放均静默。
pub fn hook_notification(
    tool: AiTool,
    event: &str,
    status: Option<&str>,
    previous: Option<HookBehavior>,
    decision: HookEventDecision,
) -> Option<HookNotification> {
    let HookEventDecision::Forward(transition) = decision else { return None; };
    // 关闭会话本身不能证明完成；Cursor另有明确的最终结果可用。
    if event_kind(tool, event, status) == Some(HookEventKind::SessionEnd)
        && matches!(transition, HookTransition::Release | HookTransition::Display(HookBehavior::Idle)) {
        return match (tool, status) {
            (AiTool::Cursor, Some("error")) if previous != Some(HookBehavior::Error) => Some(HookNotification::Error),
            (AiTool::Cursor, Some("completed")) if matches!(previous, Some(HookBehavior::Running | HookBehavior::Asking | HookBehavior::Error)) => Some(HookNotification::Ended),
            _ => None,
        };
    }
    match transition {
        HookTransition::Display(HookBehavior::Asking) if previous != Some(HookBehavior::Asking) => Some(HookNotification::Asking),
        HookTransition::Display(HookBehavior::Error) if previous != Some(HookBehavior::Error) => Some(HookNotification::Error),
        HookTransition::Display(HookBehavior::Idle) | HookTransition::Release
            if (matches!(previous, Some(HookBehavior::Running | HookBehavior::Asking))
                && matches!(event_kind(tool, event, status), Some(HookEventKind::Stop | HookEventKind::SessionEnd | HookEventKind::TerminalState(HookBehavior::Idle))))
                || (previous == Some(HookBehavior::Error) && matches!(event_kind(tool, event, status), Some(HookEventKind::Stop | HookEventKind::TerminalState(HookBehavior::Idle)))) => Some(HookNotification::Ended),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 三类状态只在真实迁移时通知；重复、启动、迟到忽略与错误后的释放不能误报。
    #[test]
    fn hook_notifications_classify_changes_and_ignore_nonterminal_idle() {
        use HookBehavior::*;
        let forward = |behavior| HookEventDecision::Forward(HookTransition::Display(behavior));
        let classify = |event, before, after| hook_notification(AiTool::Codex, event, None, before, after);
        assert_eq!(classify("PermissionRequest", Some(Running), forward(Asking)), Some(HookNotification::Asking));
        assert_eq!(classify("PermissionRequest", Some(Asking), forward(Asking)), None);
        assert_eq!(classify("PostToolUseFailure", Some(Running), forward(Error)), Some(HookNotification::Error));
        assert_eq!(classify("PostToolUseFailure", Some(Error), forward(Error)), None);
        assert_eq!(classify("Stop", Some(Running), forward(Idle)), None);
        assert_eq!(classify("Stop", Some(Asking), forward(Idle)), None);
        assert_eq!(classify("Stop", Some(Error), forward(Idle)), None);
        assert_eq!(hook_notification(AiTool::Cursor, "stop", Some("completed"), Some(Running), forward(Idle)), Some(HookNotification::Ended));
        assert_eq!(classify("SessionEnd", Some(Running), HookEventDecision::Forward(HookTransition::Release)), None);
        assert_eq!(classify("SessionEnd", Some(Idle), HookEventDecision::Forward(HookTransition::Release)), None);
        assert_eq!(classify("SessionEnd", Some(Error), HookEventDecision::Forward(HookTransition::Release)), None);
        assert_eq!(classify("SessionStart", Some(Running), forward(Idle)), None);
        assert_eq!(classify("Stop", None, forward(Idle)), None);
        assert_eq!(classify("Stop", Some(Running), HookEventDecision::Ignore), None);
    }
    /// 直通协议也保存通知前态，失活回收会清除运行态而不伪造结束事件。
    #[test]
    fn notification_behavior_tracks_passthrough_and_inactivity() {
        use std::time::Duration;
        let mut direct = super::super::HookStateMachine::default();
        direct.apply_event_with_status_at(AiTool::OpenCode, "permission.asked", None, None, None, Duration::ZERO);
        assert_eq!(direct.notification_behavior(), Some(HookBehavior::Asking));
        let previous = direct.notification_behavior();
        let decision = direct.apply_event_with_status_at(AiTool::OpenCode, "session.idle", None, None, None, Duration::ZERO);
        assert_eq!(hook_notification(AiTool::OpenCode, "session.idle", None, previous, decision), Some(HookNotification::Ended));
        assert_eq!(direct.notification_behavior(), Some(HookBehavior::Idle));
        let mut scoped = super::super::HookStateMachine::default();
        scoped.apply_event_with_status_at(AiTool::Codex, "UserPromptSubmit", Some("s"), Some("t"), None, Duration::ZERO);
        assert_eq!(scoped.notification_behavior(), Some(HookBehavior::Running));
        scoped.expire_inactive_sessions(Duration::from_secs(1801), Duration::from_secs(1800));
        assert_eq!(scoped.notification_behavior(), Some(HookBehavior::Idle));
    }
}
