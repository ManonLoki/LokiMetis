//! 命令型 Hook 中继：把原生 stdin 归约后可靠投递到当前 LokiMetis listener。

use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail};
use loki_metis_core::{
    HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, MAX_NATIVE_HOOK_INPUT_BYTES, PreparedNativeHook,
    managed_hook_marker, prepare_native_hook, tool_from_slug,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 触发 relay 子进程模式的命令行标志。
pub const HOOK_RELAY_ARGUMENT: &str = "--loki-metis-hook-relay";
/// listener 启动竞态下允许的额外连接尝试次数。
const LOCAL_RELAY_RETRY_COUNT: u8 = 5;
/// 每次重读 rendezvous 前的等待间隔。
const LOCAL_RELAY_RETRY_DELAY: Duration = Duration::from_secs(1);
/// 回环连接超时。
const LOCAL_RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// 单次请求写入与响应读取超时。
const LOCAL_RELAY_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// 防止异常本机服务返回无界响应。
const MAX_HOOK_HTTP_RESPONSE_BYTES: u64 = 16 * 1024;
/// 各平台用户私有缓存根下的稳定应用子目录。
const HOOK_RELAY_APP_DIRECTORY: &str = "lokimetis";

/// 严格解析后的 Hook relay 参数。
#[derive(Debug, PartialEq, Eq)]
struct HookRelayArguments {
    tool_slug: String,
    event: String,
    managed_by: String,
}

/// relay 参数形状错误；命中 relay 标志后绝不回落到 GUI 启动。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
enum RelayArgumentError {
    #[error("缺少 AI 工具参数")]
    MissingTool,
    #[error("缺少 Hook 事件参数")]
    MissingEvent,
    #[error("缺少 --managed-by 参数或其值")]
    MissingMarker,
    #[error("Hook relay 参数不是有效 UTF-8")]
    InvalidUtf8,
    #[error("存在重复或无法识别的 Hook relay 参数")]
    UnexpectedArgument,
}

/// listener 与短生命周期 relay/plugin 共享的实例化端点描述。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HookRelayRendezvous {
    pub(super) schema_version: u8,
    pub(super) port: u16,
    pub(super) instance_id: String,
}

impl HookRelayRendezvous {
    /// 为刚完成绑定的 listener 生成不可误投到其他本机服务的实例身份。
    pub(super) fn new(port: u16) -> Self {
        Self {
            schema_version: HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION,
            port,
            instance_id: Uuid::new_v4().to_string(),
        }
    }

    /// 严格校验跨进程描述，损坏或旧版本文件不得回退到固定端口。
    fn validate(self) -> anyhow::Result<Self> {
        if self.schema_version != HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION {
            bail!("Hook relay rendezvous 版本不受支持");
        }
        if self.port == 0 {
            bail!("Hook relay rendezvous 端口无效");
        }
        let parsed =
            Uuid::parse_str(&self.instance_id).context("Hook relay rendezvous 实例身份无效")?;
        if parsed.get_version_num() != 4 || parsed.to_string() != self.instance_id {
            bail!("Hook relay rendezvous 实例身份不是规范 UUID v4");
        }
        Ok(self)
    }
}

/// 若当前进程是 Hook relay 模式则执行并返回退出码。
pub fn run_hook_relay_if_requested() -> Option<i32> {
    let arguments = match parse_relay_arguments(std::env::args_os())? {
        Ok(arguments) => arguments,
        Err(error) => {
            report_relay_error(&error);
            return Some(2);
        }
    };
    let tool = match tool_from_slug(&arguments.tool_slug) {
        Some(tool) if arguments.managed_by == managed_hook_marker(tool) => tool,
        Some(_) => {
            report_relay_error(&anyhow!("Hook relay 的管理标识与 AI 工具不匹配"));
            return Some(1);
        }
        None => {
            report_relay_error(&anyhow!("Hook relay 不支持该 AI 工具"));
            return Some(1);
        }
    };
    match run_relay(tool, &arguments.tool_slug, &arguments.event) {
        Ok(()) => Some(0),
        Err(error) => {
            report_relay_error(&error);
            Some(relay_failure_exit_code(tool))
        }
    }
}

/// Copilot 的 preToolUse 会把非零 Hook 退出码当成拒绝，旁路监控必须 fail-open。
fn relay_failure_exit_code(tool: loki_metis_core::AiTool) -> i32 {
    i32::from(tool != loki_metis_core::AiTool::GitHubCopilot)
}

/// 只在 argv[1] 精确命中 relay 标志时解析，其后参数必须完整且唯一。
fn parse_relay_arguments<I, T>(
    arguments: I,
) -> Option<Result<HookRelayArguments, RelayArgumentError>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut arguments = arguments.into_iter();
    let _executable = arguments.next()?;
    let first_argument: OsString = arguments.next()?.into();
    if first_argument != OsStr::new(HOOK_RELAY_ARGUMENT) {
        return None;
    }
    let result = (|| {
        let tool_slug = arguments
            .next()
            .ok_or(RelayArgumentError::MissingTool)?
            .into()
            .into_string()
            .map_err(|_| RelayArgumentError::InvalidUtf8)?;
        let mut event = None;
        let mut managed_by = None;
        while let Some(argument) = arguments.next() {
            let argument = argument
                .into()
                .into_string()
                .map_err(|_| RelayArgumentError::InvalidUtf8)?;
            if argument == "--managed-by" {
                if managed_by.is_some() {
                    return Err(RelayArgumentError::UnexpectedArgument);
                }
                managed_by = Some(
                    arguments
                        .next()
                        .ok_or(RelayArgumentError::MissingMarker)?
                        .into()
                        .into_string()
                        .map_err(|_| RelayArgumentError::InvalidUtf8)?,
                );
            } else if argument.starts_with('-') || event.replace(argument).is_some() {
                return Err(RelayArgumentError::UnexpectedArgument);
            }
        }
        Ok(HookRelayArguments {
            tool_slug,
            event: event.ok_or(RelayArgumentError::MissingEvent)?,
            managed_by: managed_by.ok_or(RelayArgumentError::MissingMarker)?,
        })
    })();
    Some(result)
}

/// 读取 stdin、生成最小信封并按领域决策投递或返回合法空 JSON。
fn run_relay(tool: loki_metis_core::AiTool, tool_slug: &str, event: &str) -> anyhow::Result<()> {
    let mut stdin = Vec::new();
    std::io::stdin()
        .take(MAX_NATIVE_HOOK_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut stdin)
        .context("无法读取 AI Hook 原始输入")?;
    let prepared = prepare_native_hook(tool, &stdin, event).context("无法处理 AI Hook 原始载荷")?;
    if let PreparedNativeHook::Deliver(payload) = &prepared {
        let body = serde_json::to_vec(payload).context("无法编码 Hook 最小信封")?;
        post_local_hook(tool_slug, &payload.hook_event_name, &body)?;
    }
    if requires_json_stdout(tool, &prepared) {
        write_empty_json_response(&mut std::io::stdout())?;
    }
    Ok(())
}

/// Cursor 兼容抑制，以及 Cursor/Qwen/Gemini 原生协议，都要求 stdout 是合法 JSON。
fn requires_json_stdout(tool: loki_metis_core::AiTool, prepared: &PreparedNativeHook) -> bool {
    matches!(prepared, PreparedNativeHook::SuppressForeignHost)
        || matches!(
            tool,
            loki_metis_core::AiTool::Cursor
                | loki_metis_core::AiTool::QwenCode
                | loki_metis_core::AiTool::GeminiCli
        )
}

/// 写出不带决策字段的合法 JSON，绝不把 LokiMetis 状态混入第三方 Hook 响应。
fn write_empty_json_response(writer: &mut impl Write) -> anyhow::Result<()> {
    writer
        .write_all(b"{}\n")
        .context("无法写入 AI Hook JSON 响应")
}

/// 同时写到 Hook 宿主 stderr 与应用 tracing（若当前进程已安装 subscriber）。
fn report_relay_error(error: &dyn std::fmt::Display) {
    eprintln!("LokiMetis Hook relay: {error}");
    tracing::error!(target: "loki_metis::hook_relay", error = %error, "hook relay failed");
}

/// 支持的宿主路径规则；独立枚举使三平台单一路径可在任意 CI 宿主验证。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HookRelayHostPlatform {
    #[cfg(any(test, target_os = "linux"))]
    Linux,
    #[cfg(any(test, target_os = "macos"))]
    Macos,
    #[cfg(any(test, target_os = "windows"))]
    Windows,
}

/// 只有绝对且不是文件系统根本身的环境目录才可承载用户私有 rendezvous。
fn usable_absolute_root(path: Option<&Path>) -> Option<&Path> {
    path.filter(|path| path.is_absolute() && path.parent().is_some())
}

/// 按宿主选择稳定且唯一的每用户 rendezvous 路径。
fn select_hook_relay_rendezvous_path(
    platform: HookRelayHostPlatform,
    _xdg_runtime_dir: Option<&Path>,
    _home: Option<&Path>,
    _local_app_data: Option<&Path>,
    _user_profile: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let directory = match platform {
        #[cfg(any(test, target_os = "linux"))]
        HookRelayHostPlatform::Linux => {
            let root = usable_absolute_root(_home)
                .ok_or_else(|| anyhow!("Linux Hook relay 缺少绝对 HOME"))?;
            root.join(".cache").join(HOOK_RELAY_APP_DIRECTORY)
        }
        #[cfg(any(test, target_os = "macos"))]
        HookRelayHostPlatform::Macos => {
            let root = usable_absolute_root(_home)
                .ok_or_else(|| anyhow!("macOS Hook relay 缺少绝对 HOME"))?;
            root.join("Library")
                .join("Caches")
                .join(HOOK_RELAY_APP_DIRECTORY)
        }
        #[cfg(any(test, target_os = "windows"))]
        HookRelayHostPlatform::Windows => {
            let root = usable_absolute_root(_user_profile)
                .ok_or_else(|| anyhow!("Windows Hook relay 缺少绝对 USERPROFILE"))?;
            root.join("AppData")
                .join("Local")
                .join(HOOK_RELAY_APP_DIRECTORY)
        }
    };
    Ok(directory.join(HOOK_RELAY_RENDEZVOUS_FILENAME))
}

/// 本机中继 rendezvous 的稳定每用户位置；缺少可信绝对根时明确失败。
pub(super) fn hook_relay_rendezvous_path() -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Linux,
            None,
            home.as_deref(),
            None,
            None,
        );
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Macos,
            None,
            home.as_deref(),
            None,
            None,
        );
    }
    #[cfg(target_os = "windows")]
    {
        let user_profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
        return select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Windows,
            None,
            None,
            None,
            user_profile.as_deref(),
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow!("当前平台不支持 Hook relay rendezvous"))
    }
}

/// 创建并验证 rendezvous 私有父目录；Unix 上强制收紧到 0700。
fn ensure_private_rendezvous_directory(path: &Path) -> anyhow::Result<()> {
    let directory = path
        .parent()
        .filter(|directory| directory.is_absolute())
        .ok_or_else(|| anyhow!("Hook relay rendezvous 父目录不是绝对路径"))?;
    std::fs::create_dir_all(directory).context("无法创建 Hook relay 私有目录")?;
    let metadata = std::fs::symlink_metadata(directory).context("无法检查 Hook relay 私有目录")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Hook relay 私有目录不是普通目录");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .context("无法收紧 Hook relay 私有目录权限")?;
    }
    Ok(())
}

/// 原子发布 listener 端口与本次实例身份。
pub(super) fn persist_hook_relay_rendezvous_at(
    path: &Path,
    rendezvous: &HookRelayRendezvous,
) -> anyhow::Result<()> {
    ensure_private_rendezvous_directory(path)?;
    let payload = serde_json::to_vec(rendezvous).context("无法编码 Hook relay rendezvous")?;
    super::atomic_file::write_monitor_file_atomically(
        path,
        &payload,
        "error.hookRelay.rendezvousWriteFailed",
    )
    .context("无法原子发布 Hook relay rendezvous")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("无法收紧 Hook relay rendezvous 权限")?;
    }
    Ok(())
}

/// 严格读取当前 listener 描述；缺失或损坏不得猜测任何固定端口。
fn load_hook_relay_rendezvous_at(path: &Path) -> anyhow::Result<HookRelayRendezvous> {
    let payload = std::fs::read(path).context("无法读取 Hook relay rendezvous")?;
    serde_json::from_slice::<HookRelayRendezvous>(&payload)
        .context("无法解析 Hook relay rendezvous")?
        .validate()
}

/// 投递请求的回环套接字地址。
fn hook_relay_connect_addr(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port))
}

/// 把请求头和最小 JSON 正文编码成一次写出的 HTTP/1.1 缓冲。
fn encode_local_hook_request(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    rendezvous: &HookRelayRendezvous,
) -> Vec<u8> {
    let host = format!("127.0.0.1:{}", rendezvous.port);
    let path = format!("/api/hooks/{}/{slug}", rendezvous.instance_id);
    let header = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\n{HOOK_EVENT_TYPE_HEADER}: {hook_type}\r\n{HOOK_RELAY_INSTANCE_HEADER}: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        rendezvous.instance_id,
        body.len()
    );
    let mut request = Vec::with_capacity(header.len() + body.len());
    request.extend_from_slice(header.as_bytes());
    request.extend_from_slice(body);
    request
}

/// 一次请求的失败分类；只有确定未入队的场景才允许重试。
enum PostAttemptError {
    Retryable(anyhow::Error),
    Fatal(anyhow::Error),
}

/// POST 到当前 listener；每次重试都重读 rendezvous 以跟随启动或重绑。
fn post_local_hook(slug: &str, hook_type: &str, body: &[u8]) -> anyhow::Result<()> {
    let rendezvous_path = hook_relay_rendezvous_path()?;
    post_local_hook_at(
        &rendezvous_path,
        slug,
        hook_type,
        body,
        LOCAL_RELAY_RETRY_COUNT,
        LOCAL_RELAY_RETRY_DELAY,
    )
}

/// 可注入文件与等待策略的可靠投递实现，确保测试不触碰真实 temp 文件。
fn post_local_hook_at(
    rendezvous_path: &Path,
    slug: &str,
    hook_type: &str,
    body: &[u8],
    retry_count: u8,
    retry_delay: Duration,
) -> anyhow::Result<()> {
    let mut last_retryable = None;
    for attempt in 0..=retry_count {
        let outcome = match load_hook_relay_rendezvous_at(rendezvous_path) {
            Ok(rendezvous) => post_local_hook_once(slug, hook_type, body, &rendezvous),
            Err(error) => Err(PostAttemptError::Retryable(error)),
        };
        match outcome {
            Ok(()) => return Ok(()),
            Err(PostAttemptError::Fatal(error)) => return Err(error),
            Err(PostAttemptError::Retryable(error)) => last_retryable = Some(error),
        }
        if attempt < retry_count {
            thread::sleep(retry_delay);
        }
    }
    Err(last_retryable.unwrap_or_else(|| anyhow!("Hook relay 重试耗尽"))).context(format!(
        "无法连接 LokiMetis Hook listener（共尝试 {} 次）",
        retry_count + 1
    ))
}

/// 向一个已验证描述执行单次 HTTP 投递并校验响应实例身份。
fn post_local_hook_once(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    rendezvous: &HookRelayRendezvous,
) -> Result<(), PostAttemptError> {
    post_local_hook_once_with_timeout(
        slug,
        hook_type,
        body,
        rendezvous,
        LOCAL_RELAY_REQUEST_TIMEOUT,
    )
}

/// 使用单一 wall-clock 截止时间执行连接、写入与完整响应读取。
fn post_local_hook_once_with_timeout(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    rendezvous: &HookRelayRendezvous,
    request_timeout: Duration,
) -> Result<(), PostAttemptError> {
    let started_at = Instant::now();
    let connect_timeout = LOCAL_RELAY_CONNECT_TIMEOUT.min(request_timeout);
    let mut stream =
        TcpStream::connect_timeout(&hook_relay_connect_addr(rendezvous.port), connect_timeout)
            .map_err(|error| {
                PostAttemptError::Retryable(anyhow!(error).context("无法连接 Hook listener"))
            })?;
    stream.set_nodelay(true).map_err(|error| {
        PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook listener 连接"))
    })?;
    let request = encode_local_hook_request(slug, hook_type, body, rendezvous);
    write_hook_request_before_deadline(&mut stream, &request, started_at, request_timeout)?;
    let response = read_hook_response_before_deadline(&mut stream, started_at, request_timeout)?;
    validate_hook_response(&response, &rendezvous.instance_id)
}

/// 返回本次请求的剩余预算；耗尽后不再执行任何阻塞 I/O。
fn remaining_request_budget(
    started_at: Instant,
    request_timeout: Duration,
) -> Result<Duration, PostAttemptError> {
    request_timeout
        .checked_sub(started_at.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 请求超过总时限")))
}

/// 手动推进写入，每次系统调用都只获得整个请求剩余的 wall-clock 预算。
fn write_hook_request_before_deadline(
    stream: &mut TcpStream,
    mut request: &[u8],
    started_at: Instant,
    request_timeout: Duration,
) -> Result<(), PostAttemptError> {
    while !request.is_empty() {
        let remaining = remaining_request_budget(started_at, request_timeout)?;
        stream.set_write_timeout(Some(remaining)).map_err(|error| {
            PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook 请求写入截止时间"))
        })?;
        match stream.write(request) {
            Ok(0) => {
                return Err(PostAttemptError::Fatal(anyhow!(
                    "Hook listener 在请求写完前关闭连接"
                )));
            }
            Ok(written) => request = &request[written..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(PostAttemptError::Fatal(
                    anyhow!(error).context("无法写入 Hook 请求"),
                ));
            }
        }
    }
    Ok(())
}

/// 读取到连接结束，同时让持续小流量也不能刷新整次请求的总时限。
fn read_hook_response_before_deadline(
    stream: &mut TcpStream,
    started_at: Instant,
    request_timeout: Duration,
) -> Result<Vec<u8>, PostAttemptError> {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let remaining = remaining_request_budget(started_at, request_timeout)?;
        stream.set_read_timeout(Some(remaining)).map_err(|error| {
            PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook 响应读取截止时间"))
        })?;
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(response),
            Ok(read) => {
                response.extend_from_slice(&chunk[..read]);
                if response.len() as u64 > MAX_HOOK_HTTP_RESPONSE_BYTES {
                    return Err(PostAttemptError::Fatal(anyhow!("Hook listener 响应过大")));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(PostAttemptError::Fatal(
                    anyhow!(error).context("无法读取 Hook listener 响应"),
                ));
            }
        }
    }
}

/// 校验 2xx 状态与响应实例头，避免陈旧端口误把事件提交给 AIMonitor。
fn validate_hook_response(
    response: &[u8],
    expected_instance_id: &str,
) -> Result<(), PostAttemptError> {
    let response = std::str::from_utf8(response)
        .map_err(|_| PostAttemptError::Fatal(anyhow!("Hook listener 响应不是 UTF-8")))?;
    let headers = response
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers)
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 响应不完整")))?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 响应状态无效")))?;
    let response_instance_id = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(HOOK_RELAY_INSTANCE_HEADER)
            .then(|| value.trim())
    });
    if response_instance_id != Some(expected_instance_id) {
        return Err(PostAttemptError::Retryable(anyhow!(
            "Hook listener 响应实例身份不匹配"
        )));
    }
    match status {
        200..=299 => Ok(()),
        503 => Err(PostAttemptError::Retryable(anyhow!(
            "Hook listener 暂时不可用：HTTP 503"
        ))),
        _ => Err(PostAttemptError::Fatal(anyhow!(
            "Hook listener 拒绝事件：HTTP {status}"
        ))),
    }
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
