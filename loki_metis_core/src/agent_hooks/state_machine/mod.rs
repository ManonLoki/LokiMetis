//! Hook 生命周期状态机：按工具和会话拦截重复、迟到与失去时序意义的事件。

use std::{collections::HashMap, time::Duration};

use super::{
    AiTool, HookBehavior, HookEventKind, HookTransition, event_kind, forwards_every_event,
    release_settle_delay, session_start_revives_tombstone,
};

mod lifecycle;
mod session;
mod turn;

#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests_cursor_reentry;
#[cfg(test)]
mod tests_cursor_timing;
#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod tests_session_resume;

use lifecycle::DEFAULT_SESSION_KEY;
use session::{HookPhase, HookSessionState, session_eviction_priority};

/// Hook 事件经过生命周期归约后的唯一处理决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEventDecision {
    /// 应把展示迁移交给 adapter 执行。
    Forward(HookTransition),
    /// 重复、迟到或聚合状态未变化，保持当前展示。
    Ignore,
    /// 当前工具协议不认识该事件。
    Unsupported,
}

/// 单个工具最多保留的会话数，结束墓碑也计入上限。
pub(crate) const MAX_TRACKED_HOOK_SESSIONS: usize = 256;

/// 单个 AI 工具的进程内 Hook 生命周期状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookStateMachine {
    sessions: HashMap<String, HookSessionState>,
}

impl HookStateMachine {
    /// 测试入口：不携带会话和轮次推进一次事件。
    #[cfg(test)]
    pub fn apply(&mut self, tool: AiTool, event: &str) -> HookEventDecision {
        self.apply_event(tool, event, None, None)
    }

    /// 测试入口：不携带 status 和观察时间推进一次事件。
    #[cfg(test)]
    pub fn apply_event(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status(tool, event, session_id, turn_id, None)
    }

    /// 测试入口：固定零时刻并携带原生 status 推进一次事件。
    #[cfg(test)]
    pub fn apply_event_with_status(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status_at(tool, event, session_id, turn_id, status, Duration::ZERO)
    }

    /// 使用调用方提供的单调经过时间推进一次 Hook 事件。
    pub fn apply_event_with_status_at(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
        observed_at: Duration,
    ) -> HookEventDecision {
        let Some(event_kind) = event_kind(tool, event, status) else {
            return HookEventDecision::Unsupported;
        };
        if forwards_every_event(tool) {
            return HookEventDecision::Forward(event_kind.transition());
        }
        let previous = self.aggregate_phase();
        if event_kind == HookEventKind::WorkspaceStart {
            return self.apply_workspace_start(observed_at, previous);
        }
        let session_key = session_id.unwrap_or(DEFAULT_SESSION_KEY).to_owned();
        if event_kind == HookEventKind::SessionEnd {
            return self.apply_session_end(
                session_key,
                turn_id,
                release_settle_delay(tool),
                observed_at,
                previous,
            );
        }
        if event_kind == HookEventKind::SessionStart {
            return self.apply_session_start(
                session_key,
                session_id.is_some(),
                session_start_revives_tombstone(tool),
                observed_at,
                previous,
            );
        }
        self.apply_turn_event(
            session_key,
            session_id.is_some(),
            event_kind,
            turn_id,
            observed_at,
            previous,
        )
    }

    /// 清理到期会话；活跃展示超时只回落 Idle，不把超时猜成 SessionEnd。
    pub fn expire_inactive_sessions(
        &mut self,
        observed_at: Duration,
        timeout: Duration,
    ) -> HookEventDecision {
        let previous = self.aggregate_phase();
        self.sessions
            .retain(|_, session| observed_at.saturating_sub(session.last_seen_at) < timeout);
        let next = self.aggregate_phase();
        if next != HookPhase::Released {
            return phase_decision(previous, next);
        }
        match previous {
            HookPhase::Running | HookPhase::Asking | HookPhase::Error => {
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
            }
            HookPhase::Released | HookPhase::Idle => HookEventDecision::Ignore,
        }
    }

    /// 返回当前可重放的展示迁移；从未建立或已释放时返回 `None`。
    pub fn current_display_transition(&self) -> Option<HookTransition> {
        let phase = self.aggregate_phase();
        (phase != HookPhase::Released).then(|| phase_transition(phase))
    }

    /// 为新会话腾出有界空间，优先淘汰墓碑和非活跃记录。
    fn ensure_capacity_for(&mut self, session_key: &str) {
        if self.sessions.contains_key(session_key) {
            return;
        }
        while self.sessions.len() >= MAX_TRACKED_HOOK_SESSIONS {
            let Some(eviction_key) = self
                .sessions
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    session_eviction_priority(left)
                        .cmp(&session_eviction_priority(right))
                        .then_with(|| left.last_seen_at.cmp(&right.last_seen_at))
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.sessions.remove(&eviction_key);
        }
    }

    /// 按 Asking、Error、Running、Idle 的优先级聚合全部存活会话。
    fn aggregate_phase(&self) -> HookPhase {
        [
            HookPhase::Asking,
            HookPhase::Error,
            HookPhase::Running,
            HookPhase::Idle,
        ]
        .into_iter()
        .find(|phase| {
            self.sessions
                .values()
                .any(|session| !session.ended && session.phase == *phase)
        })
        .unwrap_or(HookPhase::Released)
    }

    /// 返回当前跟踪的会话数量，供容量与幽灵会话回归使用。
    #[cfg(test)]
    fn tracked_session_count(&self) -> usize {
        self.sessions.len()
    }
}

/// 只在聚合展示状态实际变化时向 adapter 发出迁移。
fn phase_decision(previous: HookPhase, next: HookPhase) -> HookEventDecision {
    if previous == next {
        return HookEventDecision::Ignore;
    }
    HookEventDecision::Forward(phase_transition(next))
}

/// 把内部聚合阶段映射为公开展示迁移。
fn phase_transition(phase: HookPhase) -> HookTransition {
    match phase {
        HookPhase::Released => HookTransition::Release,
        HookPhase::Idle => HookTransition::Display(HookBehavior::Idle),
        HookPhase::Running => HookTransition::Display(HookBehavior::Running),
        HookPhase::Asking => HookTransition::Display(HookBehavior::Asking),
        HookPhase::Error => HookTransition::Display(HookBehavior::Error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造带稳定时间的状态机事件，简化生命周期断言。
    fn apply(
        machine: &mut HookStateMachine,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        second: u64,
    ) -> HookEventDecision {
        apply_tool(machine, AiTool::Codex, event, session_id, turn_id, second)
    }

    /// 构造指定工具的带稳定时间事件。
    fn apply_tool(
        machine: &mut HookStateMachine,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        second: u64,
    ) -> HookEventDecision {
        machine.apply_event_with_status_at(
            tool,
            event,
            session_id,
            turn_id,
            None,
            Duration::from_secs(second),
        )
    }

    /// 全新状态机不创建任何桌宠展示。
    #[test]
    fn fresh_machine_has_no_initial_display() {
        let machine = HookStateMachine::default();
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 重复或迟到事件不得覆盖已经停止的可见状态。
    #[test]
    fn duplicate_and_late_events_do_not_overwrite_the_visible_state() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 1),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 2),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(
                &mut machine,
                "UserPromptSubmit",
                Some("session-1"),
                Some("turn-1"),
                3,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("session-1"), Some("turn-1"), 4,),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(
                &mut machine,
                "PostToolUse",
                Some("session-1"),
                Some("turn-1"),
                5,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 6),
            HookEventDecision::Forward(HookTransition::Release)
        );
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 多会话共享工具槽位时，只有最后一个会话结束才释放。
    #[test]
    fn only_the_last_live_session_releases_the_tool_slot() {
        let mut machine = HookStateMachine::default();
        let _ = apply(&mut machine, "SessionStart", Some("session-1"), None, 1);
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-2"), None, 2),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 3),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-2"), None, 4),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }

    /// 未知事件返回不支持，且不得建立展示。
    #[test]
    fn unknown_event_is_unsupported_without_creating_a_display() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(&mut machine, "UnknownEvent", None, None, 1),
            HookEventDecision::Unsupported
        );
        assert_eq!(machine.current_display_transition(), None);
    }

    /// 监控中途启动时，孤立完成事件不得创建幽灵展示。
    #[test]
    fn orphan_completion_is_ignored_without_creating_a_display() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply(
                &mut machine,
                "PostToolUse",
                Some("late-session"),
                Some("turn-1"),
                1,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(machine.current_display_transition(), None);
        assert_eq!(
            apply(
                &mut machine,
                "PreToolUse",
                Some("live-session"),
                Some("turn-1"),
                2,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
    }

    /// WorkBuddy 保持 AIMonitor 的逐事件直通行为，不参与重复抑制。
    #[test]
    fn workbuddy_forwards_repeated_supported_events() {
        let mut machine = HookStateMachine::default();
        for second in [1, 2] {
            assert_eq!(
                machine.apply_event_with_status_at(
                    AiTool::WorkBuddy,
                    "PreToolUse",
                    Some("session-1"),
                    Some("turn-1"),
                    None,
                    Duration::from_secs(second),
                ),
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
            );
        }
    }

    /// 会话墓碑拦截迟到事件，但显式 SessionStart 可恢复并保留旧轮次历史。
    #[test]
    fn ended_tombstone_rejects_late_events_and_resumes_explicitly() {
        let mut machine = HookStateMachine::default();
        let _ = apply(
            &mut machine,
            "UserPromptSubmit",
            Some("session-1"),
            Some("turn-1"),
            1,
        );
        assert_eq!(
            apply(&mut machine, "SessionEnd", Some("session-1"), None, 2),
            HookEventDecision::Forward(HookTransition::Release)
        );
        for event in [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
        ] {
            assert_eq!(
                apply(&mut machine, event, Some("session-1"), Some("turn-1"), 100,),
                HookEventDecision::Ignore,
                "墓碑应拦截迟到事件 {event}"
            );
        }
        assert_eq!(
            apply(&mut machine, "SessionStart", Some("session-1"), None, 101),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply(
                &mut machine,
                "PreToolUse",
                Some("session-1"),
                Some("turn-1"),
                102,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(
                &mut machine,
                "UserPromptSubmit",
                Some("session-1"),
                Some("turn-2"),
                103,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
    }

    /// Goal 续跑允许新轮次进度恢复，同时持续拦截旧轮次迟到事件。
    #[test]
    fn goal_mode_resumes_only_with_a_new_turn() {
        let mut machine = HookStateMachine::default();
        let _ = apply(&mut machine, "SessionStart", Some("goal"), None, 1);
        let _ = apply(
            &mut machine,
            "UserPromptSubmit",
            Some("goal"),
            Some("turn-1"),
            2,
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-1"), 3),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        for event in ["PreToolUse", "PostToolUse"] {
            assert_eq!(
                apply(&mut machine, event, Some("goal"), Some("turn-1"), 4),
                HookEventDecision::Ignore
            );
        }
        assert_eq!(
            apply(&mut machine, "PreToolUse", Some("goal"), Some("turn-2"), 5,),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-1"), 6),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply(&mut machine, "Stop", Some("goal"), Some("turn-2"), 7),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
    }

    /// 最后一个活跃会话超时只回落 Idle，不把超时误判为明确 Release。
    #[test]
    fn expiring_last_active_session_falls_back_to_idle() {
        let mut machine = HookStateMachine::default();
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("active"),
            Some("turn-1"),
            0,
        );
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(11), Duration::from_secs(10)),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(12), Duration::from_secs(10)),
            HookEventDecision::Ignore
        );
    }

    /// 多会话展示按 Asking、Error、Running、Idle 聚合，逐层退出后正确降级。
    #[test]
    fn aggregate_phase_uses_aimonitor_priority() {
        let mut machine = HookStateMachine::default();
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionStart",
                Some("idle"),
                None,
                1,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "UserPromptSubmit",
                Some("running"),
                Some("turn-running"),
                2,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("error"),
            Some("turn-error"),
            3,
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "PostToolUseFailure",
                Some("error"),
                Some("turn-error"),
                4,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
        let _ = apply_tool(
            &mut machine,
            AiTool::ClaudeCode,
            "UserPromptSubmit",
            Some("asking"),
            Some("turn-asking"),
            5,
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "PermissionRequest",
                Some("asking"),
                Some("turn-asking"),
                6,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("asking"),
                None,
                7,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("error"),
                None,
                8,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "Stop",
                Some("running"),
                Some("turn-running"),
                9,
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("running"),
                None,
                10,
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            apply_tool(
                &mut machine,
                AiTool::ClaudeCode,
                "SessionEnd",
                Some("idle"),
                None,
                11,
            ),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }
}
