//! 本机 Hook HTTP/1.1 最小协议的读取、校验与响应。

use loki_metis_core::{
    HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, MAX_NATIVE_HOOK_INPUT_BYTES,
    MinimalHookPayload, tool_from_slug,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::IncomingHookEvent;

/// 状态机长期保存的会话 ID 最大字节数。
const MAX_HOOK_SESSION_ID_BYTES: usize = 512;
/// 状态机长期保存的轮次 ID 最大字节数。
const MAX_HOOK_TURN_ID_BYTES: usize = 512;
/// 状态标量最大字节数。
const MAX_HOOK_STATUS_BYTES: usize = 64;
const HEADER_BYTE_BUDGET: usize = 8192;
const MAX_HOOK_HTTP_REQUEST_BYTES: usize = MAX_NATIVE_HOOK_INPUT_BYTES + HEADER_BYTE_BUDGET;

pub(crate) struct ConnectionOutcome {
    pub(super) ok: bool,
    pub(super) authenticated: bool,
    pub(super) status: &'static str,
    pub(super) body: &'static str,
    pub(super) event: Option<IncomingHookEvent>,
}

/// 读取一个 HTTP/1.1 请求并校验最小信封。
pub(super) async fn handle_connection<R: AsyncRead + Unpin>(
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

pub(super) fn fail(status: &'static str, body: &'static str) -> ConnectionOutcome {
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
pub(super) fn encode_http_response(
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

pub(super) async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
    authenticated_instance_id: Option<&str>,
) -> std::io::Result<()> {
    let response = encode_http_response(status, body, authenticated_instance_id);
    stream.write_all(response.as_bytes()).await
}
