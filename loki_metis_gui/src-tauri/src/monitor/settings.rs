//! 监控区本机设置：已启用 Agent 与 Hook 目录覆盖。

use std::path::{Path, PathBuf};

use loki_metis_core::{AiTool, HookConfigDirectories, HookError, normalize_enabled_ai_tools};
use serde::{Deserialize, Serialize};

/// 监控区持久设置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 已启用的四项 Agent。
    pub enabled_ai_tools: Vec<AiTool>,
    /// 自定义 Hook 配置目录。
    pub hook_directories: HookConfigDirectories,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            enabled_ai_tools: AiTool::ALL.to_vec(),
            hook_directories: HookConfigDirectories::default(),
        }
    }
}

/// 设置文件路径。
pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("monitor-settings.json")
}

/// 读取监控设置；缺失时返回默认四项全开。
pub fn load_monitor_settings(config_dir: &Path) -> Result<MonitorSettings, HookError> {
    let path = settings_path(config_dir);
    if !path.exists() {
        return Ok(MonitorSettings::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string()))?;
    let mut settings: MonitorSettings = serde_json::from_str(&raw)
        .map_err(|error| HookError::new("error.monitor.settingsInvalid").param("detail", error.to_string()))?;
    settings.enabled_ai_tools = normalize_enabled_ai_tools(&settings.enabled_ai_tools);
    if settings.enabled_ai_tools.is_empty() {
        settings.enabled_ai_tools = AiTool::ALL.to_vec();
    }
    Ok(settings)
}

/// 保存监控设置。
pub fn save_monitor_settings(config_dir: &Path, settings: &MonitorSettings) -> Result<(), HookError> {
    std::fs::create_dir_all(config_dir)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    let normalized = MonitorSettings {
        enabled_ai_tools: normalize_enabled_ai_tools(&settings.enabled_ai_tools),
        hook_directories: settings.hook_directories.clone(),
    };
    let raw = serde_json::to_string_pretty(&normalized)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    std::fs::write(settings_path(config_dir), raw)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))
}
