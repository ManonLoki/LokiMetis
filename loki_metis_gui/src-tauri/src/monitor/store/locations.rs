//! Hook 配置目录定位：默认根、自定义目录、多固定根与目录校验。
//!
//! 每个受管工具都有确定的配置根；多数工具只有一个固定根，WorkBuddy 国内版
//! `~/.workbuddy` 与国际版 `~/.workbuddy-ai` 是两个互不相同的固定安装目录，共同
//! 构成同一个受管工具，因此默认写入必须同时维护两份配置。用户显式指定自定义目录
//! 后只使用该位置，不再附带任何固定根。
//!
//! 本模块只做纯路径推导与校验，不触碰文件系统内容，也不执行写入。

use std::path::{Path, PathBuf};

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookConfigLocation, HookError, hook_config_filename,
    public_monitor_ai_tools, workbuddy_homes_from_user_home,
};

use super::super::{settings::MonitorSettings, wsl::WslDirectory};

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
///
/// 多数工具只有一个固定配置根；WorkBuddy 国内版与国际版是两个互不相同的固定安装
/// 目录，两者共同构成同一个受管工具，因此返回两个根且保持国内版在前。
fn default_directories(tool: AiTool, home: &Path) -> Vec<PathBuf> {
    let open_code_fallback =
        detected_config_directory("XDG_CONFIG_HOME", home.join(".config")).join("opencode");
    match tool {
        AiTool::Codex => vec![detected_config_directory("CODEX_HOME", home.join(".codex"))],
        AiTool::ClaudeCode => vec![detected_config_directory(
            "CLAUDE_CONFIG_DIR",
            home.join(".claude"),
        )],
        AiTool::Cursor => vec![home.join(".cursor")],
        AiTool::OpenCode => vec![detected_config_directory(
            "OPENCODE_CONFIG_DIR",
            open_code_fallback,
        )],
        AiTool::WorkBuddy => workbuddy_homes_from_user_home(home).to_vec(),
        AiTool::Hermes => vec![detected_config_directory(
            "HERMES_HOME",
            default_hermes_home(home),
        )],
        AiTool::OpenClaw => vec![detected_config_directory(
            "OPENCLAW_STATE_DIR",
            home.join(".openclaw"),
        )],
        AiTool::CodeBuddy => vec![detected_config_directory(
            "CODEBUDDY_CONFIG_DIR",
            home.join(".codebuddy"),
        )],
        AiTool::QwenCode => vec![home.join(".qwen")],
        AiTool::KimiCode => vec![detected_config_directory(
            "KIMI_CODE_HOME",
            home.join(".kimi-code"),
        )],
        AiTool::Qoder => vec![home.join(".qoder")],
        AiTool::GeminiCli => vec![home.join(".gemini")],
        AiTool::GitHubCopilot => vec![detected_config_directory(
            "COPILOT_HOME",
            home.join(".copilot"),
        )],
        AiTool::Grok => vec![detected_config_directory("GROK_HOME", home.join(".grok"))],
    }
}

/// 解析某工具本次必须写入的全部配置目录；自定义目录唯一，默认目录可能有多个。
pub(super) fn hook_config_target_directories(
    tool: AiTool,
    directories: &HookConfigDirectories,
    home_directory: &Path,
) -> Vec<PathBuf> {
    let custom = directories.get(tool).trim();
    if !custom.is_empty() {
        return vec![PathBuf::from(custom)];
    }
    default_directories(tool, home_directory)
}

/// 派生某工具在指定目录下的配置文件完整路径。
pub(super) fn hook_config_path_in(tool: AiTool, directory: &Path) -> String {
    directory
        .join(hook_config_filename(tool))
        .to_string_lossy()
        .into_owned()
}

/// 组装一条 Hook 配置定位；配置文件路径与附加路径都由目录派生。
fn hook_config_location(
    tool: AiTool,
    directory: PathBuf,
    is_custom: bool,
    additional_config_paths: Vec<String>,
) -> HookConfigLocation {
    let config_path = hook_config_path_in(tool, &directory);
    HookConfigLocation {
        tool,
        directory: directory.to_string_lossy().into_owned(),
        config_path,
        is_custom,
        additional_config_paths,
    }
}

/// 解析某工具最终配置定位；自定义目录优先，否则使用全部固定默认目录。
///
/// 自定义目录表示用户明确指定了唯一位置，因此不再附带其它固定根；默认情况下
/// 第一个固定根是主配置，其余固定根的配置文件路径作为附加路径一并返回。
pub(super) fn location_for(
    tool: AiTool,
    directories: &HookConfigDirectories,
    home_directory: &Path,
) -> HookConfigLocation {
    let custom = directories.get(tool).trim();
    let mut targets = hook_config_target_directories(tool, directories, home_directory).into_iter();
    let primary = targets
        .next()
        .unwrap_or_else(|| home_directory.to_path_buf());
    let additional_config_paths = targets
        .map(|directory| hook_config_path_in(tool, &directory))
        .collect();
    hook_config_location(tool, primary, !custom.is_empty(), additional_config_paths)
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

#[cfg(test)]
#[path = "locations_tests.rs"]
mod tests;
