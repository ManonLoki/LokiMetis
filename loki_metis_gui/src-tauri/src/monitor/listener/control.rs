//! Hook listener 的运行时启用策略与共享状态同步。

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use loki_metis_core::{AiTool, HookTransition, normalize_enabled_ai_tools};

use super::super::listener_state;
use super::HookRelayStatus;

/// 当前允许进入状态机的工具集合，以及每次启停变化后的单调代数。
#[derive(Debug)]
pub(super) struct HookListenerPolicy {
    pub(super) enabled_tools: HashSet<AiTool>,
    pub(super) generations: HashMap<AiTool, u64>,
}

impl HookListenerPolicy {
    pub(super) fn new(enabled_tools: &[AiTool]) -> Self {
        Self {
            enabled_tools: normalize_enabled_ai_tools(enabled_tools)
                .into_iter()
                .collect(),
            generations: AiTool::ALL.into_iter().map(|tool| (tool, 0)).collect(),
        }
    }

    /// 当前工具启用时返回其代数，供事件入队时捕获。
    pub(super) fn admission(&self, tool: AiTool) -> Option<u64> {
        self.enabled_tools
            .contains(&tool)
            .then(|| self.generations.get(&tool).copied().unwrap_or_default())
    }

    /// 只有工具仍启用且代数未变化时，排队事件才可推进状态机。
    pub(super) fn admits_generation(&self, tool: AiTool, generation: u64) -> bool {
        self.admission(tool) == Some(generation)
    }
}

/// Tauri 保存设置时同步更新的 listener 启用门禁。
pub struct HookListenerControl {
    pub(super) policy: Arc<RwLock<HookListenerPolicy>>,
    pub(super) status: Arc<RwLock<HookRelayStatus>>,
}

impl HookListenerControl {
    /// 替换启用集合；任何启停变化都会使该工具的旧排队事件和状态机失效。
    pub fn replace_enabled_tools(&self, enabled_tools: &[AiTool]) -> bool {
        let next = normalize_enabled_ai_tools(enabled_tools)
            .into_iter()
            .collect::<HashSet<_>>();
        let removed = {
            let mut policy = write_hook_listener_policy(&self.policy);
            let removed = policy
                .enabled_tools
                .difference(&next)
                .copied()
                .collect::<Vec<_>>();
            for tool in AiTool::ALL {
                if policy.enabled_tools.contains(&tool) != next.contains(&tool) {
                    let generation = policy.generations.entry(tool).or_default();
                    *generation = generation.saturating_add(1);
                }
            }
            policy.enabled_tools = next;
            removed
        };
        release_disabled_pet_states(&self.status, &removed)
    }
}

/// 清空被禁用工具当前占用的槽位；没有活跃槽位时不制造虚假 revision。
fn release_disabled_pet_states(
    status: &Arc<RwLock<HookRelayStatus>>,
    disabled_tools: &[AiTool],
) -> bool {
    if disabled_tools.is_empty() {
        return false;
    }
    let mut current = write_hook_relay_status(status);
    let mut changed = false;
    for &tool in disabled_tools {
        if current
            .pet_states
            .iter()
            .any(|state| state.tool == tool && state.behavior.is_some())
        {
            let mut revision = current.revision;
            listener_state::apply_pet_transition(
                &mut current.pet_states,
                &mut revision,
                tool,
                0,
                HookTransition::Release,
            );
            current.revision = revision;
            changed = true;
        }
    }
    if current
        .last_event
        .as_ref()
        .is_some_and(|event| disabled_tools.contains(&event.tool))
    {
        current.last_event = None;
    }
    if changed {
        current.last_error = None;
    }
    changed
}

/// 读取启用策略；锁污染时保留现状并记录诊断，避免静默放宽门禁。
pub(super) fn hook_listener_policy(
    policy: &RwLock<HookListenerPolicy>,
) -> RwLockReadGuard<'_, HookListenerPolicy> {
    policy.read().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook listener policy lock"
        );
        poisoned.into_inner()
    })
}

/// 写入启用策略；锁污染时仍以调用方的新设置恢复。
fn write_hook_listener_policy(
    policy: &RwLock<HookListenerPolicy>,
) -> RwLockWriteGuard<'_, HookListenerPolicy> {
    policy.write().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook listener policy lock for update"
        );
        poisoned.into_inner()
    })
}

/// 锁曾被 panic 污染时记录错误并恢复内部状态，避免后续所有 Hook 被静默丢弃。
pub(super) fn write_hook_relay_status(
    status: &RwLock<HookRelayStatus>,
) -> RwLockWriteGuard<'_, HookRelayStatus> {
    status.write().unwrap_or_else(|poisoned| {
        tracing::error!(
            target: "loki_metis::hook_listener",
            "recovering poisoned hook relay status lock"
        );
        poisoned.into_inner()
    })
}
