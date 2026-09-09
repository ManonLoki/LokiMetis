//! Hook listener 的协议、绑定与基础状态机回归。

use super::{
    ConnectionOutcome, HOOK_CONNECTION_TASK_LIMIT, HookListenerControl, HookListenerPolicy,
    HookListenerShutdown, HookRelayStatus, IncomingHookEvent, bind_local_hook_relay_listener,
    bound_hook_relay_address, encode_http_response, enqueue_connection_outcome, handle_connection,
    hook_listener_policy, hook_relay_bind_addr, parse_hook_request, process_hook_event,
    run_connection_loop,
};
use loki_metis_core::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HookBehavior, HookStateMachine,
    HookTransition,
};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tempfile::tempdir;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

/// 所有协议样例共享的合法 listener 实例身份。
const TEST_INSTANCE_ID: &str = "01234567-89ab-4def-8123-456789abcdef";

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
    /// 按既定分片顺序填充 Tokio 读缓冲，模拟一次请求被 TCP 拆分。
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

/// 生成带双重实例身份与事件头的本机 Hook 请求。
fn hook_request(slug: &str, hook_type: Option<&str>, body: &str) -> Vec<u8> {
    let mut headers = format!(
        "POST /api/hooks/{TEST_INSTANCE_ID}/{slug} HTTP/1.1\r\n{HOOK_RELAY_INSTANCE_HEADER}: {TEST_INSTANCE_ID}\r\n"
    );
    if let Some(hook_type) = hook_type {
        headers.push_str(&format!("{HOOK_EVENT_TYPE_HEADER}: {hook_type}\r\n"));
    }
    headers.push_str("\r\n");
    headers.push_str(body);
    headers.into_bytes()
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

/// listener 的默认状态和真实绑定都只使用操作系统分配的回环端口。
#[tokio::test]
async fn binds_only_an_os_assigned_loopback_port() {
    assert_eq!(hook_relay_bind_addr().to_string(), "127.0.0.1:0");
    assert_eq!(HookRelayStatus::default().bind_address, "127.0.0.1:0");
    let listener = bind_local_hook_relay_listener()
        .await
        .expect("dynamic bind");
    let bind_address = bound_hook_relay_address(&listener).expect("address");
    let port = listener.local_addr().expect("local").port();
    assert!(bind_address.starts_with("127.0.0.1:"));
    assert_eq!(bind_address, format!("127.0.0.1:{port}"));
    assert_ne!(port, 0);
    let connected = std::net::TcpStream::connect_timeout(
        &bind_address.parse().expect("socket"),
        Duration::from_secs(1),
    );
    assert!(connected.is_ok(), "bound port must accept a local connect");
}

/// 超过固定上限的慢连接必须在 accept 后立即关闭，且 shutdown 会归还全部任务许可。
#[tokio::test]
async fn excess_slow_connections_are_closed_without_leaking_task_permits() {
    let listener = bind_local_hook_relay_listener()
        .await
        .expect("dynamic bind");
    let address = listener.local_addr().expect("listener address");
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&[AiTool::Codex])));
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    let (shutdown, shutdown_receiver) = tokio::sync::watch::channel(false);
    let connection_slots = Arc::new(Semaphore::new(HOOK_CONNECTION_TASK_LIMIT));
    let server = tokio::spawn(run_connection_loop(
        listener,
        Arc::clone(&status),
        sender,
        policy,
        TEST_INSTANCE_ID.to_owned(),
        HookListenerShutdown::new(shutdown_receiver),
        Arc::clone(&connection_slots),
    ));

    let mut slow_connections = Vec::with_capacity(HOOK_CONNECTION_TASK_LIMIT);
    for _ in 0..HOOK_CONNECTION_TASK_LIMIT {
        let mut stream = TcpStream::connect(address).await.expect("slow connection");
        stream.write_all(b"P").await.expect("partial request");
        slow_connections.push(stream);
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        while connection_slots.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all slow connections occupy their task permits");

    let mut excess = TcpStream::connect(address)
        .await
        .expect("excess connection");
    let mut response = [0_u8; 1];
    let closed = tokio::time::timeout(Duration::from_secs(1), excess.read(&mut response))
        .await
        .expect("excess connection is closed promptly");
    assert!(matches!(closed, Ok(0) | Err(_)));
    assert_eq!(status.read().expect("status").failed_count, 1);
    assert_eq!(connection_slots.available_permits(), 0);

    shutdown.send(true).expect("listener shutdown");
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("connection loop stops promptly")
        .expect("connection loop task")
        .expect("connection loop result");
    assert_eq!(
        connection_slots.available_permits(),
        HOOK_CONNECTION_TASK_LIMIT
    );
    drop(slow_connections);
}

/// 合法的实例化请求应保留工具与事件。
#[test]
fn accepted_post_records_codex_session_start() {
    let raw = hook_request(
        "codex",
        Some("SessionStart"),
        r#"{"hook_event_name":"SessionStart"}"#,
    );
    let outcome = parse_hook_request(&raw, TEST_INSTANCE_ID);
    assert!(outcome.ok);
    assert!(outcome.authenticated);
    assert_eq!(outcome.status, "202 Accepted");
    let event = outcome.event.expect("event");
    assert_eq!(event.tool, AiTool::Codex);
    assert_eq!(event.hook_type, "SessionStart");
}

/// HTTP 边界必须保留状态机拦截所需的会话、轮次和状态字段。
#[test]
fn accepted_post_preserves_trimmed_lifecycle_context() {
    let raw = hook_request(
        "codex",
        Some("UserPromptSubmit"),
        r#"{"hook_event_name":"UserPromptSubmit","session_id":" session-1 ","turn_id":" turn-1 ","status":" running "}"#,
    );
    let outcome = parse_hook_request(&raw, TEST_INSTANCE_ID);
    assert!(outcome.ok);
    let event = outcome.event.expect("event");
    assert_eq!(event.session_id.as_deref(), Some("session-1"));
    assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
    assert_eq!(event.status.as_deref(), Some("running"));
}

/// 缺少可信事件头时不能仅凭正文自报事件类型。
#[test]
fn post_without_trusted_hook_type_header_is_rejected() {
    let raw = hook_request("codex", None, r#"{"hook_event_name":"SessionEnd"}"#);
    let outcome = parse_hook_request(&raw, TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(outcome.authenticated);
    assert_eq!(outcome.status, "400 Bad Request");
    assert_eq!(outcome.body, "Missing hook type");
}

/// Headers 必须是 ASCII，JSON body 必须是严格 UTF-8，不能用替换字符容错。
#[test]
fn non_ascii_headers_and_invalid_utf8_payload_are_rejected() {
    let mut invalid_header = hook_request(
        "codex",
        Some("SessionStart"),
        r#"{"hook_event_name":"SessionStart"}"#,
    );
    invalid_header[0] = 0xff;
    let outcome = parse_hook_request(&invalid_header, TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(!outcome.authenticated);
    assert_eq!(outcome.status, "400 Bad Request");
    assert_eq!(outcome.body, "Bad Request");

    let headers = format!(
        "POST /api/hooks/{TEST_INSTANCE_ID}/codex HTTP/1.1\r\n{HOOK_RELAY_INSTANCE_HEADER}: {TEST_INSTANCE_ID}\r\n{HOOK_EVENT_TYPE_HEADER}: SessionStart\r\n\r\n"
    );
    let mut invalid_body = headers.into_bytes();
    invalid_body.extend_from_slice(b"{\"hook_event_name\":\"");
    invalid_body.push(0xff);
    invalid_body.extend_from_slice(b"\"}");
    let outcome = parse_hook_request(&invalid_body, TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(outcome.authenticated);
    assert_eq!(outcome.status, "400 Bad Request");
    assert_eq!(outcome.body, "Invalid payload");
}

/// 路径或请求头不是当前实例时必须先于正文解析被拒绝。
#[test]
fn stale_or_foreign_listener_identity_is_rejected() {
    let stale = hook_request(
        "codex",
        Some("SessionStart"),
        r#"{"hook_event_name":"SessionStart"}"#,
    );
    let outcome = parse_hook_request(&stale, "fedcba98-7654-4321-8765-abcdef012345");
    assert!(!outcome.ok);
    assert!(!outcome.authenticated);
    assert_eq!(outcome.status, "404 Not Found");

    let wrong_header = format!(
        "POST /api/hooks/{TEST_INSTANCE_ID}/codex HTTP/1.1\r\n{HOOK_RELAY_INSTANCE_HEADER}: fedcba98-7654-4321-8765-abcdef012345\r\n{HOOK_EVENT_TYPE_HEADER}: SessionStart\r\n\r\n{{\"hook_event_name\":\"SessionStart\"}}"
    );
    let outcome = parse_hook_request(wrong_header.as_bytes(), TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(!outcome.authenticated);
    assert_eq!(outcome.status, "404 Not Found");

    let missing_header = format!(
        "POST /api/hooks/{TEST_INSTANCE_ID}/codex HTTP/1.1\r\n{HOOK_EVENT_TYPE_HEADER}: SessionStart\r\n\r\n{{\"hook_event_name\":\"SessionStart\"}}"
    );
    let outcome = parse_hook_request(missing_header.as_bytes(), TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(!outcome.authenticated);
    assert_eq!(outcome.body, "Missing listener identity");
}

/// 即使路径和身份头泄露到请求中，非 POST 请求也不能取得认证响应头。
#[test]
fn non_post_request_never_authenticates() {
    let raw = format!(
        "GET /api/hooks/{TEST_INSTANCE_ID}/codex HTTP/1.1\r\n{HOOK_RELAY_INSTANCE_HEADER}: {TEST_INSTANCE_ID}\r\n{HOOK_EVENT_TYPE_HEADER}: SessionStart\r\n\r\n"
    );
    let outcome = parse_hook_request(raw.as_bytes(), TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(!outcome.authenticated);
    assert_eq!(outcome.status, "400 Bad Request");
}

/// 与 AIMonitor 一致：初始无图，重复或迟到事件不覆盖，最后 SessionEnd 清空槽位。
#[test]
fn lifecycle_interception_keeps_initial_empty_and_releases_last_session() {
    let root = tempdir().expect("temp");
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let mut machines = HashMap::<AiTool, HookStateMachine>::new();
    assert!(status.read().expect("status").pet_states.is_empty());

    assert!(process_hook_event(
        codex_event("SessionStart", Some("session-1"), None),
        Duration::from_secs(1),
        &mut machines,
        &status,
        root.path(),
    ));
    assert_eq!(
        status.read().expect("status").pet_states[0].behavior,
        Some(HookBehavior::Idle)
    );
    assert!(process_hook_event(
        codex_event("UserPromptSubmit", Some("session-1"), Some("turn-1")),
        Duration::from_secs(2),
        &mut machines,
        &status,
        root.path(),
    ));
    assert_eq!(
        status.read().expect("status").pet_states[0].behavior,
        Some(HookBehavior::Running)
    );
    assert!(process_hook_event(
        codex_event("Stop", Some("session-1"), Some("turn-1")),
        Duration::from_secs(3),
        &mut machines,
        &status,
        root.path(),
    ));
    assert!(!process_hook_event(
        codex_event("PostToolUse", Some("session-1"), Some("turn-1")),
        Duration::from_secs(4),
        &mut machines,
        &status,
        root.path(),
    ));
    assert_eq!(
        status.read().expect("status").pet_states[0].behavior,
        Some(HookBehavior::Idle)
    );
    assert!(process_hook_event(
        codex_event("SessionEnd", Some("session-1"), None),
        Duration::from_secs(5),
        &mut machines,
        &status,
        root.path(),
    ));
    let final_status = status.read().expect("status");
    assert_eq!(final_status.pet_states[0].behavior, None);
    assert_eq!(final_status.pet_states[0].revision, final_status.revision);
    assert_eq!(final_status.revision, 4);
}

/// 实例路径正确但工具未知时返回明确边界错误。
#[test]
fn unknown_tool_is_rejected() {
    let raw = hook_request(
        "not-a-real-agent",
        Some("SessionStart"),
        r#"{"hook_event_name":"SessionStart"}"#,
    );
    let outcome = parse_hook_request(&raw, TEST_INSTANCE_ID);
    assert!(!outcome.ok);
    assert!(outcome.authenticated);
    assert_eq!(outcome.body, "Unknown tool");
}

/// 未认证错误响应绝不泄露实例身份，认证后的成功/背压响应继续携带身份头。
#[test]
fn response_identity_header_is_emitted_only_after_authentication() {
    let unauthenticated = encode_http_response("404 Not Found", "Not Found", None);
    assert!(!unauthenticated.contains(HOOK_RELAY_INSTANCE_HEADER));
    assert!(!unauthenticated.contains(TEST_INSTANCE_ID));

    for status in ["202 Accepted", "503 Service Unavailable"] {
        let authenticated = encode_http_response(status, "response", Some(TEST_INSTANCE_ID));
        assert!(authenticated.contains(&format!(
            "{HOOK_RELAY_INSTANCE_HEADER}: {TEST_INSTANCE_ID}\r\n"
        )));
    }
}

/// 合法 POST 分两次到达时，必须读满 Content-Length 后返回 202。
#[tokio::test]
async fn split_read_of_valid_post_is_accepted() {
    let body = serde_json::to_vec(&serde_json::json!({ "hook_event_name": "SessionStart" }))
        .expect("payload");
    let headers = format!(
        "POST /api/hooks/{TEST_INSTANCE_ID}/codex HTTP/1.1\r\n{HOOK_RELAY_INSTANCE_HEADER}: {TEST_INSTANCE_ID}\r\n{HOOK_EVENT_TYPE_HEADER}: SessionStart\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let headers_only = parse_hook_request(headers.as_bytes(), TEST_INSTANCE_ID);
    assert!(!headers_only.ok);
    assert_eq!(headers_only.body, "Invalid payload");
    let mut reader = SequentialChunks::new(vec![headers.into_bytes(), body]);
    let outcome = handle_connection(&mut reader, TEST_INSTANCE_ID).await;
    assert!(outcome.ok);
    assert_eq!(outcome.status, "202 Accepted");
    let event = outcome.event.expect("event");
    assert_eq!(event.tool, AiTool::Codex);
    assert_eq!(event.hook_type, "SessionStart");
}

/// 测试用 listener 门禁，不启动真实 Tauri worker。
fn listener_control(
    enabled_tools: &[AiTool],
) -> (Arc<RwLock<HookRelayStatus>>, HookListenerControl) {
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(enabled_tools)));
    let (shutdown, _) = tokio::sync::watch::channel(false);
    let control = HookListenerControl::new(policy, Arc::clone(&status), shutdown, Vec::new());
    (status, control)
}

/// listener 构造与运行时替换都必须再次执行统一目录门禁，不能信任内部直传集合。
#[test]
fn hidden_tool_cannot_be_admitted_by_direct_policy_inputs() {
    let (_status, initialized) = listener_control(&[AiTool::OpenCode]);
    assert!(
        hook_listener_policy(&initialized.policy)
            .admission(AiTool::OpenCode)
            .is_none()
    );

    let (_status, replaced) = listener_control(&[AiTool::Codex]);
    assert!(!replaced.replace_enabled_tools(&[AiTool::OpenCode]));
    let policy = hook_listener_policy(&replaced.policy);
    assert!(policy.admission(AiTool::Codex).is_none());
    assert!(policy.admission(AiTool::OpenCode).is_none());
}

/// 禁用工具会立即释放它占用的位置，并使旧代数事件失效。
#[test]
fn disabling_tool_releases_pet_state_and_invalidates_queued_generation() {
    let (status, control) = listener_control(&[AiTool::Codex]);
    let original_generation = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("enabled");
    {
        let mut current = status.write().expect("status");
        let mut revision = current.revision;
        super::super::listener_state::apply_pet_transition(
            &mut current.pet_states,
            &mut revision,
            AiTool::Codex,
            0,
            HookTransition::Display(HookBehavior::Running),
        );
        current.revision = revision;
    }

    assert!(control.replace_enabled_tools(&[]));
    assert!(
        status
            .read()
            .expect("status")
            .pet_states
            .iter()
            .all(|state| state.behavior.is_none())
    );
    assert!(
        !hook_listener_policy(&control.policy)
            .admits_generation(AiTool::Codex, original_generation)
    );
}

/// 重新启用会分配新代数，旧会话状态和旧排队事件不得复活。
#[test]
fn reenabled_tool_uses_a_fresh_generation() {
    let (_status, control) = listener_control(&[AiTool::Codex]);
    let original = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("enabled");
    assert!(!control.replace_enabled_tools(&[]));
    assert!(!control.replace_enabled_tools(&[AiTool::Codex]));
    let reenabled = hook_listener_policy(&control.policy)
        .admission(AiTool::Codex)
        .expect("reenabled");
    assert!(reenabled > original);
    assert!(!hook_listener_policy(&control.policy).admits_generation(AiTool::Codex, original));
}

/// 旧 Hook 文件仍投递时返回成功以保持插件 fail-open，但不进入队列或状态计数。
#[test]
fn disabled_tool_request_is_acknowledged_without_reaching_the_worker() {
    let (status, control) = listener_control(&[]);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let outcome = ConnectionOutcome {
        ok: true,
        authenticated: true,
        status: "202 Accepted",
        body: "Accepted",
        event: Some(codex_event("SessionStart", Some("session-1"), None)),
    };

    let response = enqueue_connection_outcome(outcome, &sender, &status, &control.policy);
    assert_eq!(response, ("202 Accepted", "Accepted", true));
    assert!(receiver.try_recv().is_err());
    let current = status.read().expect("status");
    assert_eq!(current.received_count, 0);
    assert!(current.pet_states.is_empty());
}
