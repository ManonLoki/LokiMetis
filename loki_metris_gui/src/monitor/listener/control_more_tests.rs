// 本文件是 control.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：返回进程级 owner 是否仍持有指定测试任务。；清空被禁用工具当前占用的槽位；没有活跃槽位时不制造虚假 revision。；读取启用策略；锁污染时保留现状并记录诊断，避免静默放宽门禁。

/// 返回进程级 owner 是否仍持有指定测试任务。
#[cfg(test)]
fn retained_hook_listener_owns_task(registration_id: u64) -> bool {
    retained_hook_listener_owner()
        .shared
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .tasks
        .iter()
        .any(|task| task.registration_id == registration_id)
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
            target: "loki_metris::hook_listener",
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
            target: "loki_metris::hook_listener",
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
            target: "loki_metris::hook_listener",
            "recovering poisoned hook relay status lock"
        );
        poisoned.into_inner()
    })
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
