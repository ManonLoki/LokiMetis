//! 会话与工作区生命周期事件的状态推进。

use std::time::Duration;

use super::{
    HookEventDecision, HookPhase, HookSessionState, HookStateMachine, HookTransition,
    phase_decision, session::turn_is_stale,
};

pub(super) const DEFAULT_SESSION_KEY: &str = "__default__";

impl HookStateMachine {
    /// 工作区启动只创建一次默认 Idle 占位，真实会话出现后不再复活它。
    pub(super) fn apply_workspace_start(
        &mut self,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if self.sessions.keys().any(|key| key != DEFAULT_SESSION_KEY) {
            return HookEventDecision::Ignore;
        }
        if let Some(existing) = self.sessions.get_mut(DEFAULT_SESSION_KEY) {
            if existing.ended {
                return HookEventDecision::Ignore;
            }
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.sessions.insert(
            DEFAULT_SESSION_KEY.to_owned(),
            HookSessionState {
                phase: HookPhase::Idle,
                last_seen_at: observed_at,
                ..HookSessionState::default()
            },
        );
        phase_decision(previous, self.aggregate_phase())
    }

    /// 会话结束建立墓碑；只有最后一个存活会话结束时才释放展示位。
    pub(super) fn apply_session_end(
        &mut self,
        session_key: String,
        turn_id: Option<&str>,
        reorder_grace: Duration,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended)
        {
            return HookEventDecision::Ignore;
        }
        if turn_id.is_none()
            && !reorder_grace.is_zero()
            && self.sessions.get(&session_key).is_some_and(|session| {
                session.turn_active
                    && session.explicit_turn_started_at.is_some_and(|started_at| {
                        observed_at
                            .checked_sub(started_at)
                            .is_some_and(|elapsed| !elapsed.is_zero() && elapsed <= reorder_grace)
                    })
            })
        {
            return HookEventDecision::Ignore;
        }
        if let Some(session) = self.sessions.get_mut(&session_key)
            && session.turn_active
            && turn_is_stale(session, turn_id)
        {
            session.retire_observed_turn(turn_id);
            return HookEventDecision::Ignore;
        }
        if session_key != DEFAULT_SESSION_KEY {
            self.sessions.remove(DEFAULT_SESSION_KEY);
        }
        self.ensure_capacity_for(&session_key);
        let mut tombstone = self.sessions.remove(&session_key).unwrap_or_default();
        tombstone.finish_turn(turn_id);
        tombstone.phase = HookPhase::Released;
        tombstone.ended = true;
        tombstone.last_seen_at = observed_at;
        self.sessions.insert(session_key, tombstone);
        let next = self.aggregate_phase();
        if next == HookPhase::Released {
            return HookEventDecision::Forward(HookTransition::Release);
        }
        phase_decision(previous, next)
    }

    /// 会话开始创建或复活 Idle 会话，并由聚合状态决定是否需要展示迁移。
    pub(super) fn apply_session_start(
        &mut self,
        session_key: String,
        has_session_id: bool,
        revives_tombstone: bool,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        let is_ended = self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended);
        if is_ended && !revives_tombstone {
            return HookEventDecision::Ignore;
        }
        if has_session_id {
            self.sessions.remove(DEFAULT_SESSION_KEY);
        }
        if let Some(existing) = self.sessions.get_mut(&session_key) {
            if existing.ended {
                existing.ended = false;
                existing.phase = HookPhase::Idle;
                existing.turn_active = false;
            }
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Idle,
                last_seen_at: observed_at,
                ..HookSessionState::default()
            },
        );
        phase_decision(previous, self.aggregate_phase())
    }

    /// 真实会话事件接管展示位时移除工作区默认占位。
    pub(super) fn remove_workspace_placeholder(&mut self) {
        self.sessions.remove(DEFAULT_SESSION_KEY);
    }
}
