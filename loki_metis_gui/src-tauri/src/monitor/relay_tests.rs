//! Hook relay 的参数、rendezvous、身份校验与重绑重试回归。

use std::{
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use loki_metis_core::{
    AiTool, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, MinimalHookPayload, PreparedNativeHook,
};
use tempfile::tempdir;
use uuid::Uuid;

use super::*;

/// 读满一条带 Content-Length 的测试 HTTP 请求。
fn read_http_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let read = stream.read(&mut buffer).expect("read");
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = std::str::from_utf8(&request[..header_end]).expect("headers");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("length"))
            })
            .expect("content length");
        if request.len() >= header_end + content_length {
            return request;
        }
    }
}

/// 按指定实例身份返回短 HTTP 响应。
fn write_hook_response(stream: &mut TcpStream, status: &str, instance_id: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\n{HOOK_RELAY_INSTANCE_HEADER}: {instance_id}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(response.as_bytes()).expect("respond");
}

/// 合法参数必须完整解析。
#[test]
fn strict_parser_accepts_complete_relay_arguments() {
    let parsed = parse_relay_arguments([
        "LokiMetis",
        HOOK_RELAY_ARGUMENT,
        "codex",
        "PostToolUse",
        "--managed-by",
        "LokiMetis:tool=codex",
    ])
    .expect("relay mode")
    .expect("valid arguments");
    assert_eq!(parsed.tool_slug, "codex");
    assert_eq!(parsed.event, "PostToolUse");
    assert_eq!(parsed.managed_by, managed_hook_marker(AiTool::Codex));
}

/// 命中 relay 模式后，缺参、多参或重复选项不能误启动 GUI。
#[test]
fn strict_parser_rejects_incomplete_or_extra_arguments() {
    assert!(
        parse_relay_arguments(["LokiMetis", HOOK_RELAY_ARGUMENT, "codex", "Stop"])
            .expect("relay mode")
            .is_err()
    );
    assert!(
        parse_relay_arguments([
            "LokiMetis",
            HOOK_RELAY_ARGUMENT,
            "codex",
            "Stop",
            "--managed-by",
            "LokiMetis:tool=codex",
            "unexpected",
        ])
        .expect("relay mode")
        .is_err()
    );
    assert!(parse_relay_arguments(["LokiMetis", "--silent"]).is_none());
}

/// JSON stdout 与 Copilot fail-open 规则必须和上游原生 Hook 协议一致。
#[test]
fn special_hook_protocols_keep_json_stdout_and_fail_open_contracts() {
    let delivered = PreparedNativeHook::Deliver(MinimalHookPayload {
        hook_event_name: "SessionStart".to_owned(),
        session_id: None,
        turn_id: None,
        status: None,
    });
    assert!(requires_json_stdout(
        AiTool::ClaudeCode,
        &PreparedNativeHook::SuppressForeignHost
    ));
    assert!(requires_json_stdout(AiTool::Cursor, &delivered));
    assert!(requires_json_stdout(AiTool::QwenCode, &delivered));
    assert!(requires_json_stdout(AiTool::GeminiCli, &delivered));
    assert!(!requires_json_stdout(AiTool::Codex, &delivered));
    let mut output = Vec::new();
    write_empty_json_response(&mut output).expect("JSON response");
    assert_eq!(output, b"{}\n");
    assert_eq!(relay_failure_exit_code(AiTool::GitHubCopilot), 0);
    assert_eq!(relay_failure_exit_code(AiTool::Codex), 1);
}

/// 响应判断应接受任意真实 2xx，而不是在文本里模糊查找“202”。
#[test]
fn response_status_parsing_accepts_only_real_success_statuses() {
    let instance_id = Uuid::new_v4().to_string();
    let accepted = format!(
        "HTTP/1.1 204 No Content\r\n{HOOK_RELAY_INSTANCE_HEADER}: {instance_id}\r\nContent-Length: 0\r\n\r\n"
    );
    assert!(validate_hook_response(accepted.as_bytes(), &instance_id).is_ok());

    let rejected = format!(
        "HTTP/1.1 500 Internal Server Error\r\n{HOOK_RELAY_INSTANCE_HEADER}: {instance_id}\r\nContent-Length: 3\r\n\r\n202"
    );
    assert!(matches!(
        validate_hook_response(rejected.as_bytes(), &instance_id),
        Err(PostAttemptError::Fatal(_))
    ));
}

/// 持续返回小块数据也不能刷新单次 relay 的三秒总预算。
#[test]
fn trickle_response_cannot_extend_the_wall_clock_deadline() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let rendezvous = HookRelayRendezvous {
        schema_version: HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION,
        port: listener.local_addr().expect("address").port(),
        instance_id: Uuid::new_v4().to_string(),
    };
    let response_instance_id = rendezvous.instance_id.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let _ = read_http_request(&mut stream);
        let headers = format!(
            "HTTP/1.1 202 Accepted\r\n{HOOK_RELAY_INSTANCE_HEADER}: {response_instance_id}\r\nContent-Length: 0\r\n\r\n"
        );
        stream.write_all(headers.as_bytes()).expect("headers");
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(10));
            if stream.write_all(b" ").is_err() {
                break;
            }
        }
    });
    let started_at = Instant::now();
    let result = post_local_hook_once_with_timeout(
        "codex",
        "SessionStart",
        br#"{"hook_event_name":"SessionStart"}"#,
        &rendezvous,
        Duration::from_millis(120),
    );
    let elapsed = started_at.elapsed();
    assert!(matches!(result, Err(PostAttemptError::Fatal(_))));
    assert!(elapsed < Duration::from_secs(1), "elapsed: {elapsed:?}");
    server.join().expect("server");
}

/// Linux 始终使用 HOME cache；桌面进程与 Hook 是否看见 XDG 都不得分叉。
#[test]
fn linux_rendezvous_path_is_canonical_regardless_of_xdg_visibility() {
    let runtime = Path::new("/run/user/1000");
    let home = Path::new("/home/alice");
    let expected = home
        .join(".cache")
        .join("lokimetis")
        .join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    for xdg_runtime_dir in [Some(runtime), None] {
        assert_eq!(
            select_hook_relay_rendezvous_path(
                HookRelayHostPlatform::Linux,
                xdg_runtime_dir,
                Some(home),
                None,
                None,
            )
            .unwrap(),
            expected
        );
    }
    assert_eq!(
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Linux,
            Some(Path::new("relative/runtime")),
            Some(home),
            None,
            None,
        )
        .unwrap(),
        expected
    );
}

/// macOS 与 Windows 都使用单一稳定的用户缓存根。
#[test]
fn desktop_rendezvous_paths_use_stable_user_cache_roots() {
    let home = Path::new("/Users/alice");
    assert_eq!(
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Macos,
            None,
            Some(home),
            None,
            None,
        )
        .unwrap(),
        home.join("Library")
            .join("Caches")
            .join("lokimetis")
            .join(HOOK_RELAY_RENDEZVOUS_FILENAME)
    );

    let local_app_data = Path::new("/windows/local-app-data");
    let user_profile = Path::new("/windows/users/alice");
    let expected = user_profile
        .join("AppData")
        .join("Local")
        .join("lokimetis")
        .join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    for local_app_data in [Some(local_app_data), None] {
        assert_eq!(
            select_hook_relay_rendezvous_path(
                HookRelayHostPlatform::Windows,
                None,
                None,
                local_app_data,
                Some(user_profile),
            )
            .unwrap(),
            expected
        );
    }
}

/// 任何平台缺少可验证绝对用户根时都明确失败，不回退 temp 或当前目录。
#[test]
fn rendezvous_path_rejects_missing_or_relative_user_roots() {
    assert!(
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Linux,
            None,
            Some(Path::new("relative/home")),
            None,
            None,
        )
        .is_err()
    );
    assert!(
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Linux,
            Some(Path::new("/run/user/1000")),
            Some(Path::new("/")),
            None,
            None,
        )
        .is_err()
    );
    assert!(
        select_hook_relay_rendezvous_path(HookRelayHostPlatform::Macos, None, None, None, None,)
            .is_err()
    );
    assert!(
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Windows,
            None,
            None,
            Some(Path::new("relative/local-app-data")),
            Some(Path::new("relative/user-profile")),
        )
        .is_err()
    );
}

/// rendezvous 必须只在测试临时目录原子写入并完整读回。
#[test]
fn rendezvous_round_trip_uses_injected_path() {
    let root = tempdir().expect("temp dir");
    let directory = root.path().join("lokimetis");
    let path = directory.join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    let rendezvous = HookRelayRendezvous::new(23_456);
    persist_hook_relay_rendezvous_at(&path, &rendezvous).expect("persist");
    assert_eq!(
        load_hook_relay_rendezvous_at(&path).expect("load"),
        rendezvous
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

/// 损坏描述即使携带另一应用常用端口，也不得被当作有效 rendezvous。
#[test]
fn invalid_rendezvous_has_no_fixed_port_fallback() {
    let root = tempdir().expect("temp dir");
    let path = root.path().join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    for instance_id in ["bad", "00000000-0000-1000-8000-000000000000"] {
        std::fs::write(
            &path,
            format!(r#"{{"schemaVersion":1,"port":10240,"instanceId":"{instance_id}"}}"#),
        )
        .expect("fixture");
        assert!(load_hook_relay_rendezvous_at(&path).is_err());
    }
}

/// HTTP 请求只发送最小信封，并同时在路径与请求头绑定实例身份。
#[test]
fn relay_request_is_minimal_and_instance_bound() {
    let root = tempdir().expect("temp dir");
    let path = root.path().join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let rendezvous = HookRelayRendezvous {
        schema_version: HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION,
        port: listener.local_addr().expect("address").port(),
        instance_id: Uuid::new_v4().to_string(),
    };
    persist_hook_relay_rendezvous_at(&path, &rendezvous).expect("persist");
    let response_instance_id = rendezvous.instance_id.clone();
    let expected_path = format!("/api/hooks/{}/codex", rendezvous.instance_id);
    let (request_sender, request_receiver) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        request_sender
            .send(read_http_request(&mut stream))
            .expect("send request");
        write_hook_response(&mut stream, "202 Accepted", &response_instance_id);
    });
    let payload = MinimalHookPayload {
        hook_event_name: "PostToolUse".to_owned(),
        session_id: Some("session-1".to_owned()),
        turn_id: Some("turn-2".to_owned()),
        status: None,
    };
    let body = serde_json::to_vec(&payload).expect("body");
    post_local_hook_at(&path, "codex", "PostToolUse", &body, 0, Duration::ZERO).expect("deliver");
    server.join().expect("server");
    let request = request_receiver.recv().expect("request");
    let split = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .expect("header end");
    let headers = std::str::from_utf8(&request[..split]).expect("headers");
    assert!(headers.starts_with(&format!("POST {expected_path} HTTP/1.1")));
    assert!(headers.contains(&format!("{HOOK_EVENT_TYPE_HEADER}: PostToolUse")));
    assert!(headers.contains(&format!(
        "{HOOK_RELAY_INSTANCE_HEADER}: {}",
        rendezvous.instance_id
    )));
    let received: MinimalHookPayload =
        serde_json::from_slice(&request[split + 4..]).expect("minimal payload");
    assert_eq!(received, payload);
}

/// 首次命中陈旧身份时应重读已原子更新的 rendezvous 并投递给新实例。
#[test]
fn stale_identity_response_reloads_rendezvous_before_retry() {
    let root = tempdir().expect("temp dir");
    let path = root.path().join(HOOK_RELAY_RENDEZVOUS_FILENAME);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = listener.local_addr().expect("address").port();
    let stale = HookRelayRendezvous::new(port);
    let current = HookRelayRendezvous::new(port);
    persist_hook_relay_rendezvous_at(&path, &stale).expect("persist stale");
    let server_path = path.clone();
    let server_current = current.clone();
    let server = std::thread::spawn(move || {
        let (mut first, _) = listener.accept().expect("accept stale");
        let first_request = read_http_request(&mut first);
        assert!(String::from_utf8_lossy(&first_request).contains(&stale.instance_id));
        persist_hook_relay_rendezvous_at(&server_path, &server_current).expect("publish current");
        write_hook_response(&mut first, "404 Not Found", &server_current.instance_id);
        drop(first);

        let (mut second, _) = listener.accept().expect("accept current");
        let second_request = read_http_request(&mut second);
        assert!(String::from_utf8_lossy(&second_request).contains(&server_current.instance_id));
        write_hook_response(&mut second, "202 Accepted", &server_current.instance_id);
    });
    let body = br#"{"hook_event_name":"SessionStart"}"#;
    post_local_hook_at(&path, "codex", "SessionStart", body, 1, Duration::ZERO)
        .expect("retry succeeds");
    server.join().expect("server");
    assert_eq!(
        load_hook_relay_rendezvous_at(&path).expect("current rendezvous"),
        current
    );
}
