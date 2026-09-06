//! 本机环回 Hook listener：接受四项 Agent 的最小信封并记账。

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use loki_metis_core::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, HOOK_RELAY_EPHEMERAL_PORT, HookBehavior, HookEventDecision,
    HookStateMachine, HookTransition, MAX_NATIVE_HOOK_INPUT_BYTES, MinimalHookPayload,
    PetOverlayToolBehavior, hook_relay_loopback_address, tool_from_slug,
};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
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
    /// 各已批准 Agent 最近一次展示行为，供桌宠选图。
    pub last_behaviors: Vec<PetOverlayToolBehavior>,
    /// 最近一次错误。
    pub last_error: Option<String>,
}

impl Default for HookRelayStatus {
    fn default() -> Self {
        Self {
            listening: false,
            bind_address: hook_relay_loopback_address(DEFAULT_HOOK_RELAY_PORT),
            received_count: 0,
            failed_count: 0,
            last_event: None,
            last_behaviors: Vec::new(),
            last_error: None,
        }
    }
}

/// 启动环回 listener，返回可查询的共享状态。
pub fn spawn_hook_listener() -> Arc<RwLock<HookRelayStatus>> {
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let (sender, receiver) = mpsc::channel(HOOK_EVENT_QUEUE_CAPACITY);
    tauri::async_runtime::spawn(run_hook_worker(receiver, Arc::clone(&status)));
    let shared = Arc::clone(&status);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_listener(Arc::clone(&shared), sender).await {
            if let Ok(mut current) = shared.write() {
                current.listening = false;
                current.last_error = Some(error);
            }
        }
    });
    status
}

/// 先绑 10240；占用则改绑操作系统分配的回环空闲端口。
pub async fn bind_local_hook_relay_listener() -> std::io::Result<TcpListener> {
    match TcpListener::bind(("127.0.0.1", DEFAULT_HOOK_RELAY_PORT)).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            TcpListener::bind(("127.0.0.1", HOOK_RELAY_EPHEMERAL_PORT)).await
        }
        Err(error) => Err(error),
    }
}

fn upsert_last_behavior(
    behaviors: &mut Vec<PetOverlayToolBehavior>,
    tool: AiTool,
    behavior: HookBehavior,
) {
    if let Some(existing) = behaviors.iter_mut().find(|item| item.tool == tool) {
        existing.behavior = behavior;
        return;
    }
    behaviors.push(PetOverlayToolBehavior { tool, behavior });
}

/// 把状态机迁移应用到桌宠当前行为；Release 真实清空该工具槽位。
fn apply_pet_transition(
    behaviors: &mut Vec<PetOverlayToolBehavior>,
    tool: AiTool,
    transition: HookTransition,
) {
    match transition {
        HookTransition::Display(behavior) => upsert_last_behavior(behaviors, tool, behavior),
        HookTransition::Release => behaviors.retain(|item| item.tool != tool),
    }
}

/// 串行推进每个工具的生命周期，避免连接任务调度顺序直接改写桌宠状态。
async fn run_hook_worker(
    mut receiver: mpsc::Receiver<IncomingHookEvent>,
    status: Arc<RwLock<HookRelayStatus>>,
) {
    let mut state_machines = HashMap::<AiTool, HookStateMachine>::new();
    let clock_started_at = Instant::now();
    let first_sweep = tokio::time::Instant::now() + HOOK_SESSION_SWEEP_INTERVAL;
    let mut sweep = interval_at(first_sweep, HOOK_SESSION_SWEEP_INTERVAL);
    sweep.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = receiver.recv() => {
                let Some(event) = event else {
                    break;
                };
                process_hook_event(
                    event,
                    clock_started_at.elapsed(),
                    &mut state_machines,
                    &status,
                );
            }
            _ = sweep.tick() => {
                expire_inactive_hook_sessions(
                    &mut state_machines,
                    clock_started_at.elapsed(),
                    &status,
                );
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
) {
    let decision = state_machines
        .entry(event.tool)
        .or_default()
        .apply_event_with_status_at(
            event.tool,
            &event.hook_type,
            event.session_id.as_deref(),
            event.turn_id.as_deref(),
            event.status.as_deref(),
            observed_at,
        );
    if let Ok(mut current) = status.write() {
        current.received_count += 1;
        current.last_event = Some(HookRelayLastEvent {
            tool: event.tool,
            hook_type: event.hook_type.clone(),
        });
        match decision {
            HookEventDecision::Forward(transition) => {
                apply_pet_transition(&mut current.last_behaviors, event.tool, transition);
                current.last_error = None;
            }
            HookEventDecision::Ignore => current.last_error = None,
            HookEventDecision::Unsupported => {
                current.failed_count += 1;
                current.last_error = Some(format!("unsupported hook type: {}", event.hook_type));
            }
        }
    }
}

/// 定期回收孤儿会话，并应用状态机要求的 Idle 回落。
fn expire_inactive_hook_sessions(
    state_machines: &mut HashMap<AiTool, HookStateMachine>,
    observed_at: Duration,
    status: &Arc<RwLock<HookRelayStatus>>,
) {
    let transitions = state_machines
        .iter_mut()
        .filter_map(|(&tool, machine)| {
            match machine.expire_inactive_sessions(observed_at, HOOK_SESSION_INACTIVITY_TIMEOUT) {
                HookEventDecision::Forward(transition) => Some((tool, transition)),
                HookEventDecision::Ignore | HookEventDecision::Unsupported => None,
            }
        })
        .collect::<Vec<_>>();
    if transitions.is_empty() {
        return;
    }
    if let Ok(mut current) = status.write() {
        for (tool, transition) in transitions {
            apply_pet_transition(&mut current.last_behaviors, tool, transition);
        }
    }
}

/// 绑定后的实际回环地址，供状态与 relay 投递共用。
pub fn bound_hook_relay_address(listener: &TcpListener) -> std::io::Result<String> {
    Ok(hook_relay_loopback_address(listener.local_addr()?.port()))
}

/// 绑定并接受本机 Hook POST。
async fn run_listener(
    status: Arc<RwLock<HookRelayStatus>>,
    sender: mpsc::Sender<IncomingHookEvent>,
) -> Result<(), String> {
    let listener = bind_local_hook_relay_listener()
        .await
        .map_err(|error| error.to_string())?;
    let bind_address = bound_hook_relay_address(&listener).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    super::relay::persist_bound_hook_relay_port(port).map_err(|error| error.to_string())?;
    if let Ok(mut current) = status.write() {
        current.listening = true;
        current.bind_address = bind_address;
    }
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let status = Arc::clone(&status);
        let sender = sender.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = handle_connection(&mut stream).await;
            let (response_status, response_body) =
                enqueue_connection_outcome(outcome, &sender, &status);
            let _ = write_http_response(&mut stream, response_status, response_body).await;
        });
    }
}

/// 把解析成功的事件放入有界队列，并为解析或背压失败记账。
fn enqueue_connection_outcome(
    outcome: ConnectionOutcome,
    sender: &mpsc::Sender<IncomingHookEvent>,
    status: &Arc<RwLock<HookRelayStatus>>,
) -> (&'static str, &'static str) {
    if !outcome.ok {
        record_listener_failure(status, outcome.body);
        return (outcome.status, outcome.body);
    }
    let Some(event) = outcome.event else {
        record_listener_failure(status, "Invalid payload");
        return ("400 Bad Request", "Invalid payload");
    };
    match sender.try_send(event) {
        Ok(()) => (outcome.status, outcome.body),
        Err(TrySendError::Full(_)) => {
            record_listener_failure(status, "Hook event queue is full");
            ("503 Service Unavailable", "Service Unavailable")
        }
        Err(TrySendError::Closed(_)) => {
            record_listener_failure(status, "Hook event worker is unavailable");
            ("503 Service Unavailable", "Service Unavailable")
        }
    }
}

/// 记录 HTTP 边界或队列失败，不伪造任何桌宠展示迁移。
fn record_listener_failure(status: &Arc<RwLock<HookRelayStatus>>, error: &str) {
    if let Ok(mut current) = status.write() {
        current.failed_count += 1;
        current.last_error = Some(error.to_owned());
    }
}

pub(crate) struct ConnectionOutcome {
    ok: bool,
    status: &'static str,
    body: &'static str,
    event: Option<IncomingHookEvent>,
}

const HEADER_BYTE_BUDGET: usize = 8192;
const MAX_HOOK_HTTP_REQUEST_BYTES: usize = MAX_NATIVE_HOOK_INPUT_BYTES + HEADER_BYTE_BUDGET;

/// 读取一个 HTTP/1.1 请求并校验最小信封。
async fn handle_connection<R: AsyncRead + Unpin>(stream: &mut R) -> ConnectionOutcome {
    match read_complete_http_request(stream).await {
        Ok(raw) => parse_hook_request(&raw),
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

/// 解析 POST /api/hooks/{slug} 的最小信封。
pub(crate) fn parse_hook_request(raw: &[u8]) -> ConnectionOutcome {
    let text = String::from_utf8_lossy(raw);
    let (headers, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or("");
    let Some(path) = request_line
        .strip_prefix("POST ")
        .and_then(|rest| rest.split(' ').next())
    else {
        return fail("400 Bad Request", "Bad Request");
    };
    let Some(slug) = path.strip_prefix("/api/hooks/") else {
        return fail("404 Not Found", "Not Found");
    };
    let Some(tool) = tool_from_slug(slug) else {
        return fail("400 Bad Request", "Unknown tool");
    };
    let header_type = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("x-lokimetis-hook-type") {
            Some(value.trim().to_owned())
        } else {
            None
        }
    });
    let payload: MinimalHookPayload = match serde_json::from_str(body.trim()) {
        Ok(payload) => payload,
        Err(_) => return fail("400 Bad Request", "Invalid payload"),
    };
    let hook_type = payload.hook_event_name.trim();
    if hook_type.is_empty() || hook_type.len() > 128 {
        return fail("400 Bad Request", "Invalid hook type");
    }
    let Some(header_type) = header_type else {
        return fail("400 Bad Request", "Missing hook type");
    };
    if header_type != hook_type {
        return fail("400 Bad Request", "Header mismatch");
    }
    let Ok(session_id) =
        normalize_hook_context_field(payload.session_id, MAX_HOOK_SESSION_ID_BYTES)
    else {
        return fail("400 Bad Request", "Invalid session id");
    };
    let Ok(turn_id) = normalize_hook_context_field(payload.turn_id, MAX_HOOK_TURN_ID_BYTES) else {
        return fail("400 Bad Request", "Invalid turn id");
    };
    let Ok(status) = normalize_hook_context_field(payload.status, MAX_HOOK_STATUS_BYTES) else {
        return fail("400 Bad Request", "Invalid status");
    };
    ConnectionOutcome {
        ok: true,
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
        status,
        body,
        event: None,
    }
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::{
        HookRelayStatus, IncomingHookEvent, bind_local_hook_relay_listener,
        bound_hook_relay_address, handle_connection, parse_hook_request, process_hook_event,
    };
    use loki_metis_core::{AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookStateMachine};
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, RwLock};
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, ReadBuf};

    /// 每次 poll_read 只交出一个分片，用来复现 TCP 拆包。
    struct SequentialChunks {
        chunks: Vec<Vec<u8>>,
        index: usize,
        offset: usize,
    }

    impl SequentialChunks {
        /// 用给定分片构造只读流。
        fn new(chunks: Vec<Vec<u8>>) -> Self {
            Self {
                chunks,
                index: 0,
                offset: 0,
            }
        }
    }

    impl AsyncRead for SequentialChunks {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            if this.index >= this.chunks.len() {
                return Poll::Ready(Ok(()));
            }
            let chunk = &this.chunks[this.index];
            let remaining = &chunk[this.offset..];
            let take = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..take]);
            this.offset += take;
            if this.offset >= chunk.len() {
                this.index += 1;
                this.offset = 0;
            }
            Poll::Ready(Ok(()))
        }
    }

    /// 构造一条 Codex 生命周期事件，供串行 adapter 回归复用。
    fn codex_event(
        hook_type: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> IncomingHookEvent {
        IncomingHookEvent {
            tool: AiTool::Codex,
            hook_type: hook_type.to_owned(),
            session_id: session_id.map(str::to_owned),
            turn_id: turn_id.map(str::to_owned),
            status: None,
        }
    }

    /// 10240 被占用时，已发布绑定入口改绑其他回环端口且可连接。
    #[tokio::test]
    async fn occupies_default_port_then_binds_another_loopback_port() {
        let _occupied = std::net::TcpListener::bind(("127.0.0.1", DEFAULT_HOOK_RELAY_PORT)).ok();
        let listener = bind_local_hook_relay_listener()
            .await
            .expect("fallback bind");
        let bind_address = bound_hook_relay_address(&listener).expect("address");
        let port = listener.local_addr().expect("local").port();
        assert!(bind_address.starts_with("127.0.0.1:"));
        assert_eq!(bind_address, format!("127.0.0.1:{port}"));
        assert_ne!(port, DEFAULT_HOOK_RELAY_PORT);
        assert_ne!(port, 0);
        let connected = std::net::TcpStream::connect_timeout(
            &bind_address.parse().expect("socket"),
            std::time::Duration::from_secs(1),
        );
        assert!(connected.is_ok(), "bound port must accept a local connect");
    }

    #[test]
    fn accepted_post_records_codex_session_start() {
        let raw = b"POST /api/hooks/codex HTTP/1.1\r\nX-LokiMetis-Hook-Type: SessionStart\r\n\r\n{\"hook_event_name\":\"SessionStart\"}";
        let outcome = parse_hook_request(raw);
        assert!(outcome.ok);
        assert_eq!(outcome.status, "202 Accepted");
        let event = outcome.event.expect("event");
        assert_eq!(event.tool, AiTool::Codex);
        assert_eq!(event.hook_type, "SessionStart");
    }

    /// HTTP 边界必须保留状态机拦截所需的会话、轮次和状态字段。
    #[test]
    fn accepted_post_preserves_trimmed_lifecycle_context() {
        let raw = b"POST /api/hooks/codex HTTP/1.1\r\nX-LokiMetis-Hook-Type: UserPromptSubmit\r\n\r\n{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\" session-1 \",\"turn_id\":\" turn-1 \",\"status\":\" running \"}";
        let outcome = parse_hook_request(raw);
        assert!(outcome.ok);
        let event = outcome.event.expect("event");
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(event.status.as_deref(), Some("running"));
    }

    /// 缺少可信事件头时不能仅凭正文自报事件类型。
    #[test]
    fn post_without_trusted_hook_type_header_is_rejected() {
        let raw = b"POST /api/hooks/codex HTTP/1.1\r\n\r\n{\"hook_event_name\":\"SessionEnd\"}";
        let outcome = parse_hook_request(raw);
        assert!(!outcome.ok);
        assert_eq!(outcome.status, "400 Bad Request");
        assert_eq!(outcome.body, "Missing hook type");
    }

    /// 与 AIMonitor 一致：初始无图，重复或迟到事件不覆盖，最后 SessionEnd 清空槽位。
    #[test]
    fn lifecycle_interception_keeps_initial_empty_and_releases_last_session() {
        let status = Arc::new(RwLock::new(HookRelayStatus::default()));
        let mut machines = HashMap::<AiTool, HookStateMachine>::new();
        assert!(status.read().expect("status").last_behaviors.is_empty());

        process_hook_event(
            codex_event("SessionStart", Some("session-1"), None),
            Duration::from_secs(1),
            &mut machines,
            &status,
        );
        assert_eq!(
            status.read().expect("status").last_behaviors[0].behavior,
            HookBehavior::Idle
        );
        process_hook_event(
            codex_event("UserPromptSubmit", Some("session-1"), Some("turn-1")),
            Duration::from_secs(2),
            &mut machines,
            &status,
        );
        assert_eq!(
            status.read().expect("status").last_behaviors[0].behavior,
            HookBehavior::Running
        );
        process_hook_event(
            codex_event("Stop", Some("session-1"), Some("turn-1")),
            Duration::from_secs(3),
            &mut machines,
            &status,
        );
        process_hook_event(
            codex_event("PostToolUse", Some("session-1"), Some("turn-1")),
            Duration::from_secs(4),
            &mut machines,
            &status,
        );
        assert_eq!(
            status.read().expect("status").last_behaviors[0].behavior,
            HookBehavior::Idle
        );
        process_hook_event(
            codex_event("SessionEnd", Some("session-1"), None),
            Duration::from_secs(5),
            &mut machines,
            &status,
        );
        assert!(status.read().expect("status").last_behaviors.is_empty());
    }

    #[test]
    fn unknown_tool_is_rejected() {
        let raw = b"POST /api/hooks/cursor HTTP/1.1\r\n\r\n{\"hook_event_name\":\"sessionStart\"}";
        let outcome = parse_hook_request(raw);
        assert!(!outcome.ok);
        assert_eq!(outcome.body, "Unknown tool");
    }

    /// 合法 POST 分两次到达时，必须读满 Content-Length 后返回 202。
    #[tokio::test]
    async fn split_read_of_valid_post_is_accepted() {
        let body = serde_json::to_vec(&serde_json::json!({ "hook_event_name": "SessionStart" }))
            .expect("payload");
        let headers = format!(
            "POST /api/hooks/codex HTTP/1.1\r\nX-LokiMetis-Hook-Type: SessionStart\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let headers_only = parse_hook_request(headers.as_bytes());
        assert!(!headers_only.ok);
        assert_eq!(headers_only.body, "Invalid payload");
        let mut reader = SequentialChunks::new(vec![headers.into_bytes(), body]);
        let outcome = handle_connection(&mut reader).await;
        assert!(outcome.ok);
        assert_eq!(outcome.status, "202 Accepted");
        let event = outcome.event.expect("event");
        assert_eq!(event.tool, AiTool::Codex);
        assert_eq!(event.hook_type, "SessionStart");
    }
}
