//! 串行推进 Hook 生命周期并映射桌宠展示状态。

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use loki_metris_core::{AiTool, HookEventDecision, HookStateMachine, HookTransition};
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval_at};

use super::super::listener_state;
use super::control::{
    HookListenerPolicy, HookListenerShutdown, hook_listener_policy, write_hook_relay_status,
};
use super::{HookRelayLastEvent, HookRelayStatus, IncomingHookEvent, QueuedHookEvent};

/// 孤儿会话回收时间；超时只回落 Idle，不猜测为 SessionEnd。
const HOOK_SESSION_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 即使没有新事件也执行会话回收的周期。
const HOOK_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(1);

/// 串行推进每个工具的生命周期，避免连接任务调度顺序直接改写桌宠状态。
pub(super) async fn run_hook_worker(
    mut receiver: mpsc::Receiver<QueuedHookEvent>,
    status: Arc<RwLock<HookRelayStatus>>,
    config_dir: std::path::PathBuf,
    policy: Arc<RwLock<HookListenerPolicy>>,
    mut shutdown: HookListenerShutdown,
) {
    let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
    let mut machine_generations = HashMap::<AiTool, u64>::new();
    let clock_started_at = Instant::now();
    let first_sweep = tokio::time::Instant::now() + HOOK_SESSION_SWEEP_INTERVAL;
    let mut sweep = interval_at(first_sweep, HOOK_SESSION_SWEEP_INTERVAL);
    sweep.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            queued = receiver.recv() => {
                let Some(queued) = queued else {
                    break;
                };
                if !hook_listener_policy(&policy).admits_generation(queued.event.tool, queued.generation)
                {
                    continue;
                }
                if machine_generations.get(&queued.event.tool) != Some(&queued.generation) {
                    state_machines.remove(&queued.event.tool);
                    machine_generations.insert(queued.event.tool, queued.generation);
                }
                if process_hook_event_checked(
                    queued.event,
                    clock_started_at.elapsed(),
                    &mut state_machines,
                    &status,
                    &config_dir,
                    Some((&policy,queued.generation)),
                ) {
                    // 状态 revision 已同步提交，原生视图直接读取该权威快照。
                }
            }
            _ = sweep.tick() => {
                let current_generations = hook_listener_policy(&policy);
                state_machines.retain(|tool, _| {
                    machine_generations
                        .get(tool)
                        .is_some_and(|generation| current_generations.admits_generation(*tool, *generation))
                });
                machine_generations.retain(|tool, generation| {
                    current_generations.admits_generation(*tool, *generation)
                });
                drop(current_generations);
                if expire_inactive_hook_sessions_checked(
                    &mut state_machines,
                    clock_started_at.elapsed(),
                    &status,
                    &config_dir,
                    Some((&policy,&machine_generations)),
                ) {
                    // 状态 revision 已同步提交，原生视图直接读取该权威快照。
                }
            }
        }
    }
}

/// 推进一条事件并只把 Forward(Display/Release) 应用到桌宠状态。
/// 直接入口供已隔离的状态机回归使用；生产经 checked 入口复核启用代数。
#[cfg(test)]
pub(super) fn process_hook_event(event:IncomingHookEvent,observed_at:Duration,machines:&mut HashMap<AiTool,HookStateMachine>,status:&Arc<RwLock<HookRelayStatus>>,config_dir:&Path)->bool {
    process_hook_event_checked(event,observed_at,machines,status,config_dir,None)
}

/// 复核启用代数后推进事件并写入状态。
fn process_hook_event_checked(
    event: IncomingHookEvent,
    observed_at: Duration,
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    status: &Arc<RwLock<HookRelayStatus>>,
    config_dir: &Path,
    admission:Option<(&RwLock<HookListenerPolicy>,u64)>,
) -> bool {
    let mut candidate_machine = state_machines.get(&event.tool).cloned().unwrap_or_default();
    let decision = candidate_machine.apply_event_with_status_at(
        event.tool,
        &event.hook_type,
        event.session_id.as_deref(),
        event.turn_id.as_deref(),
        event.status.as_deref(),
        observed_at,
    );
    let slot_result = matches!(
        &decision,
        HookEventDecision::Forward(HookTransition::Display(_))
    )
    .then(|| listener_state::configured_slot_index(config_dir, event.tool));
    // 位置文件在锁外准备，只有纯内存提交持有 policy→status 锁序。
    let admitted=admission.map(|(policy,generation)|(hook_listener_policy(policy),generation));
    if admitted.as_ref().is_some_and(|(policy,generation)|!policy.admits_generation(event.tool,*generation)) {return false}
    let mut pet_state_changed = false;
    let mut commit_machine = false;
    let mut current = write_hook_relay_status(status);
    current.received_count += 1;
    current.last_event = Some(HookRelayLastEvent {
        tool: event.tool,
        hook_type: event.hook_type.clone(),
    });
    match decision {
        HookEventDecision::Forward(HookTransition::Release) => {
            let mut revision = current.revision;
            listener_state::apply_pet_transition(
                &mut current.pet_states,
                &mut revision,
                event.tool,
                0,
                HookTransition::Release,
            );
            current.revision = revision;
            current.last_error = None;
            pet_state_changed = true;
            commit_machine = true;
        }
        HookEventDecision::Forward(transition @ HookTransition::Display(_)) => {
            match slot_result.expect("forward transition resolves a slot before locking") {
                Ok(slot_index) => {
                    let mut revision = current.revision;
                    listener_state::apply_pet_transition(
                        &mut current.pet_states,
                        &mut revision,
                        event.tool,
                        slot_index,
                        transition,
                    );
                    current.revision = revision;
                    current.last_error = None;
                    pet_state_changed = true;
                    commit_machine = true;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "loki_metris::hook_listener",
                        tool = ?event.tool,
                        "hook event could not resolve its configured slot"
                    );
                    current.failed_count += 1;
                    current.last_error = Some(error);
                }
            }
        }
        HookEventDecision::Ignore => {
            current.last_error = None;
            commit_machine = true;
        }
        HookEventDecision::Unsupported => {
            tracing::warn!(
                target: "loki_metris::hook_listener",
                tool = ?event.tool,
                hook_type = %event.hook_type,
                "unsupported hook event reached listener"
            );
            current.failed_count += 1;
            current.last_error = Some(format!("unsupported hook type: {}", event.hook_type));
            commit_machine = true;
        }
    }
    drop(current);
    if commit_machine {
        state_machines.insert(event.tool, candidate_machine);
    }
    pet_state_changed
}

/// 定期回收孤儿会话，并应用状态机要求的 Idle 回落。
/// 无生产门禁的过期入口仅用于隔离状态机回归。
#[cfg(test)]
pub(super) fn expire_inactive_hook_sessions(machines:&mut HashMap<AiTool,HookStateMachine>,observed_at:Duration,status:&Arc<RwLock<HookRelayStatus>>,config_dir:&Path)->bool {
    expire_inactive_hook_sessions_checked(machines,observed_at,status,config_dir,None)
}

/// 复核启用代数后回收失活会话。
fn expire_inactive_hook_sessions_checked(
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    observed_at: Duration,
    status: &Arc<RwLock<HookRelayStatus>>,
    config_dir: &Path,
    admission:Option<(&RwLock<HookListenerPolicy>,&HashMap<AiTool,u64>)>,
) -> bool {
    let prepared = state_machines
        .iter()
        .map(|(&tool, machine)| {
            let mut candidate = machine.clone();
            let decision =
                candidate.expire_inactive_sessions(observed_at, HOOK_SESSION_INACTIVITY_TIMEOUT);
            (tool, candidate, decision)
        })
        .collect::<Vec<_>>();
    let mut transitions = Vec::new();
    for (tool, candidate, decision) in prepared {
        match decision {
            HookEventDecision::Forward(transition) => {
                transitions.push((tool, candidate, transition));
            }
            HookEventDecision::Ignore | HookEventDecision::Unsupported => {
                state_machines.insert(tool, candidate);
            }
        }
    }
    if transitions.is_empty() {
        return false;
    }
    let mut resolved = transitions
        .into_iter()
        .map(|(tool, candidate, transition)| {
            (
                tool,
                candidate,
                transition,
                matches!(transition, HookTransition::Display(_))
                    .then(|| listener_state::configured_slot_index(config_dir, tool)),
            )
        })
        .collect::<Vec<_>>();
    let admitted=admission.map(|(policy,generations)|(hook_listener_policy(policy),generations));
    if let Some((policy,generations))=&admitted {resolved.retain(|(tool,_,_,_)|generations.get(tool).is_some_and(|generation|policy.admits_generation(*tool,*generation)));}
    let mut pet_state_changed = false;
    let mut committed = Vec::new();
    let mut current = write_hook_relay_status(status);
    for (tool, candidate, transition, slot_result) in resolved {
        match transition {
            HookTransition::Release => {
                let mut revision = current.revision;
                listener_state::apply_pet_transition(
                    &mut current.pet_states,
                    &mut revision,
                    tool,
                    0,
                    HookTransition::Release,
                );
                current.revision = revision;
                current.last_error = None;
                pet_state_changed = true;
                committed.push((tool, candidate));
            }
            HookTransition::Display(_) => {
                match slot_result.expect("display transition resolves a slot before locking") {
                    Ok(slot_index) => {
                        let mut revision = current.revision;
                        listener_state::apply_pet_transition(
                            &mut current.pet_states,
                            &mut revision,
                            tool,
                            slot_index,
                            transition,
                        );
                        current.revision = revision;
                        current.last_error = None;
                        pet_state_changed = true;
                        committed.push((tool, candidate));
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "loki_metris::hook_listener",
                            tool = ?tool,
                            "expired hook session could not resolve its configured slot"
                        );
                        current.failed_count += 1;
                        current.last_error = Some(error);
                    }
                }
            }
        }
    }
    drop(current);
    for (tool, machine) in committed {
        state_machines.insert(tool, machine);
    }
    pet_state_changed
}
