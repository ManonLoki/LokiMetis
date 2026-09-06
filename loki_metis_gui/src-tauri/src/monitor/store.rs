//! 本机 Hook 配置目录、受管多文件原子写入与生命周期补写 worker。

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
    thread,
};

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookConfigLocation, HookConfigPreview, HookConfigWriteResult,
    HookError, ai_tool_name, generate_hook_auxiliary_configs, generate_hook_config,
    generate_wsl_hook_config, hook_config_filename, hook_config_write_result, hook_supports_wsl,
    normalize_enabled_ai_tools, public_monitor_ai_tools,
};

use super::{settings::MonitorSettings, wsl::WslDirectory};

/// 串行化显式写入与后台自动补写，避免同一工具配置被并发读改写覆盖。
static HOOK_CONFIG_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
/// 外部 Hook writer 停止后，后台自愈在此期限内重新收敛受管条目。
const AUTOMATIC_REPAIR_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// 自动补写 worker 的单槽合并状态；新设置覆盖尚未处理的旧快照。
#[derive(Default)]
struct HookWriterState {
    /// 最新设置快照；保留后供低频内容感知自愈重复检查。
    settings: Option<MonitorSettings>,
    /// 保存或启动请求要求立即检查，而不是等待下一个周期。
    repair_requested: bool,
    /// 应用退出后阻止接受和执行更多写入。
    shutting_down: bool,
}

/// 由 Tauri 应用生命周期拥有的 Hook 自动补写 worker。
pub struct HookConfigWriter {
    /// worker 与调用方共享的单槽状态和唤醒信号。
    shared: Option<Arc<(Mutex<HookWriterState>, Condvar)>>,
    /// 应用退出时必须回收的具名线程。
    worker: Mutex<Option<thread::JoinHandle<()>>>,
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
        repair_interval: std::time::Duration,
    ) -> Self {
        let shared = Arc::new((Mutex::new(HookWriterState::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let worker = match thread::Builder::new()
            .name("loki-metis-auto-hooks".to_owned())
            .spawn(move || {
                hook_config_writer_loop(
                    &worker_shared,
                    &relay_executable,
                    &home_directory,
                    repair_interval,
                )
            }) {
            Ok(worker) => worker,
            Err(error) => {
                tracing::warn!(error = %error, "failed to start automatic hook writer");
                return Self::disabled();
            }
        };
        Self {
            shared: Some(shared),
            worker: Mutex::new(Some(worker)),
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

    /// 停止并回收 worker；重复调用保持幂等。
    pub fn shutdown(&self) {
        let Ok(mut worker) = self.worker.lock() else {
            return;
        };
        let Some(worker) = worker.take() else {
            return;
        };
        if let Some(shared) = &self.shared {
            let (state, wake) = &**shared;
            if let Ok(mut state) = state.lock() {
                state.settings = None;
                state.repair_requested = false;
                state.shutting_down = true;
                wake.notify_one();
            }
        }
        let _ = worker.join();
    }

    /// 构造没有线程的生命周期占位，保证 Tauri 命令仍可安全取 State。
    pub(crate) fn disabled() -> Self {
        Self {
            shared: None,
            worker: Mutex::new(None),
        }
    }
}

/// 消费自动补写请求，直到应用生命周期明确关闭。
fn hook_config_writer_loop(
    shared: &Arc<(Mutex<HookWriterState>, Condvar)>,
    relay_executable: &Path,
    home_directory: &Path,
    repair_interval: std::time::Duration,
) {
    loop {
        let settings = {
            let (state, wake) = &**shared;
            let Ok(mut state) = state.lock() else {
                tracing::warn!("automatic hook repair state was poisoned");
                return;
            };
            loop {
                if state.shutting_down {
                    return;
                }
                if state.repair_requested {
                    state.repair_requested = false;
                    break state.settings.clone();
                }
                let Ok((next, wait)) = wake.wait_timeout(state, repair_interval) else {
                    tracing::warn!("automatic hook repair wait state was poisoned");
                    return;
                };
                state = next;
                if wait.timed_out() && state.settings.is_some() {
                    state.repair_requested = false;
                    break state.settings.clone();
                }
            }
        };
        if let Some(settings) = settings {
            repair_enabled_hook_configs(&settings, relay_executable, home_directory);
        }
    }
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
pub fn write_hook_config(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    let _guard = HOOK_CONFIG_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            HookError::new("error.hooks.writeFailed")
                .param("detail", "hook config write lock was poisoned")
        })?;
    write_hook_config_unlocked(settings, tool, relay_executable, home_directory)
}

/// 已持有配置写锁时执行单个工具的完整多文件写入。
fn write_hook_config_unlocked(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    let custom_directory = settings.hook_directories.get(tool);
    if !custom_directory.trim().is_empty() {
        validate_hook_config_directory(custom_directory)?;
    }
    let location = location_for(tool, &settings.hook_directories, home_directory);
    let config_path = PathBuf::from(&location.config_path);
    let wsl_directory = WslDirectory::parse(&location.directory);
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

    if let Some(wsl_directory) = wsl_directory {
        return write_wsl_configs(tool, &config_path, &wsl_directory, generated);
    }
    write_local_configs(
        tool,
        &config_path,
        Path::new(&location.directory),
        generated,
    )
}

/// 合并并写入普通本机配置及工具声明的全部辅助文件。
fn write_local_configs(
    tool: AiTool,
    config_path: &Path,
    directory: &Path,
    generated: HookConfigPreview,
) -> Result<HookConfigWriteResult, HookError> {
    let mut generated_files = vec![(config_path.to_owned(), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let mut config_changed =
        super::hook_config_io::reconcile_local_config_set(directory, tool, generated_files)?;
    if tool == AiTool::Grok {
        config_changed |= super::hook_config_migration::migrate_legacy_grok_local(directory)?;
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
) -> Result<HookConfigWriteResult, HookError> {
    let mut generated_files = vec![(directory.join(hook_config_filename(tool)), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let mut config_changed =
        super::hook_config_io::reconcile_wsl_config_set(tool, generated_files)?;
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
fn repair_enabled_hook_configs(
    settings: &MonitorSettings,
    relay_executable: &Path,
    home_directory: &Path,
) {
    for tool in normalize_enabled_ai_tools(&settings.enabled_ai_tools) {
        if let Err(error) = write_hook_config(settings, tool, relay_executable, home_directory) {
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
