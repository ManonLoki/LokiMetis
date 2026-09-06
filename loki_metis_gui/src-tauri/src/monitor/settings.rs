//! 监控区本机设置：已启用 Agent 与 Hook 目录覆盖。

use std::path::{Path, PathBuf};

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookError, PetOverlayPosition, normalize_enabled_ai_tools,
};
use serde::{Deserialize, Serialize};

/// 监控区持久设置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 已启用的四项 Agent。
    pub enabled_ai_tools: Vec<AiTool>,
    /// 自定义 Hook 配置目录。
    pub hook_directories: HookConfigDirectories,
    /// 最近一次合法的桌宠浮窗位置；缺失表示使用默认几何。
    #[serde(default)]
    pub pet_overlay_position: Option<PetOverlayPosition>,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            enabled_ai_tools: AiTool::ALL.to_vec(),
            hook_directories: HookConfigDirectories::default(),
            pet_overlay_position: None,
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

/// 保存监控设置并返回落盘后的规范化结果，调用方无需再读一次。
pub fn save_monitor_settings(
    config_dir: &Path,
    settings: &MonitorSettings,
) -> Result<MonitorSettings, HookError> {
    std::fs::create_dir_all(config_dir)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    let normalized = MonitorSettings {
        enabled_ai_tools: normalize_enabled_ai_tools(&settings.enabled_ai_tools),
        hook_directories: settings.hook_directories.clone(),
        pet_overlay_position: settings.pet_overlay_position,
    };
    let raw = serde_json::to_string_pretty(&normalized)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    std::fs::write(settings_path(config_dir), raw)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    Ok(normalized)
}

/// 读改写单个偏好字段并返回规范化结果；偏好命令共用，避免各自重复 load→save→load。
pub fn update_monitor_settings(
    config_dir: &Path,
    mutate: impl FnOnce(&mut MonitorSettings),
) -> Result<MonitorSettings, HookError> {
    let mut settings = load_monitor_settings(config_dir)?;
    mutate(&mut settings);
    save_monitor_settings(config_dir, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn legacy_pet_close_control_setting_is_ignored_and_removed() {
        let root = tempdir().expect("temp");
        let config = root.path();
        std::fs::write(
            settings_path(config),
            r#"{"enabledAiTools":["codex"],"hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""},"petCloseControlVisible":false}"#,
        )
        .expect("write");
        let settings = load_monitor_settings(config).expect("load");
        assert_eq!(settings.enabled_ai_tools, vec![AiTool::Codex]);
        save_monitor_settings(config, &settings).expect("save");
        let saved = std::fs::read_to_string(settings_path(config)).expect("read");
        assert!(!saved.contains("petCloseControlVisible"));
    }

    #[test]
    fn missing_pet_overlay_position_stays_unset() {
        let root = tempdir().expect("temp");
        let config = root.path();
        std::fs::write(
            settings_path(config),
            r#"{"enabledAiTools":["codex"],"hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""}}"#,
        )
        .expect("write");
        let settings = load_monitor_settings(config).expect("load");
        assert_eq!(settings.pet_overlay_position, None);
    }

    #[test]
    fn saved_pet_overlay_position_round_trips() {
        let root = tempdir().expect("temp");
        let config = root.path();
        let mut settings = MonitorSettings::default();
        settings.pet_overlay_position = Some(PetOverlayPosition { x: 240, y: 90 });
        save_monitor_settings(config, &settings).expect("save");
        let loaded = load_monitor_settings(config).expect("load");
        assert_eq!(
            loaded.pet_overlay_position,
            Some(PetOverlayPosition { x: 240, y: 90 })
        );
    }
}
