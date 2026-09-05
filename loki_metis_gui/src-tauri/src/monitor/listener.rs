//! 本机环回 Hook listener：接受四项 Agent 的最小信封并记账。

use std::sync::{Arc, RwLock};

use loki_metis_core::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, HOOK_RELAY_EPHEMERAL_PORT, HookBehavior,
    MAX_NATIVE_HOOK_INPUT_BYTES, MinimalHookPayload, PetOverlayToolBehavior,
    display_behavior_for_hook_event, hook_relay_loopback_address, tool_from_slug,
};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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
    let shared = Arc::clone(&status);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_listener(Arc::clone(&shared)).await {
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
        Err(error) if is_address_in_use(&error) => {
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

fn is_address_in_use(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AddrInUse
        || matches!(error.raw_os_error(), Some(48 | 98 | 10048))
}

/// 绑定后的实际回环地址，供状态与 relay 投递共用。
pub fn bound_hook_relay_address(listener: &TcpListener) -> std::io::Result<String> {
    Ok(hook_relay_loopback_address(listener.local_addr()?.port()))
}

/// 绑定并接受本机 Hook POST。
async fn run_listener(status: Arc<RwLock<HookRelayStatus>>) -> Result<(), String> {
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
        tauri::async_runtime::spawn(async move {
            let outcome = handle_connection(&mut stream).await;
            let _ = write_http_response(&mut stream, outcome.status, outcome.body).await;
            if let Ok(mut current) = status.write() {
                if outcome.ok {
                    current.received_count += 1;
                    current.last_event = outcome.event.clone();
                    if let Some(event) = outcome.event {
                        let behavior =
                            display_behavior_for_hook_event(event.tool, &event.hook_type);
                        upsert_last_behavior(&mut current.last_behaviors, event.tool, behavior);
                    }
                    current.last_error = None;
                } else {
                    current.failed_count += 1;
                    current.last_error = Some(outcome.body.to_owned());
                }
            }
        });
    }
}

pub(crate) struct ConnectionOutcome {
    ok: bool,
    status: &'static str,
    body: &'static str,
    event: Option<HookRelayLastEvent>,
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
    buf.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
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
    if let Some(header_type) = header_type
        && header_type != payload.hook_event_name
    {
        return fail("400 Bad Request", "Header mismatch");
    }
    ConnectionOutcome {
        ok: true,
        status: "202 Accepted",
        body: "Accepted",
        event: Some(HookRelayLastEvent {
            tool,
            hook_type: payload.hook_event_name,
        }),
    }
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
        bind_local_hook_relay_listener, bound_hook_relay_address, handle_connection,
        parse_hook_request,
    };
    use loki_metis_core::{AiTool, DEFAULT_HOOK_RELAY_PORT};
    use std::pin::Pin;
    use std::task::{Context, Poll};
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
