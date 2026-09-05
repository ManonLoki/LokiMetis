//! 监控区本机设置：已启用 Agent 与 Hook 目录覆盖。

use std::path::{Path, PathBuf};

use loki_metis_core::{
    AiTool, DEFAULT_PET_CLOSE_CONTROL_VISIBLE, HookConfigDirectories, HookError, PetOverlayPosition,
    normalize_enabled_ai_tools, normalize_pet_close_control_visible,
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
    /// 是否显示桌宠悬浮窗圆形关闭控件（兔耳）。
    #[serde(default = "default_pet_close_control_visible")]
    pub pet_close_control_visible: bool,
    /// 最近一次合法的桌宠浮窗位置；缺失表示使用默认几何。
    #[serde(default)]
    pub pet_overlay_position: Option<PetOverlayPosition>,
}

/// serde 缺字段时回落到 core 默认显示兔耳。
fn default_pet_close_control_visible() -> bool {
    DEFAULT_PET_CLOSE_CONTROL_VISIBLE
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            enabled_ai_tools: AiTool::ALL.to_vec(),
            hook_directories: HookConfigDirectories::default(),
            pet_close_control_visible: DEFAULT_PET_CLOSE_CONTROL_VISIBLE,
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
    settings.pet_close_control_visible =
        normalize_pet_close_control_visible(Some(settings.pet_close_control_visible));
    Ok(settings)
}

/// 保存监控设置。
pub fn save_monitor_settings(config_dir: &Path, settings: &MonitorSettings) -> Result<(), HookError> {
    std::fs::create_dir_all(config_dir)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    let normalized = MonitorSettings {
        enabled_ai_tools: normalize_enabled_ai_tools(&settings.enabled_ai_tools),
        hook_directories: settings.hook_directories.clone(),
        pet_close_control_visible: normalize_pet_close_control_visible(Some(
            settings.pet_close_control_visible,
        )),
        pet_overlay_position: settings.pet_overlay_position,
    };
    let raw = serde_json::to_string_pretty(&normalized)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))?;
    std::fs::write(settings_path(config_dir), raw)
        .map_err(|error| HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_pet_close_control_visible_defaults_to_shown() {
        let root = tempdir().expect("temp");
        let config = root.path();
        std::fs::write(
            settings_path(config),
            r#"{"enabledAiTools":["codex"],"hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""}}"#,
        )
        .expect("write");
        let settings = load_monitor_settings(config).expect("load");
        assert!(settings.pet_close_control_visible);
        assert_eq!(settings.enabled_ai_tools, vec![AiTool::Codex]);
    }

    #[test]
    fn saved_pet_close_control_hidden_round_trips() {
        let root = tempdir().expect("temp");
        let config = root.path();
        let mut settings = MonitorSettings::default();
        settings.pet_close_control_visible = false;
        save_monitor_settings(config, &settings).expect("save");
        let loaded = load_monitor_settings(config).expect("load");
        assert!(!loaded.pet_close_control_visible);
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
