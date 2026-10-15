//! 单个会话的轮次历史、墓碑与迟到事件判定。

use std::{collections::VecDeque, time::Duration};

use super::super::HookEventKind;

/// 单个会话的有界生命周期状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HookSessionState {
    pub(super) phase: HookPhase,
    pub(super) turn_active: bool,
    pub(super) turn_id: Option<String>,
    pub(super) retired_turn_ids: VecDeque<String>,
    pub(super) quarantined_turn_ids: VecDeque<String>,
    /// 已结束但上游未提供轮次 ID；仅用于去重迟到的终止事件。
    pub(super) finished_unidentified_turn: bool,
    /// 已明确终结或取消无ID轮次；只有新的工作起点才能重启。
    pub(super) requires_explicit_restart: bool,
    /// 上游Stop尚未通过其它Hook门禁，允许同轮次继续执行。
    pub(super) continuable_stop: bool,
    pub(super) ended: bool,
    pub(super) last_seen_at: Duration,
    pub(super) explicit_turn_started_at: Option<Duration>,
    /// 每个会话至多256个未解决交互；其它工具完成不消耗这些等待。
    pub(super) waiting: Vec<HookInteraction>,
    /// 超过有界容量后保守保留询问态，直到明确的新轮次或终态。
    pub(super) waiting_overflow: bool,
    /// 有界的在途工具调用，用于关联缺少 tool_use_id 的权限请求。
    pub(super) active_interactions: Vec<HookInteraction>,
    /// 活动集合容量溢出后禁用唯一候选推断，避免把遗漏调用错配到已知调用。
    pub(super) active_interactions_overflow: bool,
}

/// 等待用户的工具/澄清归属，与会话轮次身份分离。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HookInteraction {
    pub(super) agent_id: Option<String>,
    pub(super) interaction_id: Option<String>,
    pub(super) tool_name: Option<String>,
    /// MCP澄清等待单独归属，缺少调用ID的回执只能解除这一类等待。
    pub(super) elicitation: bool,
    /// UI通知仅补充观察；稍后到达的真实澄清请求可以替换这一泛化等待。
    pub(super) notification_only: bool,
}

pub(super) const MAX_RETIRED_TURNS_PER_SESSION: usize = 256;

impl HookSessionState {
    /// 判断传入轮次是否已明确结束或判旧。
    pub(super) fn is_retired_turn(&self, turn_id: Option<&str>) -> bool {
        turn_id.is_some_and(|turn_id| {
            self.retired_turn_ids
                .iter()
                .any(|retired| retired == turn_id)
        })
    }

    /// 判断传入轮次是否因先后关系不明而被隔离。
    pub(super) fn is_quarantined_turn(&self, turn_id: Option<&str>) -> bool {
        turn_id.is_some_and(|turn_id| {
            self.quarantined_turn_ids
                .iter()
                .any(|quarantined| quarantined == turn_id)
        })
    }

    /// 开启新轮次并退休被替代的可识别轮次。
    pub(super) fn start_turn(&mut self, turn_id: Option<&str>) {
        self.waiting.clear();
        self.waiting_overflow = false;
        self.active_interactions.clear();
        self.active_interactions_overflow = false;
        self.finished_unidentified_turn = false;
        self.requires_explicit_restart = false;
        self.continuable_stop = false;
        if self.turn_id.as_deref() != turn_id {
            if let Some(previous) = self.turn_id.take() {
                self.retire_turn(previous);
            }
            self.turn_id = turn_id.map(str::to_owned);
        }
        self.turn_active = true;
    }

    /// 以明确工作起点开启轮次，并解除此前的临时隔离。
    pub(super) fn start_explicit_turn(&mut self, turn_id: Option<&str>, observed_at: Duration) {
        self.quarantined_turn_ids.clear();
        self.start_turn(turn_id);
        self.explicit_turn_started_at = Some(observed_at);
    }

    /// 延续当前轮次；无当前 ID 时才从事件补全。
    pub(super) fn continue_turn(&mut self, turn_id: Option<&str>) {
        if self.turn_id.is_none() {
            self.turn_id = turn_id.map(str::to_owned);
        }
        self.turn_active = true;
    }

    /// 结束轮次并把其 ID 记入有界退休队列。
    pub(super) fn finish_turn(&mut self, turn_id: Option<&str>) {
        self.waiting.clear();
        self.waiting_overflow = false;
        self.active_interactions.clear();
        self.active_interactions_overflow = false;
        self.continuable_stop = false;
        let finished = turn_id.map(str::to_owned).or_else(|| self.turn_id.clone());
        if let Some(finished) = finished {
            self.turn_id = Some(finished.clone());
            self.retire_turn(finished);
        }
        self.turn_active = false;
        self.quarantined_turn_ids.clear();
        self.explicit_turn_started_at = None;
    }

    /// 把可选的传入轮次记为已退休。
    pub(super) fn retire_observed_turn(&mut self, turn_id: Option<&str>) {
        if let Some(turn_id) = turn_id {
            self.retire_turn(turn_id.to_owned());
        }
    }

    /// 把尚不能判断先后的轮次加入有界隔离队列。
    pub(super) fn quarantine_observed_turn(&mut self, turn_id: Option<&str>) {
        let Some(turn_id) = turn_id else {
            return;
        };
        if self.is_retired_turn(Some(turn_id))
            || self
                .quarantined_turn_ids
                .iter()
                .any(|quarantined| quarantined == turn_id)
        {
            return;
        }
        if self.quarantined_turn_ids.len() == MAX_RETIRED_TURNS_PER_SESSION {
            self.quarantined_turn_ids.pop_front();
        }
        self.quarantined_turn_ids.push_back(turn_id.to_owned());
    }

    /// 去重地退休一个轮次，并同时解除其隔离状态。
    fn retire_turn(&mut self, turn_id: String) {
        self.quarantined_turn_ids
            .retain(|quarantined| quarantined != &turn_id);
        if self
            .retired_turn_ids
            .iter()
            .any(|retired| retired == &turn_id)
        {
            return;
        }
        if self.retired_turn_ids.len() == MAX_RETIRED_TURNS_PER_SESSION {
            self.retired_turn_ids.pop_front();
        }
        self.retired_turn_ids.push_back(turn_id);
    }
}

/// 会话聚合使用的内部展示阶段；默认表示从未建立展示。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HookPhase {
    #[default]
    Released,
    Idle,
    Running,
    Asking,
    Error,
}

/// 已停止会话收到进度类事件时的时序判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StoppedTurnDecision {
    NotApplicable,
    SuppressLateEvent,
    StartNewTurn,
}

/// 返回会话的淘汰优先级：墓碑、停止会话、活跃会话。
pub(super) fn session_eviction_priority(session: &HookSessionState) -> u8 {
    if session.ended {
        0
    } else if !session.turn_active {
        1
    } else {
        2
    }
}

/// 有当前轮次时，不同的传入 ID 视为旧轮次。
pub(super) fn turn_is_stale(session: &HookSessionState, incoming_turn_id: Option<&str>) -> bool {
    incoming_turn_id.is_some_and(|incoming| {
        session
            .turn_id
            .as_deref()
            .is_some_and(|current| current != incoming)
    })
}

/// 判断停止后的进度是迟到事件，还是 Goal 续跑形成的新隐式轮次。
pub(super) fn stopped_turn_decision(
    session: &HookSessionState,
    event_kind: HookEventKind,
    turn_id: Option<&str>,
) -> StoppedTurnDecision {
    if session.turn_active
        || !matches!(
            event_kind,
            HookEventKind::WorkProgress(_) | HookEventKind::State(_)
        )
    {
        return StoppedTurnDecision::NotApplicable;
    }
    if turn_id.is_none() {
        if session.continuable_stop {
            return StoppedTurnDecision::StartNewTurn;
        }
        return if session.turn_id.is_some() || session.requires_explicit_restart {
            StoppedTurnDecision::SuppressLateEvent
        } else {
            StoppedTurnDecision::NotApplicable
        };
    }
    if session.is_retired_turn(turn_id) || session.is_quarantined_turn(turn_id) {
        StoppedTurnDecision::SuppressLateEvent
    } else if session.continuable_stop
        || session.turn_id.is_none()
        || turn_is_stale(session, turn_id)
    {
        StoppedTurnDecision::StartNewTurn
    } else {
        StoppedTurnDecision::SuppressLateEvent
    }
}
