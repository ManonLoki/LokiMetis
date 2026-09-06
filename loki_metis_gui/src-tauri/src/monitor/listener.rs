//! 本机环回 Hook listener：接受受支持 AI 工具的最小信封并记账。

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
    time::{Duration, Instant},
};

use loki_metis_core::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_EPHEMERAL_PORT, HOOK_RELAY_INSTANCE_HEADER,
    HookEventDecision, HookStateMachine, HookTransition, MAX_NATIVE_HOOK_INPUT_BYTES,
    MinimalHookPayload, PetOverlayToolState, hook_relay_loopback_address, tool_from_slug,
};
use serde::Serialize;
use tauri::AppHandle;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::timeout;
use tokio::time::{MissedTickBehavior, interval_at};

/// listener 到状态机 worker 的有界事件队列容量。
const HOOK_EVENT_QUEUE_CAPACITY: usize = 256;
/// 孤儿会话回收时间；超时只回落 Idle，不猜测为 SessionEnd。
const HOOK_SESSION_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 即使没有新事件也执行会话回收的周期。
const HOOK_SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(1);
/// 状态机长期保存的会话 ID 最大字节数。
const MAX_HOOK_SESSION_ID_BYTES: usize = 512;
/// 状态机长期保存的轮次 ID 最大字节数。
const MAX_HOOK_TURN_ID_BYTES: usize = 512;
/// 状态标量最大字节数。
const MAX_HOOK_STATUS_BYTES: usize = 64;
/// 单条本机 HTTP 请求从建立连接到读完正文的最长时间。
const HOOK_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// listener 回写短响应的最长时间。
const HOOK_HTTP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

/// listener 接收时附带的启用代数；禁用或重启用后旧排队事件会失效。
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueuedHookEvent {
    event: IncomingHookEvent,
    generation: u64,
}

/// 当前允许进入状态机的工具集合，以及每次启停变化后的单调代数。
#[derive(Debug)]
struct HookListenerPolicy {
    enabled_tools: HashSet<AiTool>,
    generations: HashMap<AiTool, u64>,
}

impl HookListenerPolicy {
    fn new(enabled_tools: &[AiTool]) -> Self {
        Self {
            enabled_tools: enabled_tools.iter().copied().collect(),
            generations: AiTool::ALL.into_iter().map(|tool| (tool, 0)).collect(),
        }
    }

    /// 当前工具启用时返回其代数，供事件入队时捕获。
    fn admission(&self, tool: AiTool) -> Option<u64> {
        self.enabled_tools
            .contains(&tool)
            .then(|| self.generations.get(&tool).copied().unwrap_or_default())
    }

    /// 只有工具仍启用且代数未变化时，排队事件才可推进状态机。
    fn admits_generation(&self, tool: AiTool, generation: u64) -> bool {
        self.admission(tool) == Some(generation)
    }
}

/// Tauri 保存设置时同步更新的 listener 启用门禁。
pub struct HookListenerControl {
    policy: Arc<RwLock<HookListenerPolicy>>,
    status: Arc<RwLock<HookRelayStatus>>,
}

impl HookListenerControl {
    /// 替换启用集合；任何启停变化都会使该工具的旧排队事件和状态机失效。
    pub fn replace_enabled_tools(&self, enabled_tools: &[AiTool]) -> bool {
        let next = enabled_tools.iter().copied().collect::<HashSet<_>>();
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

/// 通过 HTTP 边界校验后送入生命周期状态机的完整事件。
#[derive(Clone, Debug, PartialEq, Eq)]
struct IncomingHookEvent {
    tool: AiTool,
    hook_type: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    status: Option<String>,
}

/// 工作台展示的最近一次 Hook 事件。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayLastEvent {
    /// 工具。
    pub tool: AiTool,
    /// 事件名。
    pub hook_type: String,
}

/// 本机 Hook 中继运行状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayStatus {
    /// 是否正在监听。
    pub listening: bool,
    /// 绑定地址。
    pub bind_address: String,
    /// 已接受事件数。
    pub received_count: u64,
    /// 失败数。
    pub failed_count: u64,
    /// 最近一次成功事件。
    pub last_event: Option<HookRelayLastEvent>,
    /// 桌宠状态的全局递增版本；每次有效展示或释放迁移都增长。
    pub revision: u64,
    /// 各位置最近一次展示或释放状态，供同位置按版本决胜。
    pub pet_states: Vec<PetOverlayToolState>,
    /// 最近一次错误。
    pub last_error: Option<String>,
}

impl Default for HookRelayStatus {
    fn default() -> Self {
        Self {
            listening: false,
            bind_address: hook_relay_loopback_address(HOOK_RELAY_EPHEMERAL_PORT),
            received_count: 0,
            failed_count: 0,
            last_event: None,
            revision: 0,
            pet_states: Vec::new(),
            last_error: None,
        }
    }
}

/// 启动环回 listener，返回可查询状态与可同步更新的启用门禁。
pub fn spawn_hook_listener(
    app: AppHandle,
    config_dir: PathBuf,
    enabled_tools: Vec<AiTool>,
) -> (Arc<RwLock<HookRelayStatus>>, HookListenerControl) {
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&enabled_tools)));
    let (sender, receiver) = mpsc::channel(HOOK_EVENT_QUEUE_CAPACITY);
    tauri::async_runtime::spawn(run_hook_worker(
        receiver,
        Arc::clone(&status),
        app,
        config_dir,
        Arc::clone(&policy),
    ));
    let shared = Arc::clone(&status);
    let listener_policy = Arc::clone(&policy);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_listener(Arc::clone(&shared), sender, listener_policy).await {
            tracing::error!(target: "loki_metis::hook_listener", %error, "hook listener stopped");
            let mut current = write_hook_relay_status(&shared);
            current.listening = false;
            current.last_error = Some(error);
        }
    });
    let control = HookListenerControl {
        policy,
        status: Arc::clone(&status),
    };
    (status, control)
}

/// 返回只允许操作系统分配端口的回环绑定地址。
fn hook_relay_bind_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, HOOK_RELAY_EPHEMERAL_PORT))
}

/// 始终绑定操作系统分配的回环空闲端口，不抢占其他监控程序的固定端口。
pub async fn bind_local_hook_relay_listener() -> std::io::Result<TcpListener> {
    TcpListener::bind(hook_relay_bind_addr()).await
}

/// 串行推进每个工具的生命周期，避免连接任务调度顺序直接改写桌宠状态。
async fn run_hook_worker(
    mut receiver: mpsc::Receiver<QueuedHookEvent>,
    status: Arc<RwLock<HookRelayStatus>>,
    app: AppHandle,
    config_dir: PathBuf,
    policy: Arc<RwLock<HookListenerPolicy>>,
) {
    let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
    let mut machine_generations = HashMap::<AiTool, u64>::new();
    let clock_started_at = Instant::now();
    let first_sweep = tokio::time::Instant::now() + HOOK_SESSION_SWEEP_INTERVAL;
    let mut sweep = interval_at(first_sweep, HOOK_SESSION_SWEEP_INTERVAL);
    sweep.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            queued = receiver.recv() => {
                let Some(queued) = queued else {
                    break;
                };
                if !hook_listener_policy(&policy)
                    .admits_generation(queued.event.tool, queued.generation)
                {
                    continue;
                }
                if machine_generations.get(&queued.event.tool) != Some(&queued.generation) {
                    state_machines.remove(&queued.event.tool);
                    machine_generations.insert(queued.event.tool, queued.generation);
                }
                if process_hook_event(
                    queued.event,
                    clock_started_at.elapsed(),
                    &mut state_machines,
                    &status,
                    &config_dir,
                ) {
                    super::pet_events::emit_pet_window_state_changed(&app);
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
                if expire_inactive_hook_sessions(
                    &mut state_machines,
                    clock_started_at.elapsed(),
                    &status,
                    &config_dir,
                ) {
                    super::pet_events::emit_pet_window_state_changed(&app);
                }
            }
        }
    }
}

/// 推进一条事件并只把 Forward(Display/Release) 应用到桌宠状态。
fn process_hook_event(
    event: IncomingHookEvent,
    observed_at: Duration,
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    status: &Arc<RwLock<HookRelayStatus>>,
    config_dir: &Path,
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
    .then(|| super::listener_state::configured_slot_index(config_dir, event.tool));
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
            super::listener_state::apply_pet_transition(
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
                    super::listener_state::apply_pet_transition(
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
                        target: "loki_metis::hook_listener",
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
                target: "loki_metis::hook_listener",
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
fn expire_inactive_hook_sessions(
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    observed_at: Duration,
    status: &Arc<RwLock<HookRelayStatus>>,
    config_dir: &Path,
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
    let resolved = transitions
        .into_iter()
        .map(|(tool, candidate, transition)| {
            (
                tool,
                candidate,
                transition,
                matches!(transition, HookTransition::Display(_))
                    .then(|| super::listener_state::configured_slot_index(config_dir, tool)),
            )
        })
        .collect::<Vec<_>>();
    let mut pet_state_changed = false;
    let mut committed = Vec::new();
    let mut current = write_hook_relay_status(status);
    for (tool, candidate, transition, slot_result) in resolved {
        match transition {
            HookTransition::Release => {
                let mut revision = current.revision;
                super::listener_state::apply_pet_transition(
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
                        super::listener_state::apply_pet_transition(
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
                            target: "loki_metis::hook_listener",
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

/// 绑定后的实际回环地址，供状态与 relay 投递共用。
pub fn bound_hook_relay_address(listener: &TcpListener) -> std::io::Result<String> {
    Ok(hook_relay_loopback_address(listener.local_addr()?.port()))
}

/// 绑定并接受本机 Hook POST。
async fn run_listener(
    status: Arc<RwLock<HookRelayStatus>>,
    sender: mpsc::Sender<QueuedHookEvent>,
    policy: Arc<RwLock<HookListenerPolicy>>,
) -> Result<(), String> {
    let listener = bind_local_hook_relay_listener()
        .await
        .map_err(|error| error.to_string())?;
    let bind_address = bound_hook_relay_address(&listener).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let rendezvous = super::relay::HookRelayRendezvous::new(port);
    let rendezvous_path =
        super::relay::hook_relay_rendezvous_path().map_err(|error| error.to_string())?;
    super::relay::persist_hook_relay_rendezvous_at(&rendezvous_path, &rendezvous)
        .map_err(|error| error.to_string())?;
    tracing::info!(
        target: "loki_metis::hook_listener",
        port,
        "hook listener started"
    );
    {
        let mut current = write_hook_relay_status(&status);
        current.listening = true;
        current.bind_address = bind_address;
    }
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let status = Arc::clone(&status);
        let sender = sender.clone();
        let policy = Arc::clone(&policy);
        let instance_id = rendezvous.instance_id.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = match timeout(
                HOOK_HTTP_REQUEST_TIMEOUT,
                handle_connection(&mut stream, &instance_id),
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(_) => fail("408 Request Timeout", "Request Timeout"),
            };
            let (response_status, response_body, authenticated) =
                enqueue_connection_outcome(outcome, &sender, &status, &policy);
            if let Err(error) = timeout(
                HOOK_HTTP_RESPONSE_TIMEOUT,
                write_http_response(
                    &mut stream,
                    response_status,
                    response_body,
                    authenticated.then_some(instance_id.as_str()),
                ),
            )
            .await
            .map_err(|_| "response timeout".to_owned())
            .and_then(|result| result.map_err(|error| error.to_string()))
            {
                tracing::warn!(
                    target: "loki_metis::hook_listener",
                    %error,
                    "failed to write hook listener response"
                );
            }
        });
    }
}

/// 把解析成功的事件放入有界队列，并为解析或背压失败记账。
fn enqueue_connection_outcome(
    outcome: ConnectionOutcome,
    sender: &mpsc::Sender<QueuedHookEvent>,
    status: &Arc<RwLock<HookRelayStatus>>,
    policy: &Arc<RwLock<HookListenerPolicy>>,
) -> (&'static str, &'static str, bool) {
    if !outcome.ok {
        tracing::warn!(
            target: "loki_metis::hook_listener",
            status = outcome.status,
            reason = outcome.body,
            "hook request rejected"
        );
        record_listener_failure(status, outcome.body);
        return (outcome.status, outcome.body, outcome.authenticated);
    }
    let Some(event) = outcome.event else {
        record_listener_failure(status, "Invalid payload");
        return ("400 Bad Request", "Invalid payload", outcome.authenticated);
    };
    let tool = event.tool;
    let hook_type = event.hook_type.clone();
    let Some(generation) = hook_listener_policy(policy).admission(tool) else {
        tracing::debug!(
            target: "loki_metis::hook_listener",
            tool = ?tool,
            %hook_type,
            "hook request ignored because the tool is disabled"
        );
        return (outcome.status, outcome.body, true);
    };
    match sender.try_send(QueuedHookEvent { event, generation }) {
        Ok(()) => {
            tracing::debug!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook request accepted"
            );
            (outcome.status, outcome.body, true)
        }
        Err(TrySendError::Full(_)) => {
            tracing::warn!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook event queue is full"
            );
            record_listener_failure(status, "Hook event queue is full");
            ("503 Service Unavailable", "Service Unavailable", true)
        }
        Err(TrySendError::Closed(_)) => {
            tracing::error!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook event worker is unavailable"
            );
            record_listener_failure(status, "Hook event worker is unavailable");
            ("503 Service Unavailable", "Service Unavailable", true)
        }
    }
}

/// 记录 HTTP 边界或队列失败，不伪造任何桌宠展示迁移。
fn record_listener_failure(status: &Arc<RwLock<HookRelayStatus>>, error: &str) {
    let mut current = write_hook_relay_status(status);
    current.failed_count += 1;
    current.last_error = Some(error.to_owned());
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
            super::listener_state::apply_pet_transition(
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
fn hook_listener_policy(
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
fn write_hook_relay_status(
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

pub(crate) struct ConnectionOutcome {
    ok: bool,
    authenticated: bool,
    status: &'static str,
    body: &'static str,
    event: Option<IncomingHookEvent>,
}

const HEADER_BYTE_BUDGET: usize = 8192;
const MAX_HOOK_HTTP_REQUEST_BYTES: usize = MAX_NATIVE_HOOK_INPUT_BYTES + HEADER_BYTE_BUDGET;

/// 读取一个 HTTP/1.1 请求并校验最小信封。
async fn handle_connection<R: AsyncRead + Unpin>(
    stream: &mut R,
    expected_instance_id: &str,
) -> ConnectionOutcome {
    match read_complete_http_request(stream).await {
        Ok(raw) => parse_hook_request(&raw, expected_instance_id),
        Err(()) => fail("400 Bad Request", "Bad Request"),
    }
}

/// 按请求头结束标记和 Content-Length 读完整 HTTP/1.1 请求，避免拆包后正文为空。
async fn read_complete_http_request<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ()> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 1024];
    loop {
        let read = reader.read(&mut tmp).await.map_err(|_| ())?;
        if read == 0 {
            return complete_request_len(&buf)
                .filter(|total| buf.len() >= *total)
                .map(|_| buf)
                .ok_or(());
        }
        if buf.len().saturating_add(read) > MAX_HOOK_HTTP_REQUEST_BYTES {
            return Err(());
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(total) = complete_request_len(&buf)
            && buf.len() >= total
        {
            buf.truncate(total);
            return Ok(buf);
        }
    }
}

/// 定位请求头结束位置（含 `\r\n\r\n`）。
fn header_end_index(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

/// 从已完整的请求头解析 Content-Length。
fn declared_content_length(headers: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(headers).ok()?;
    for line in text.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

/// 已声明的完整请求字节数；没有 Content-Length 时只接受当前已读到的头后正文。
fn complete_request_len(buf: &[u8]) -> Option<usize> {
    let header_end = header_end_index(buf)?;
    match declared_content_length(&buf[..header_end]) {
        Some(length) => Some(header_end.saturating_add(length)),
        None => Some(buf.len()),
    }
}

/// 解析 POST /api/hooks/{instance}/{slug} 的最小信封。
pub(crate) fn parse_hook_request(raw: &[u8], expected_instance_id: &str) -> ConnectionOutcome {
    let Some(header_end) = header_end_index(raw) else {
        return fail("400 Bad Request", "Bad Request");
    };
    let header_bytes = &raw[..header_end - 4];
    if !header_bytes.is_ascii() {
        return fail("400 Bad Request", "Bad Request");
    }
    let Ok(headers) = std::str::from_utf8(header_bytes) else {
        return fail("400 Bad Request", "Bad Request");
    };
    let body = &raw[header_end..];
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or("");
    let Some(path) = request_line
        .strip_prefix("POST ")
        .and_then(|rest| rest.split(' ').next())
    else {
        return fail("400 Bad Request", "Bad Request");
    };
    let expected_path_prefix = format!("/api/hooks/{expected_instance_id}/");
    let Some(slug) = path.strip_prefix(&expected_path_prefix) else {
        return fail("404 Not Found", "Not Found");
    };
    if slug.is_empty() || slug.contains('/') {
        return fail("404 Not Found", "Not Found");
    }
    let header_value = |expected_name: &str| {
        headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.eq_ignore_ascii_case(expected_name) {
                Some(value.trim().to_owned())
            } else {
                None
            }
        })
    };
    let Some(request_instance_id) = header_value(HOOK_RELAY_INSTANCE_HEADER) else {
        return fail("400 Bad Request", "Missing listener identity");
    };
    if request_instance_id != expected_instance_id {
        return fail("404 Not Found", "Not Found");
    }
    let Some(tool) = tool_from_slug(slug) else {
        return authenticated_fail("400 Bad Request", "Unknown tool");
    };
    let header_type = header_value(HOOK_EVENT_TYPE_HEADER);
    let payload: MinimalHookPayload = match serde_json::from_slice(body) {
        Ok(payload) => payload,
        Err(_) => return authenticated_fail("400 Bad Request", "Invalid payload"),
    };
    let hook_type = payload.hook_event_name.trim();
    if hook_type.is_empty() || hook_type.len() > 128 {
        return authenticated_fail("400 Bad Request", "Invalid hook type");
    }
    let Some(header_type) = header_type else {
        return authenticated_fail("400 Bad Request", "Missing hook type");
    };
    if header_type != hook_type {
        return authenticated_fail("400 Bad Request", "Header mismatch");
    }
    let Ok(session_id) =
        normalize_hook_context_field(payload.session_id, MAX_HOOK_SESSION_ID_BYTES)
    else {
        return authenticated_fail("400 Bad Request", "Invalid session id");
    };
    let Ok(turn_id) = normalize_hook_context_field(payload.turn_id, MAX_HOOK_TURN_ID_BYTES) else {
        return authenticated_fail("400 Bad Request", "Invalid turn id");
    };
    let Ok(status) = normalize_hook_context_field(payload.status, MAX_HOOK_STATUS_BYTES) else {
        return authenticated_fail("400 Bad Request", "Invalid status");
    };
    ConnectionOutcome {
        ok: true,
        authenticated: true,
        status: "202 Accepted",
        body: "Accepted",
        event: Some(IncomingHookEvent {
            tool,
            hook_type: hook_type.to_owned(),
            session_id,
            turn_id,
            status,
        }),
    }
}

/// 修剪可选上下文字段，空白规整为 None，超限则拒绝。
fn normalize_hook_context_field(
    value: Option<String>,
    max_bytes: usize,
) -> Result<Option<String>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > max_bytes {
        return Err(());
    }
    Ok(Some(value.to_owned()))
}

fn fail(status: &'static str, body: &'static str) -> ConnectionOutcome {
    ConnectionOutcome {
        ok: false,
        authenticated: false,
        status,
        body,
        event: None,
    }
}

/// 身份路径与请求头均匹配后的协议错误，可以安全回显当前实例身份。
fn authenticated_fail(status: &'static str, body: &'static str) -> ConnectionOutcome {
    ConnectionOutcome {
        ok: false,
        authenticated: true,
        status,
        body,
        event: None,
    }
}

/// 编码响应；只有已认证请求才携带当前实例头。
fn encode_http_response(
    status: &str,
    body: &str,
    authenticated_instance_id: Option<&str>,
) -> String {
    let identity_header = authenticated_instance_id
        .map(|instance_id| format!("{HOOK_RELAY_INSTANCE_HEADER}: {instance_id}\r\n"))
        .unwrap_or_default();
    format!(
        "HTTP/1.1 {status}\r\n{identity_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
    authenticated_instance_id: Option<&str>,
) -> std::io::Result<()> {
    let response = encode_http_response(status, body, authenticated_instance_id);
    stream.write_all(response.as_bytes()).await
}

#[cfg(test)]
#[path = "listener_protocol_tests.rs"]
mod protocol_tests;

#[cfg(test)]
#[path = "listener_slot_tests.rs"]
mod slot_tests;
