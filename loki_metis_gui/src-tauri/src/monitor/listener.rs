//! 本机环回 Hook listener：接受四项 Agent 的最小信封并记账。

use std::sync::{Arc, RwLock};

use loki_metis_core::{
    AiTool, DEFAULT_HOOK_RELAY_PORT, MAX_NATIVE_HOOK_INPUT_BYTES, MinimalHookPayload,
    tool_from_slug,
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
    /// 最近一次错误。
    pub last_error: Option<String>,
}

impl Default for HookRelayStatus {
    fn default() -> Self {
        Self {
            listening: false,
            bind_address: format!("127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}"),
            received_count: 0,
            failed_count: 0,
            last_event: None,
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

/// 绑定并接受本机 Hook POST。
async fn run_listener(status: Arc<RwLock<HookRelayStatus>>) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", DEFAULT_HOOK_RELAY_PORT))
        .await
        .map_err(|error| error.to_string())?;
    if let Ok(mut current) = status.write() {
        current.listening = true;
        current.bind_address = format!("127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}");
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
                    current.last_event = outcome.event;
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
    use super::{handle_connection, parse_hook_request};
    use loki_metis_core::AiTool;
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
