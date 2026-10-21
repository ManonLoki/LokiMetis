//! 受限的 curl 与 sqlite3 调用。令牌只进入 curl 配置的标准输入，不进入参数向量。

use std::{path::{Path, PathBuf}, time::Duration};

use loki_metris_core::QuotaFailure;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command, sync::watch};

/// 单次外部读取的时间与输出上限。
#[derive(Clone, Copy)]
pub(crate) struct CurlLimits {
    /// 从启动到必须回收的时间。
    pub timeout: Duration,
    /// 标准输出字节上限，超过即失败并结束进程。
    pub max_body: usize,
}

impl CurlLimits {
    /// 生产读取的默认上限。
    pub(crate) fn production() -> Self {
        Self { timeout: Duration::from_secs(15), max_body: 256 * 1024 }
    }
}

/// 发往允许主机的一次 HTTPS 请求。
pub(crate) struct HttpRequest {
    /// `GET` 或 `POST`。
    pub method: &'static str,
    /// 完整 HTTPS URL。
    pub url: String,
    /// 请求头。值在写入配置前再次检查。
    pub headers: Vec<(&'static str, String)>,
    /// POST 正文。
    pub body: Option<String>,
}

/// curl 的参数与标准输入配置。参数中没有令牌。
pub(crate) struct CurlExchange {
    /// 仅 `--config` 与 `-`。
    pub args: Vec<String>,
    /// 传给 curl 标准输入的配置。
    pub config: String,
}

/// 带状态码的响应正文。
pub(crate) struct HttpResponse {
    /// HTTP 状态码。
    pub status: u16,
    /// 去掉状态标记后的正文。
    pub body: String,
}

const ALLOWED_HOSTS: &[&str] = &[
    "chatgpt.com",
    "cursor.com",
    "cli-chat-proxy.grok.com",
    "www.codebuddy.cn",
    "www.workbuddy.ai",
];

/// Cursor 只读查询。语句是常量，不含令牌。
pub(crate) const CURSOR_TOKEN_SQL: &str = "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1;";

/// 本机缓存的用量摘要。没有这些键就视为本地未获取。
pub(crate) const CURSOR_USAGE_SQL: &str = "SELECT value FROM ItemTable WHERE key IN ('cursorai/usageSummary','cursorAuth/usageSummary') LIMIT 1;";

/// 在 PATH 中查找同名可执行文件，不经过命令解释器。
pub(crate) fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
}

/// 生成 curl 配置。主机不在允许列表、或头的值不能安全引用时直接失败。
pub(crate) fn build_curl_exchange(request: &HttpRequest, limits: &CurlLimits) -> Result<CurlExchange, QuotaFailure> {
    allow_url(&request.url)?;
    if request.method != "GET" && request.method != "POST" {
        return Err(QuotaFailure::Transport);
    }
    let mut config = String::new();
    push(&mut config, "silent");
    push(&mut config, "show-error");
    push(&mut config, &format!("max-time = {}", limits.timeout.as_secs().clamp(1, 60)));
    push(&mut config, "proto = \"=https\"");
    push(&mut config, "proto-redir = \"=https\"");
    push(&mut config, "max-redirs = 0");
    push(&mut config, &format!("url = {}", curl_quote(&request.url)?));
    push(&mut config, &format!("request = {}", curl_quote(request.method)?));
    for (name, value) in &request.headers {
        if !header_name_safe(name) || !header_value_safe(value) {
            return Err(QuotaFailure::Transport);
        }
        push(&mut config, &format!("header = {}", curl_quote(&format!("{name}: {value}"))?));
    }
    if let Some(body) = &request.body {
        push(&mut config, &format!("data-binary = {}", curl_quote(body)?));
    }
    push(&mut config, "output = \"-\"");
    push(&mut config, r#"write-out = "\n__LM_HTTP_%{http_code}""#);
    Ok(CurlExchange { args: vec!["--config".to_owned(), "-".to_owned()], config })
}

/// sqlite3 的只读参数。路径不能以 `-` 开头，也不能含控制字符。
pub(crate) fn cursor_command_args(database: &Path) -> Result<Vec<String>, QuotaFailure> {
    cursor_query_args(database, CURSOR_TOKEN_SQL)
}

/// 用固定查询读取 Cursor 状态库。查询本身不能含控制字符。
pub(crate) fn cursor_query_args(database: &Path, sql: &str) -> Result<Vec<String>, QuotaFailure> {
    if sql.is_empty() || sql.chars().any(|ch| ch.is_ascii_control()) {
        return Err(QuotaFailure::Transport);
    }
    let path = database.to_str().filter(|path| !path.starts_with('-') && !path.chars().any(|ch| ch.is_ascii_control())).ok_or(QuotaFailure::Transport)?;
    Ok(vec!["-readonly".to_owned(), "-batch".to_owned(), "-noheader".to_owned(), path.to_owned(), sql.to_owned()])
}

/// 执行已构造的参数向量。取消、超时和输出超限都会结束子进程。
pub(crate) async fn run_command(program: &Path, args: &[String], stdin: Option<String>, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<Vec<u8>, QuotaFailure> {
    if !program.is_file() {
        return Err(QuotaFailure::Transport);
    }
    let mut command = Command::new(program);
    command.args(args).kill_on_drop(true).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(unix)]
    { command.process_group(0); }
    let mut child = command.spawn().map_err(|_| QuotaFailure::Transport)?;
    if let Some(payload) = stdin {
        let mut pipe = child.stdin.take().ok_or(QuotaFailure::Transport)?;
        pipe.write_all(payload.as_bytes()).await.map_err(|_| QuotaFailure::Transport)?;
        pipe.shutdown().await.map_err(|_| QuotaFailure::Transport)?;
    } else {
        drop(child.stdin.take());
    }
    let mut stdout = child.stdout.take().ok_or(QuotaFailure::Transport)?;
    let stderr = child.stderr.take();
    let drain = tokio::spawn(async move { drain_stderr(stderr).await; });
    let read = read_capped(&mut stdout, limits.max_body);
    tokio::pin!(read);
    let read_result = tokio::select! {
        biased;
        _ = watch_cancelled(cancel) => Err(QuotaFailure::Transport),
        _ = tokio::time::sleep(limits.timeout) => Err(QuotaFailure::Transport),
        value = &mut read => value,
    };
    if read_result.is_err() {
        stop_child(&mut child).await;
    } else {
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
    drain.abort();
    read_result
}

/// 拆出 curl `write-out` 附上的状态码。没有标记视为传输失败。
pub(crate) fn split_http(bytes: &[u8]) -> Result<HttpResponse, QuotaFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| QuotaFailure::Malformed)?;
    let (body, code) = text.rsplit_once("\n__LM_HTTP_").ok_or(QuotaFailure::Transport)?;
    let status = code.trim().parse::<u16>().map_err(|_| QuotaFailure::Transport)?;
    if status == 0 {
        return Err(QuotaFailure::Transport);
    }
    Ok(HttpResponse { status, body: body.to_owned() })
}

fn allow_url(url: &str) -> Result<(), QuotaFailure> {
    let rest = url.strip_prefix("https://").ok_or(QuotaFailure::Transport)?;
    if rest.contains('@') || rest.contains('\\') || rest.chars().any(|ch| ch.is_ascii_control()) {
        return Err(QuotaFailure::Transport);
    }
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_port.rsplit_once(':').map(|(host, _)| host).unwrap_or(host_port);
    if ALLOWED_HOSTS.iter().any(|allowed| allowed.eq_ignore_ascii_case(host)) { Ok(()) } else { Err(QuotaFailure::Transport) }
}

pub(crate) fn curl_quote(value: &str) -> Result<String, QuotaFailure> {
    if value.chars().any(|ch| ch == '\n' || ch == '\r' || ch == '\0') {
        return Err(QuotaFailure::Transport);
    }
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        if ch == '\\' || ch == '"' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    Ok(quoted)
}

fn header_name_safe(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn header_value_safe(value: &str) -> bool {
    !value.is_empty() && value.len() <= 8192 && value.chars().all(|ch| ch == ' ' || ch == '\t' || (ch.is_ascii() && !ch.is_ascii_control() && ch != '"' && ch != '\\'))
}

fn push(config: &mut String, line: &str) {
    config.push_str(line);
    config.push('\n');
}

async fn read_capped(stdout: &mut tokio::process::ChildStdout, max_body: usize) -> Result<Vec<u8>, QuotaFailure> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = stdout.read(&mut chunk).await.map_err(|_| QuotaFailure::Transport)?;
        if read == 0 {
            return Ok(buffer);
        }
        if buffer.len().saturating_add(read) > max_body {
            return Err(QuotaFailure::Transport);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

async fn drain_stderr(stderr: Option<tokio::process::ChildStderr>) {
    let Some(mut stderr) = stderr else { return };
    let mut chunk = [0_u8; 1024];
    let mut total = 0_usize;
    while total < 8 * 1024 {
        match stderr.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => total = total.saturating_add(read),
        }
    }
}

async fn watch_cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}

async fn stop_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}
