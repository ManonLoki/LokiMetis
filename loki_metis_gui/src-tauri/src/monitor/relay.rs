//! 命令型 Hook 中继：把原生 stdin 归约后 POST 到本机 listener。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use loki_metis_core::{
    DEFAULT_HOOK_RELAY_PORT, MAX_NATIVE_HOOK_INPUT_BYTES, PreparedNativeHook,
    hook_relay_loopback_address, managed_hook_marker, prepare_native_hook, tool_from_slug,
};

/// 触发 relay 子进程模式的命令行标志。
pub const HOOK_RELAY_ARGUMENT: &str = "--loki-metis-hook-relay";

/// 若当前进程是 Hook relay 模式则执行并返回退出码。
pub fn run_hook_relay_if_requested() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    let Some(flag) = args.next() else {
        return None;
    };
    if flag != HOOK_RELAY_ARGUMENT {
        return None;
    }
    let tool_slug = args.next()?;
    let event = args.next()?;
    let mut managed_by = None;
    while let Some(arg) = args.next() {
        if arg == "--managed-by" {
            managed_by = args.next();
        }
    }
    Some(run_relay(&tool_slug, &event, managed_by.as_deref()))
}

/// 校验参数、读取 stdin 并投递最小信封。
fn run_relay(tool_slug: &str, event: &str, managed_by: Option<&str>) -> i32 {
    let Some(tool) = tool_from_slug(tool_slug) else {
        return 2;
    };
    let expected = managed_hook_marker(tool);
    if managed_by != Some(expected.as_str()) {
        return 2;
    }
    let mut stdin = Vec::new();
    if std::io::stdin()
        .take(MAX_NATIVE_HOOK_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut stdin)
        .is_err()
    {
        return 1;
    }
    match prepare_native_hook(tool, &stdin, event) {
        Ok(PreparedNativeHook::SuppressForeignHost) => 0,
        Ok(PreparedNativeHook::Deliver(payload)) => {
            let body = match serde_json::to_vec(&payload) {
                Ok(body) => body,
                Err(_) => return 1,
            };
            match post_local_hook(tool_slug, &payload.hook_event_name, &body) {
                Ok(true) => 0,
                _ => 1,
            }
        }
        Err(_) => 1,
    }
}

/// 本机中继实际绑定端口的落盘文件名。
fn bound_port_path() -> std::path::PathBuf {
    std::env::temp_dir().join("loki-metis-hook-relay.port")
}

/// 把 listener 实际绑定的端口交给 relay 子进程读取。
pub(super) fn persist_bound_hook_relay_port(port: u16) -> std::io::Result<()> {
    std::fs::write(bound_port_path(), port.to_string())
}

/// 读取当前中继端口；缺失或损坏时回退首选端口。
pub(super) fn load_bound_hook_relay_port() -> u16 {
    std::fs::read_to_string(bound_port_path())
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_HOOK_RELAY_PORT)
}

/// 投递使用的回环套接字地址。
pub(super) fn hook_relay_connect_addr(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

/// 把请求头和 JSON 正文编码成一次写出的 HTTP/1.1 缓冲。
pub(super) fn encode_local_hook_request(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    port: u16,
) -> Vec<u8> {
    let host = hook_relay_loopback_address(port);
    let header = format!(
        "POST /api/hooks/{slug} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nX-LokiMetis-Hook-Type: {hook_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut request = Vec::with_capacity(header.len() + body.len());
    request.extend_from_slice(header.as_bytes());
    request.extend_from_slice(body);
    request
}

/// POST 到本机 listener 的实际绑定端口。
fn post_local_hook(slug: &str, hook_type: &str, body: &[u8]) -> Result<bool, ()> {
    let port = load_bound_hook_relay_port();
    let mut stream =
        TcpStream::connect_timeout(&hook_relay_connect_addr(port), Duration::from_secs(1))
            .map_err(|_| ())?;
    stream.set_nodelay(true).map_err(|_| ())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| ())?;
    let request = encode_local_hook_request(slug, hook_type, body, port);
    stream.write_all(&request).map_err(|_| ())?;
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    Ok(response.contains("202"))
}

#[cfg(test)]
mod tests {
    use super::{
        encode_local_hook_request, hook_relay_connect_addr, persist_bound_hook_relay_port,
    };
    use loki_metis_core::{DEFAULT_HOOK_RELAY_PORT, hook_relay_loopback_address};

    /// 中继请求头和正文必须落在同一缓冲里，避免分两次 write_all。
    #[test]
    fn encode_local_hook_request_keeps_headers_and_body_together() {
        let body = br#"{"hook_event_name":"SessionStart"}"#;
        let request = encode_local_hook_request("codex", "SessionStart", body, DEFAULT_HOOK_RELAY_PORT);
        let split = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("header terminator");
        let encoded_body = &request[split + 4..];
        assert_eq!(encoded_body, body);
        let headers = std::str::from_utf8(&request[..split]).expect("headers");
        assert!(headers.contains(&format!("Content-Length: {}", body.len())));
        assert!(!headers.contains('\0'));
    }

    /// 绑定端口不是 10240 时，Host 与连接地址使用该端口，正文仍完整。
    #[test]
    fn encode_and_connect_use_the_actual_bound_port() {
        let port = 23_456_u16;
        assert_ne!(port, DEFAULT_HOOK_RELAY_PORT);
        let body = br#"{"hook_event_name":"SessionStart","tool":"codex"}"#;
        let request = encode_local_hook_request("codex", "SessionStart", body, port);
        let split = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("header terminator");
        assert_eq!(&request[split + 4..], body);
        let headers = std::str::from_utf8(&request[..split]).expect("headers");
        let expected_host = hook_relay_loopback_address(port);
        assert!(headers.contains(&format!("Host: {expected_host}")));
        assert!(!headers.contains(&format!("Host: {}", hook_relay_loopback_address(DEFAULT_HOOK_RELAY_PORT))));
        let previous = super::load_bound_hook_relay_port();
        persist_bound_hook_relay_port(port).expect("persist");
        let addr = hook_relay_connect_addr(super::load_bound_hook_relay_port());
        assert_eq!(addr, hook_relay_connect_addr(port));
        assert_eq!(addr.port(), port);
        persist_bound_hook_relay_port(previous).expect("restore");
    }
}
