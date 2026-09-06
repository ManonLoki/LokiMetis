//! 会话内部轮次事件的状态推进与迟到事件拦截。

use std::time::Duration;

use super::{
    HookEventDecision, HookPhase, HookSessionState, HookStateMachine, HookTransition,
    phase_decision,
    session::{StoppedTurnDecision, stopped_turn_decision, turn_is_stale},
};
use crate::agent_hooks::{HookBehavior, HookEventKind};

impl HookStateMachine {
    /// 推进一条轮次事件，只在最终聚合阶段变化时产出展示迁移。
    pub(super) fn apply_turn_event(
        &mut self,
        session_key: String,
        has_session_id: bool,
        event_kind: HookEventKind,
        turn_id: Option<&str>,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if !self.accept_ended_session_event(&session_key, event_kind, turn_id) {
            return HookEventDecision::Ignore;
        }
        if matches!(
            event_kind,
            HookEventKind::WorkCompletion(_) | HookEventKind::UnscopedWorkCompletion
        ) && !self.sessions.contains_key(&session_key)
        {
            return HookEventDecision::Ignore;
        }
        if has_session_id {
            self.remove_workspace_placeholder();
        }
        self.ensure_capacity_for(&session_key);
        let session = self
            .sessions
            .entry(session_key)
            .or_insert_with(|| HookSessionState {
                last_seen_at: observed_at,
                ..HookSessionState::default()
            });

        if matches!(event_kind, HookEventKind::UnscopedWorkCompletion) {
            if session.turn_active {
                session.last_seen_at = observed_at;
            }
            return HookEventDecision::Ignore;
        }
        if matches!(event_kind, HookEventKind::UnscopedWorkStart(_)) {
            if session.turn_active {
                session.last_seen_at = observed_at;
                return HookEventDecision::Ignore;
            }
            if session.turn_id.is_some() {
                return HookEventDecision::Ignore;
            }
            session.turn_active = true;
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        if event_kind == HookEventKind::WorkStart {
            if session.is_retired_turn(turn_id) {
                return HookEventDecision::Ignore;
            }
            session.start_explicit_turn(turn_id, observed_at);
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        let transition = event_kind.transition();
        if event_kind == HookEventKind::Stop
            || matches!(event_kind, HookEventKind::TerminalState(_))
        {
            if !session.turn_active
                && turn_id.is_none()
                && session.is_retired_turn(session.turn_id.as_deref())
            {
                return HookEventDecision::Ignore;
            }
            if session.is_retired_turn(turn_id) {
                return HookEventDecision::Ignore;
            }
            if turn_is_stale(session, turn_id) {
                session.retire_observed_turn(turn_id);
                return HookEventDecision::Ignore;
            }
            session.finish_turn(turn_id);
            session.phase = match transition {
                HookTransition::Display(HookBehavior::Error) => HookPhase::Error,
                _ => HookPhase::Idle,
            };
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if session.is_retired_turn(turn_id) || session.is_quarantined_turn(turn_id) {
            return HookEventDecision::Ignore;
        }
        if matches!(event_kind, HookEventKind::WorkCompletion(_)) && !session.turn_active {
            return HookEventDecision::Ignore;
        }
        let starts_new_implicit_turn = match stopped_turn_decision(session, event_kind, turn_id) {
            StoppedTurnDecision::SuppressLateEvent => return HookEventDecision::Ignore,
            StoppedTurnDecision::StartNewTurn => true,
            StoppedTurnDecision::NotApplicable => false,
        };
        if turn_is_stale(session, turn_id) && !starts_new_implicit_turn {
            session.quarantine_observed_turn(turn_id);
            return HookEventDecision::Ignore;
        }
        if starts_new_implicit_turn {
            session.start_turn(turn_id);
        }
        session.phase = next_turn_phase(session, event_kind, transition, turn_id);
        session.last_seen_at = observed_at;
        phase_decision(previous, self.aggregate_phase())
    }

    /// 墓碑只接受携带新轮次 ID 的明确工作起点。
    fn accept_ended_session_event(
        &mut self,
        session_key: &str,
        event_kind: HookEventKind,
        turn_id: Option<&str>,
    ) -> bool {
        let Some(session) = self
            .sessions
            .get_mut(session_key)
            .filter(|session| session.ended)
        else {
            return true;
        };
        if event_kind != HookEventKind::WorkStart
            || turn_id.is_none()
            || session.is_retired_turn(turn_id)
        {
            return false;
        }
        session.ended = false;
        true
    }
}

/// 根据已接纳事件更新单个会话阶段及轮次活跃性。
fn next_turn_phase(
    session: &mut HookSessionState,
    event_kind: HookEventKind,
    transition: HookTransition,
    turn_id: Option<&str>,
) -> HookPhase {
    match transition {
        HookTransition::Release => HookPhase::Released,
        HookTransition::Display(HookBehavior::Idle) => HookPhase::Idle,
        HookTransition::Display(HookBehavior::Running) => {
            session.continue_turn(turn_id);
            HookPhase::Running
        }
        HookTransition::Display(HookBehavior::Asking) => {
            session.continue_turn(turn_id);
            HookPhase::Asking
        }
        HookTransition::Display(HookBehavior::Error) => {
            if matches!(event_kind, HookEventKind::WorkProgress(_)) {
                session.continue_turn(turn_id);
            } else {
                session.finish_turn(turn_id);
            }
            HookPhase::Error
        }
    }
}
