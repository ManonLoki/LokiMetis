//! Hook 生命周期状态机：按工具和会话拦截重复、迟到与失去时序意义的事件。

use std::{collections::HashMap, time::Duration};

use super::{
    AiTool, HookBehavior, HookEventKind, HookTransition, event_kind, forwards_every_event,
    release_settle_delay, session_start_revives_tombstone,
};

mod interaction;
mod lifecycle;
mod session;
mod turn;

#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests_claude_compatible;
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
    /// 按会话 ID 索引的会话状态，结束墓碑也计入 MAX_TRACKED_HOOK_SESSIONS 上限；缺少会话 ID 的事件共用默认键。
    sessions: HashMap<String, HookSessionState>,
    /// 最近已接受的展示行为；直通协议同样维护，不依赖宿主桌宠槽位。
    notification_behavior: Option<HookBehavior>,
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
        self.apply_event_with_scope_at(
            tool,
            event,
            session_id,
            turn_id,
            status,
            super::HookEventScope::default(),
            observed_at,
        )
    }

    /// 真实宿主附带交互归属；无关并行工具或子 Agent 收尾不能解除其它用户询问。
    #[allow(clippy::too_many_arguments)]
    pub fn apply_event_with_scope_at(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
        scope: super::HookEventScope<'_>,
        observed_at: Duration,
    ) -> HookEventDecision {
        let decision =
            self.apply_event_inner(tool, event, session_id, turn_id, status, scope, observed_at);
        self.remember_transition(decision)
    }

    /// 返回工具自身前态，通知去重不受其它工具覆盖同一展示槽位影响。
    pub fn notification_behavior(&self) -> Option<HookBehavior> {
        self.notification_behavior
    }

    /// 只记录已接受迁移；忽略/不支持的事件不能改变通知前态。
    fn remember_transition(&mut self, decision: HookEventDecision) -> HookEventDecision {
        if let HookEventDecision::Forward(transition) = decision {
            self.notification_behavior = match transition {
                HookTransition::Display(behavior) => Some(behavior),
                HookTransition::Release => None,
            };
        }
        decision
    }

    /// 原有会话和轮次归约，不改变直通协议及生命周期语义。
    #[allow(clippy::too_many_arguments)]
    fn apply_event_inner(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
        scope: super::HookEventScope<'_>,
        observed_at: Duration,
    ) -> HookEventDecision {
        let Some(event_kind) = event_kind(tool, event, status) else {
            return HookEventDecision::Unsupported;
        };
        // Claude 全局工具 Hooks 也在子 Agent 中运行，session_id 仍指向父会话。
        // agent_id 是明确的子 Agent 归属；局部失败不能改写父轮次或伪造父任务错误。
        // 协议依据：https://code.claude.com/docs/en/hooks#common-input-fields
        if tool == AiTool::ClaudeCode
            && scope.agent_id.is_some_and(|id| !id.trim().is_empty())
            && matches!(
                event_kind,
                HookEventKind::WorkProgress(HookBehavior::Error)
                    | HookEventKind::WorkCompletion(HookBehavior::Error)
                    | HookEventKind::TerminalState(HookBehavior::Error)
            )
        {
            return self.apply_child_failure(
                session_id.unwrap_or(DEFAULT_SESSION_KEY),
                turn_id,
                scope,
                event_kind,
                interaction::interaction_action(event, status),
                observed_at,
            );
        }
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
            scope,
            interaction::interaction_action(event, status),
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
            return self.remember_transition(phase_decision(previous, next));
        }
        let decision = match previous {
            HookPhase::Running | HookPhase::Asking | HookPhase::Error => {
                HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
            }
            HookPhase::Released | HookPhase::Idle => HookEventDecision::Ignore,
        };
        self.remember_transition(decision)
    }

    /// 返回当前可重放的展示迁移；从未建立或已释放时返回 `None`。
    #[cfg(any(test, feature = "test-support"))]
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - state_machine_tests.rs：本模块的单元测试
include!("state_machine_tests.rs");
