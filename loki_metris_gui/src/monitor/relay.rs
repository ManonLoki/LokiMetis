//! 命令型 Hook 中继：把原生 stdin 归约后可靠投递到当前 LokiMetris listener。

use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail};
use loki_metris_core::{
    HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, MAX_NATIVE_HOOK_INPUT_BYTES, PreparedNativeHook,
    managed_hook_marker, prepare_native_hook, tool_from_slug,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 触发 relay 子进程模式的命令行标志。
pub const HOOK_RELAY_ARGUMENT: &str = "--loki-metris-hook-relay";
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
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
const HOOK_RELAY_APP_DIRECTORY: &str = "lokimetris";
/// Windows 当前用户安装目录是 `LokiMetris`；缓存命名空间必须避免
/// 在大小写不敏感文件系统上与其别名。
#[cfg(any(test, target_os = "windows"))]
const WINDOWS_HOOK_RELAY_CACHE_NAMESPACE: &str = "com.manonloki.lokimetris";

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
fn relay_failure_exit_code(tool: loki_metris_core::AiTool) -> i32 {
    i32::from(tool != loki_metris_core::AiTool::GitHubCopilot)
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
fn run_relay(tool: loki_metris_core::AiTool, tool_slug: &str, event: &str) -> anyhow::Result<()> {
    relay_native_input(tool, tool_slug, event, &mut std::io::stdin(), &mut std::io::stdout(), post_local_hook)
}

/// 原生载荷读取、隐私归约与投递共用一个路径，隔离回归可注入自己的端点传输。
fn relay_native_input(
    tool: loki_metris_core::AiTool,
    tool_slug: &str,
    event: &str,
    reader: &mut impl Read,
    writer: &mut impl Write,
    post: impl FnOnce(&str, &str, &[u8]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut stdin = Vec::new();
    reader
        .take(MAX_NATIVE_HOOK_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut stdin)
        .context("无法读取 AI Hook 原始输入")?;
    let prepared = prepare_native_hook(tool, &stdin, event).context("无法处理 AI Hook 原始载荷")?;
    deliver_prepared(tool, tool_slug, &prepared, writer, post)
}

/// 必需的原生 JSON 回应独立于旁路投递结果，网络失败不能破坏宿主协议。
fn deliver_prepared(tool:loki_metris_core::AiTool,slug:&str,prepared:&PreparedNativeHook,writer:&mut impl Write,post:impl FnOnce(&str,&str,&[u8])->anyhow::Result<()>)->anyhow::Result<()> {
    let delivery=if let PreparedNativeHook::Deliver(payload)=prepared {
        serde_json::to_vec(payload).context("无法编码 Hook 最小信封").and_then(|body|post(slug,&payload.hook_event_name,&body))
    } else {Ok(())};
    if requires_json_stdout(tool,prepared) {write_empty_json_response(writer)?;}
    delivery
}

/// Cursor 兼容抑制，以及 Cursor/Qwen/Gemini 原生协议，都要求 stdout 是合法 JSON。
fn requires_json_stdout(tool: loki_metris_core::AiTool, prepared: &PreparedNativeHook) -> bool {
    matches!(prepared, PreparedNativeHook::SuppressForeignHost)
        || matches!(
            tool,
            loki_metris_core::AiTool::Cursor
                | loki_metris_core::AiTool::QwenCode
                | loki_metris_core::AiTool::GeminiCli
        )
}

/// 写出不带决策字段的合法 JSON，绝不把 LokiMetris 状态混入第三方 Hook 响应。
fn write_empty_json_response(writer: &mut impl Write) -> anyhow::Result<()> {
    writer
        .write_all(b"{}\n")
        .context("无法写入 AI Hook JSON 响应")
}

/// 同时写到 Hook 宿主 stderr 与应用 tracing（若当前进程已安装 subscriber）。
fn report_relay_error(error: &dyn std::fmt::Display) {
    eprintln!("LokiMetris Hook relay: {error}");
    tracing::error!(target: "loki_metris::hook_relay", error = %error, "hook relay failed");
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
            let root = usable_absolute_root(_local_app_data)
                .ok_or_else(|| anyhow!("Windows Hook relay 缺少绝对 LOCALAPPDATA"))?;
            root.join(WINDOWS_HOOK_RELAY_CACHE_NAMESPACE)
        }
    };
    Ok(directory.join(HOOK_RELAY_RENDEZVOUS_FILENAME))
}

/// 本机中继 rendezvous 的稳定每用户位置；缺少可信绝对根时明确失败。
pub(crate) fn hook_relay_rendezvous_path() -> anyhow::Result<PathBuf> {
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
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Macos,
            None,
            home.as_deref(),
            None,
            None,
        )
    }
    #[cfg(target_os = "windows")]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        select_hook_relay_rendezvous_path(
            HookRelayHostPlatform::Windows,
            None,
            None,
            local_app_data.as_deref(),
            None,
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow!("当前平台不支持 Hook relay rendezvous"))
    }
}

/// 创建并验证 rendezvous 私有父目录；Unix 上强制收紧到 0700。
pub(super) fn ensure_private_rendezvous_directory(path: &Path) -> anyhow::Result<()> {
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
pub(super) fn load_hook_relay_rendezvous_at(path: &Path) -> anyhow::Result<HookRelayRendezvous> {
    let metadata=std::fs::symlink_metadata(path).context("无法检查 Hook relay rendezvous")?;
    if !metadata.is_file() || metadata.len()>1024 {bail!("Hook relay rendezvous 文件类型或大小无效");}
    let mut payload=Vec::new();
    std::fs::File::open(path)?.take(1025).read_to_end(&mut payload).context("无法读取 Hook relay rendezvous")?;
    if payload.len()>1024 {bail!("Hook relay rendezvous 大小无效");}
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - relay_delivery.rs：可注入文件与等待策略的可靠投递实现，确保测试不触碰真实 temp 文件。；向一个已验证描述执行单次 HTTP 投递并校验响应实例身份。；使用单一 wall-clock 截止时间执行连接、写入与完整响应读取。
include!("relay_delivery.rs");

#[cfg(all(test, target_os = "linux"))]
#[path = "relay_command_tests.rs"]
mod command_tests;
