//! 本机 Hook 配置目录、受管多文件原子写入与生命周期补写 worker。

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookConfigLocation, HookConfigPreview, HookConfigWriteResult,
    HookError, ai_tool_name, generate_hook_auxiliary_configs, generate_hook_config,
    generate_wsl_hook_config, hook_config_filename, hook_config_write_result, hook_supports_wsl,
    normalize_enabled_ai_tools, public_monitor_ai_tools,
};

use super::{settings::MonitorSettings, thread_owner::RetainedThreadOwner, wsl::WslDirectory};

/// 串行化显式写入与后台自动补写，避免同一工具配置被并发读改写覆盖。
static HOOK_CONFIG_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
/// 超过同步退出预算的 Hook writer 仍由后台 owner 持有并在空闲期 join。
static RETAINED_HOOK_WRITERS: OnceLock<RetainedThreadOwner> = OnceLock::new();
/// 外部 Hook writer 停止后，后台自愈在此期限内重新收敛受管条目。
const AUTOMATIC_REPAIR_INTERVAL: Duration = Duration::from_secs(5);
/// 退出回调等待 Hook writer 主动收敛的上限；不可中断的本机文件系统调用最终由进程退出回收。
const HOOK_WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// 等待具名线程退出时的短轮询间隔。
const HOOK_WRITER_SHUTDOWN_POLL: Duration = Duration::from_millis(10);
/// 显式写入等待 owner 完成的上限；覆盖一个在途 WSL 写入和本次 WSL 总时限。
const EXPLICIT_HOOK_WRITE_TIMEOUT: Duration = Duration::from_secs(35);

/// 一次由 UI 明确请求、但由生命周期 worker 实际执行的 Hook 配置写入。
struct ExplicitHookWriteRequest {
    /// 监控设置目录只作为受信路径解析结果传入，读取发生在 worker 线程。
    config_directory: PathBuf,
    /// 用户选择写入的公开工具。
    tool: AiTool,
    /// IPC 取消、超时或应用退出时由 owner 观察的单请求令牌。
    cancellation: Arc<AtomicBool>,
    /// 把 worker 结果送回异步 Tauri command；接收端消失不改变 worker 所有权。
    response: oneshot::Sender<Result<HookConfigWriteResult, HookError>>,
}

/// Hook writer 每轮只取一项工作，显式用户请求优先于周期自愈。
enum HookWriterWork {
    /// 必须返回结构化结果的显式写入。
    Explicit(ExplicitHookWriteRequest),
    /// 无需阻塞 UI 的最新设置自愈。
    AutomaticRepair(MonitorSettings),
}

/// 异步等待被取消时同步标记对应请求，避免 worker 变成 detached 写入。
struct HookWriteCancellationGuard {
    /// 与排队请求共享的取消标志。
    cancellation: Arc<AtomicBool>,
    /// worker 已返回结果后解除，避免把已完成请求误标为取消。
    armed: bool,
}

impl HookWriteCancellationGuard {
    /// 为已成功排队的请求建立取消所有权。
    fn new(cancellation: Arc<AtomicBool>) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    /// worker 已完成并交付结果后解除取消动作。
    fn disarm(&mut self) {
        self.armed = false;
    }

    /// 到达响应时限时立即发布取消，但继续持有守卫直到 worker 返回终态。
    fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }
}

impl Drop for HookWriteCancellationGuard {
    /// future 被取消或超时时通知仍由生命周期 owner 持有的阻塞工作。
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.store(true, Ordering::Release);
        }
    }
}

/// Hook writer 的请求状态；自动设置合并，显式写入保持逐次结果。
#[derive(Default)]
struct HookWriterState {
    /// 最新设置快照；保留后供低频内容感知自愈重复检查。
    settings: Option<MonitorSettings>,
    /// 保存或启动请求要求立即检查，而不是等待下一个周期。
    repair_requested: bool,
    /// 等待单一具名 worker 执行的显式写入，禁止为每个 IPC 创建 detached 线程。
    explicit_requests: VecDeque<ExplicitHookWriteRequest>,
    /// worker 当前正在执行的显式请求，供应用退出同步触发取消。
    active_explicit_cancellation: Option<Arc<AtomicBool>>,
    /// 应用退出后阻止接受和执行更多写入。
    shutting_down: bool,
}

/// 由 Tauri 应用生命周期拥有的 Hook 显式写入与自动补写 worker。
pub struct HookConfigWriter {
    /// worker 与调用方共享的请求状态和唤醒信号。
    shared: Option<Arc<(Mutex<HookWriterState>, Condvar)>>,
    /// 应用退出时必须回收的具名线程。
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    /// 退出时同步打断 worker 正在等待的受管 WSL 子进程。
    cancellation: Option<Arc<AtomicBool>>,
}

impl HookConfigWriter {
    /// 使用 Tauri 解析的用户主目录创建 worker；初始化失败不阻断 GUI 启动。
    pub fn new(home_directory: PathBuf) -> Self {
        let relay_executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(error = %error, "failed to locate executable for automatic hooks");
                return Self::disabled();
            }
        };
        Self::start(relay_executable, home_directory)
    }

    /// 用确定的 relay 与主目录启动 worker，生产构造与隔离测试复用同一生命周期。
    fn start(relay_executable: PathBuf, home_directory: PathBuf) -> Self {
        Self::start_with_interval(relay_executable, home_directory, AUTOMATIC_REPAIR_INTERVAL)
    }

    /// 用可注入周期启动 worker，隔离测试无需等待生产自愈间隔。
    fn start_with_interval(
        relay_executable: PathBuf,
        home_directory: PathBuf,
        repair_interval: Duration,
    ) -> Self {
        let shared = Arc::new((Mutex::new(HookWriterState::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let worker = match thread::Builder::new()
            .name("loki-metis-hook-writer".to_owned())
            .spawn(move || {
                hook_config_writer_loop(
                    &worker_shared,
                    &relay_executable,
                    &home_directory,
                    repair_interval,
                    &worker_cancellation,
                )
            }) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(error = %error, "failed to start hook config writer");
                return Self::disabled();
            }
        };
        Self {
            shared: Some(shared),
            worker: Mutex::new(Some(worker)),
            cancellation: Some(cancellation),
        }
    }

    /// 合并排队当前已启用工具的最新设置；不会因磁盘或 WSL I/O 阻塞 UI。
    pub fn request_enabled(&self, settings: MonitorSettings) {
        let Some(shared) = &self.shared else {
            return;
        };
        let (state, wake) = &**shared;
        let Ok(mut state) = state.lock() else {
            tracing::warn!("failed to lock automatic hook repair state");
            return;
        };
        if !state.shutting_down {
            state.settings = Some(settings);
            state.repair_requested = true;
            wake.notify_one();
        }
    }

    /// 将显式 Hook 写入排入生命周期 worker，并异步等待有界结果。
    pub async fn write_config(
        &self,
        config_directory: PathBuf,
        tool: AiTool,
    ) -> Result<HookConfigWriteResult, HookError> {
        self.write_config_with_timeout(config_directory, tool, EXPLICIT_HOOK_WRITE_TIMEOUT)
            .await
    }

    /// 使用可注入等待期限排队显式写入，供超时与取消回归测试复用。
    async fn write_config_with_timeout(
        &self,
        config_directory: PathBuf,
        tool: AiTool,
        timeout: Duration,
    ) -> Result<HookConfigWriteResult, HookError> {
        let Some(shared) = &self.shared else {
            return Err(hook_write_lifecycle_error(
                "hook config writer is unavailable",
            ));
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let (response, mut receiver) = oneshot::channel();
        {
            let (state, wake) = &**shared;
            let mut state = state
                .lock()
                .map_err(|_| hook_write_lifecycle_error("hook config writer state was poisoned"))?;
            if state.shutting_down {
                return Err(hook_write_lifecycle_error(
                    "hook config writer is shutting down",
                ));
            }
            state.explicit_requests.push_back(ExplicitHookWriteRequest {
                config_directory,
                tool,
                cancellation: Arc::clone(&cancellation),
                response,
            });
            wake.notify_one();
        }

        let mut cancellation_guard = HookWriteCancellationGuard::new(cancellation);
        match tokio::time::timeout(timeout, &mut receiver).await {
            Ok(Ok(result)) => {
                cancellation_guard.disarm();
                result
            }
            Ok(Err(_)) => Err(hook_write_lifecycle_error(
                "hook config writer stopped before returning a result",
            )),
            Err(_) => {
                cancellation_guard.cancel();
                // 不在 35 秒处制造假失败：必须等 owner 确认再返回，保证调用方收到
                // 结果后不会再有本机 persist 或 WSL mv 落盘。
                let result = receiver.await.map_err(|_| {
                    hook_write_lifecycle_error(
                        "hook config writer stopped while cancelling an overdue write",
                    )
                })?;
                cancellation_guard.disarm();
                result
            }
        }
    }

    /// 停止并有界回收 worker；重复调用保持幂等。
    pub fn shutdown(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.store(true, Ordering::Release);
        }
        if let Some(shared) = &self.shared {
            let (state, wake) = &**shared;
            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            begin_hook_writer_shutdown(&mut state);
            wake.notify_one();
        }
        let worker_handle = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(worker_handle) = worker_handle {
            if let Err(worker_handle) =
                join_worker_until(worker_handle, HOOK_WRITER_SHUTDOWN_TIMEOUT)
            {
                retained_hook_writer_owner().retain(worker_handle);
                tracing::warn!(
                    timeout_ms = HOOK_WRITER_SHUTDOWN_TIMEOUT.as_millis(),
                    "hook config writer exceeded shutdown deadline and was transferred to the retained owner"
                );
            }
        }
        retained_hook_writer_owner().reap_finished();
        #[cfg(target_os = "windows")]
        super::wsl::shutdown_wsl_owners(HOOK_WRITER_SHUTDOWN_TIMEOUT);
    }

    /// 构造没有线程的生命周期占位，保证 Tauri 命令仍可安全取 State。
    pub(crate) fn disabled() -> Self {
        Self {
            shared: None,
            worker: Mutex::new(None),
            cancellation: None,
        }
    }
}

impl Drop for HookConfigWriter {
    /// 即使应用未显式调用 shutdown，也发布取消并把超时线程移交长期 owner。
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 返回跨 Hook writer 实例共享的超时线程 owner。
fn retained_hook_writer_owner() -> &'static RetainedThreadOwner {
    RETAINED_HOOK_WRITERS.get_or_init(|| RetainedThreadOwner::new("hook-writer"))
}

/// 只在具名 worker 已完成时 join；超时返回原句柄，保持真实所有权。
fn join_worker_until(
    worker: thread::JoinHandle<()>,
    timeout: Duration,
) -> Result<(), thread::JoinHandle<()>> {
    let started = Instant::now();
    while !worker.is_finished() {
        if started.elapsed() >= timeout {
            return Err(worker);
        }
        thread::sleep(HOOK_WRITER_SHUTDOWN_POLL.min(timeout.saturating_sub(started.elapsed())));
    }
    let _ = worker.join();
    Ok(())
}

/// 构造不会暴露配置路径或内容的 Hook writer 生命周期错误。
fn hook_write_lifecycle_error(detail: &'static str) -> HookError {
    HookError::new("error.hooks.writeFailed").param("detail", detail)
}

/// 关闭 writer 状态并取消当前及排队显式写入，不把接收端留到 owner 销毁才唤醒。
fn begin_hook_writer_shutdown(state: &mut HookWriterState) {
    state.settings = None;
    state.repair_requested = false;
    state.shutting_down = true;
    if let Some(cancellation) = &state.active_explicit_cancellation {
        cancellation.store(true, Ordering::Release);
    }
    for request in state.explicit_requests.drain(..) {
        request.cancellation.store(true, Ordering::Release);
        let _ = request.response.send(Err(hook_write_lifecycle_error(
            "hook config writer stopped before executing the request",
        )));
    }
}

/// 消费自动补写请求，直到应用生命周期明确关闭。
fn hook_config_writer_loop(
    shared: &Arc<(Mutex<HookWriterState>, Condvar)>,
    relay_executable: &Path,
    home_directory: &Path,
    repair_interval: Duration,
    cancellation: &Arc<AtomicBool>,
) {
    loop {
        let work = {
            let (state, wake) = &**shared;
            let Ok(mut state) = state.lock() else {
                tracing::warn!("automatic hook repair state was poisoned");
                return;
            };
            loop {
                if state.shutting_down {
                    return;
                }
                if let Some(request) = state.explicit_requests.pop_front() {
                    state.active_explicit_cancellation = Some(Arc::clone(&request.cancellation));
                    break HookWriterWork::Explicit(request);
                }
                if state.repair_requested {
                    state.repair_requested = false;
                    if let Some(settings) = state.settings.clone() {
                        break HookWriterWork::AutomaticRepair(settings);
                    }
                }
                let Ok((next, wait)) = wake.wait_timeout(state, repair_interval) else {
                    tracing::warn!("automatic hook repair wait state was poisoned");
                    return;
                };
                state = next;
                if wait.timed_out() && state.settings.is_some() {
                    state.repair_requested = true;
                }
            }
        };
        match work {
            HookWriterWork::Explicit(request) => {
                let request_cancellation = Arc::clone(&request.cancellation);
                let result =
                    execute_explicit_hook_write(&request, relay_executable, home_directory);
                let _ = request.response.send(result);
                let (state, _) = &**shared;
                let Ok(mut state) = state.lock() else {
                    tracing::warn!("automatic hook repair state was poisoned");
                    return;
                };
                if state
                    .active_explicit_cancellation
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &request_cancellation))
                {
                    state.active_explicit_cancellation = None;
                }
            }
            HookWriterWork::AutomaticRepair(settings) => {
                repair_enabled_hook_configs_with_cancellation(
                    &settings,
                    relay_executable,
                    home_directory,
                    Arc::clone(cancellation),
                )
            }
        }
    }
}

/// 在具名 worker 中读取设置并完成显式写入，不占用 Tauri 异步执行线程。
fn execute_explicit_hook_write(
    request: &ExplicitHookWriteRequest,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(Some(&request.cancellation))?;
    let settings = super::settings::load_monitor_settings(&request.config_directory)?;
    write_hook_config_with_cancellation(
        &settings,
        request.tool,
        relay_executable,
        home_directory,
        Some(Arc::clone(&request.cancellation)),
    )
}

/// 读取环境变量覆盖的绝对配置目录。
fn detected_config_directory(variable: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or(fallback)
}

/// Hermes 在 Windows 上遵循 `%LOCALAPPDATA%\hermes`。
#[cfg(target_os = "windows")]
fn default_hermes_home(home: &Path) -> PathBuf {
    detected_config_directory("LOCALAPPDATA", home.to_owned()).join("hermes")
}

/// Hermes 在 POSIX 系统上使用 `~/.hermes`。
#[cfg(not(target_os = "windows"))]
fn default_hermes_home(home: &Path) -> PathBuf {
    home.join(".hermes")
}

/// 全部受支持 AI 工具的公开默认配置根目录。
fn default_directory(tool: AiTool, home: &Path) -> PathBuf {
    let open_code_fallback =
        detected_config_directory("XDG_CONFIG_HOME", home.join(".config")).join("opencode");
    match tool {
        AiTool::Codex => detected_config_directory("CODEX_HOME", home.join(".codex")),
        AiTool::ClaudeCode => detected_config_directory("CLAUDE_CONFIG_DIR", home.join(".claude")),
        AiTool::Cursor => home.join(".cursor"),
        AiTool::OpenCode => detected_config_directory("OPENCODE_CONFIG_DIR", open_code_fallback),
        AiTool::WorkBuddy => home.join(".workbuddy"),
        AiTool::Hermes => detected_config_directory("HERMES_HOME", default_hermes_home(home)),
        AiTool::OpenClaw => detected_config_directory("OPENCLAW_STATE_DIR", home.join(".openclaw")),
        AiTool::CodeBuddy => {
            detected_config_directory("CODEBUDDY_CONFIG_DIR", home.join(".codebuddy"))
        }
        AiTool::QwenCode => home.join(".qwen"),
        AiTool::KimiCode => detected_config_directory("KIMI_CODE_HOME", home.join(".kimi-code")),
        AiTool::Qoder => home.join(".qoder"),
        AiTool::GeminiCli => home.join(".gemini"),
        AiTool::GitHubCopilot => detected_config_directory("COPILOT_HOME", home.join(".copilot")),
        AiTool::Grok => detected_config_directory("GROK_HOME", home.join(".grok")),
    }
}

/// 解析某工具最终配置定位。
fn location_for(
    tool: AiTool,
    directories: &HookConfigDirectories,
    home_directory: &Path,
) -> HookConfigLocation {
    let custom = directories.get(tool).trim();
    let (directory, is_custom) = if custom.is_empty() {
        (default_directory(tool, home_directory), false)
    } else {
        (PathBuf::from(custom), true)
    };
    let config_path = directory.join(hook_config_filename(tool));
    HookConfigLocation {
        tool,
        directory: directory.to_string_lossy().into_owned(),
        config_path: config_path.to_string_lossy().into_owned(),
        is_custom,
    }
}

/// 按统一公开目录列出当前可配置 Agent 的 Hook 定位；隐藏协议仍保留内部实现。
pub fn list_hook_config_locations(
    settings: &MonitorSettings,
    home_directory: &Path,
) -> Vec<HookConfigLocation> {
    public_monitor_ai_tools()
        .map(|tool| location_for(tool, &settings.hook_directories, home_directory))
        .collect()
}

/// 校验并规范化自定义 Hook 目录；空字符串表示恢复默认目录。
pub fn validate_hook_config_directory(directory: &str) -> Result<String, HookError> {
    let directory = directory.trim();
    if directory.is_empty() {
        return Ok(String::new());
    }
    let path = Path::new(directory);
    let is_wsl_unc = cfg!(target_os = "windows") && WslDirectory::parse(directory).is_some();
    if !path.is_absolute() && !is_wsl_unc {
        return Err(HookError::new("error.hooks.directoryNotAbsolute"));
    }
    if !is_wsl_unc && path.exists() && !path.is_dir() {
        return Err(HookError::new("error.hooks.directoryNotAFolder")
            .param("path", path.to_string_lossy().into_owned()));
    }
    Ok(directory.to_owned())
}

/// 为指定工具生成、完整校验并以原子替换写入本机或 WSL 配置。
#[cfg(test)]
fn write_hook_config(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    write_hook_config_with_cancellation(settings, tool, relay_executable, home_directory, None)
}

/// 串行化一次显式或后台写入；后台路径额外携带应用退出取消令牌。
fn write_hook_config_with_cancellation(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;
    let _guard = lock_hook_config_writes(cancellation.as_deref())?;
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;
    write_hook_config_unlocked(
        settings,
        tool,
        relay_executable,
        home_directory,
        cancellation,
    )
}

/// 以可取消短轮询取得进程内写锁，避免排队请求在 35 秒后仍无法确认取消。
fn lock_hook_config_writes(
    cancellation: Option<&AtomicBool>,
) -> Result<std::sync::MutexGuard<'static, ()>, HookError> {
    let lock = HOOK_CONFIG_WRITE_LOCK.get_or_init(|| Mutex::new(()));
    loop {
        ensure_hook_write_not_cancelled(cancellation)?;
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => thread::sleep(HOOK_WRITER_SHUTDOWN_POLL),
            Err(TryLockError::Poisoned(_)) => {
                return Err(HookError::new("error.hooks.writeFailed")
                    .param("detail", "hook config write lock was poisoned"));
            }
        }
    }
}

/// 在阻塞阶段边界观察调用或应用退出取消，未开始的写入必须失败关闭。
fn ensure_hook_write_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    if cancellation.is_some_and(|cancellation| cancellation.load(Ordering::Acquire)) {
        return Err(hook_write_lifecycle_error(
            "hook config write was cancelled before completion",
        ));
    }
    Ok(())
}

/// 已持有配置写锁时执行单个工具的完整多文件写入。
fn write_hook_config_unlocked(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<HookConfigWriteResult, HookError> {
    let custom_directory = settings.hook_directories.get(tool);
    if !custom_directory.trim().is_empty() {
        validate_hook_config_directory(custom_directory)?;
    }
    let location = location_for(tool, &settings.hook_directories, home_directory);
    let config_path = PathBuf::from(&location.config_path);
    let wsl_directory = WslDirectory::parse(&location.directory)
        .map(|directory| directory.with_cancellation(cancellation.clone()));
    if wsl_directory.is_some() && !hook_supports_wsl(tool) {
        return Err(HookError::new("error.hooks.wslUnsupportedByWindowsHost")
            .param("tool", ai_tool_name(tool)));
    }
    let generated = if let Some(wsl_directory) = &wsl_directory {
        let wsl_executable = wsl_directory.translate_windows_executable(relay_executable)?;
        generate_wsl_hook_config(tool, relay_executable, &wsl_executable)?
    } else {
        generate_hook_config(tool, relay_executable)?
    };
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;

    if let Some(wsl_directory) = wsl_directory {
        return write_wsl_configs(
            tool,
            &config_path,
            &wsl_directory,
            generated,
            cancellation.as_deref(),
        );
    }
    write_local_configs(
        tool,
        &config_path,
        Path::new(&location.directory),
        generated,
        cancellation.as_deref(),
    )
}

/// 合并并写入普通本机配置及工具声明的全部辅助文件。
fn write_local_configs(
    tool: AiTool,
    config_path: &Path,
    directory: &Path,
    generated: HookConfigPreview,
    cancellation: Option<&AtomicBool>,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation)?;
    let mut generated_files = vec![(config_path.to_owned(), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let mut config_changed = super::hook_config_io::reconcile_local_config_set_with_cancellation(
        directory,
        tool,
        generated_files,
        cancellation,
    )?;
    ensure_hook_write_not_cancelled(cancellation)?;
    if tool == AiTool::Grok {
        config_changed |=
            super::hook_config_migration::migrate_legacy_grok_local_with_cancellation(
                directory,
                cancellation,
            )?;
    }
    Ok(hook_config_write_result(
        tool,
        config_path.to_string_lossy().into_owned(),
        config_changed,
    ))
}

/// 合并并写入 WSL 主配置及工具声明的全部辅助文件。
fn write_wsl_configs(
    tool: AiTool,
    config_path: &Path,
    directory: &WslDirectory,
    generated: HookConfigPreview,
    cancellation: Option<&AtomicBool>,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation)?;
    let mut generated_files = vec![(directory.join(hook_config_filename(tool)), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let mut config_changed =
        super::hook_config_io::reconcile_wsl_config_set(tool, generated_files)?;
    ensure_hook_write_not_cancelled(cancellation)?;
    if tool == AiTool::Grok {
        config_changed |= super::hook_config_migration::migrate_legacy_grok_wsl(directory)?;
    }
    Ok(hook_config_write_result(
        tool,
        config_path.to_string_lossy().into_owned(),
        config_changed,
    ))
}

/// 对所有已启用工具运行完整合并，因此管理标识存在时也会修复旧路径和缺失事件。
#[cfg(test)]
fn repair_enabled_hook_configs(
    settings: &MonitorSettings,
    relay_executable: &Path,
    home_directory: &Path,
) {
    repair_enabled_hook_configs_with_cancellation(
        settings,
        relay_executable,
        home_directory,
        Arc::new(AtomicBool::new(false)),
    );
}

/// 后台自愈在每个工具边界观察应用退出，并把同一令牌传入 WSL 子进程。
fn repair_enabled_hook_configs_with_cancellation(
    settings: &MonitorSettings,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Arc<AtomicBool>,
) {
    for tool in normalize_enabled_ai_tools(&settings.enabled_ai_tools) {
        if cancellation.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = write_hook_config_with_cancellation(
            settings,
            tool,
            relay_executable,
            home_directory,
            Some(Arc::clone(&cancellation)),
        ) {
            tracing::warn!(
                tool = ai_tool_name(tool),
                code = error.code,
                "failed to repair enabled hook config"
            );
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
