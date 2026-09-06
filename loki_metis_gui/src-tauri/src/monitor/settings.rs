//! 监控区本机设置：已启用 Agent 与 Hook 目录覆盖。

use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookError, PetLayout, PetOverlayPosition,
    normalize_enabled_ai_tools,
};
use serde::{Deserialize, Serialize};

/// 监控设置文件的进程内读写锁；所有配置目录共用以保证简单且确定的串行化。
static MONITOR_SETTINGS_LOCK: OnceLock<RwLock<()>> = OnceLock::new();

/// 返回监控设置文件的全局读写锁。
fn monitor_settings_lock() -> &'static RwLock<()> {
    MONITOR_SETTINGS_LOCK.get_or_init(|| RwLock::new(()))
}

/// 桌宠窗口的持久偏好；可见性只属于当前会话，不在这里落盘。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetWindowPreferences {
    /// 当前桌宠布局。
    #[serde(default, deserialize_with = "deserialize_pet_layout")]
    pub layout: PetLayout,
    /// 当前页首个位置的零基索引。
    #[serde(default, deserialize_with = "deserialize_focused_slot")]
    pub focused_slot: u8,
    /// 单格逻辑像素边长。
    #[serde(
        default = "default_pet_size",
        deserialize_with = "deserialize_pet_size"
    )]
    pub pet_size: u16,
    /// 是否始终置顶。
    #[serde(
        default = "default_true",
        deserialize_with = "deserialize_default_true"
    )]
    pub always_on_top: bool,
    /// 是否锁定拖拽和缩放。
    #[serde(default, deserialize_with = "deserialize_default_false")]
    pub locked: bool,
}

impl Default for PetWindowPreferences {
    fn default() -> Self {
        Self {
            layout: PetLayout::Grid,
            focused_slot: 0,
            pet_size: default_pet_size(),
            always_on_top: true,
            locked: false,
        }
    }
}

/// 默认桌宠单格边长，与 AIMonitorDesktop 的 64 逻辑像素基线一致。
const fn default_pet_size() -> u16 {
    64
}

/// serde 使用的 true 产品默认值。
const fn default_true() -> bool {
    true
}

/// 把单个畸形 JSON 字段恢复为产品默认，而不连坐整个设置文件。
fn deserialize_or_default<'de, D, T>(
    deserializer: D,
    fallback: impl FnOnce() -> T,
) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_else(|_| fallback()))
}

/// 解析布局，畸形值恢复为默认 2×2。
fn deserialize_pet_layout<'de, D>(deserializer: D) -> Result<PetLayout, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, PetLayout::default)
}

/// 解析页首位置，畸形值恢复为第一页。
fn deserialize_focused_slot<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, || 0)
}

/// 解析单格大小，畸形值恢复为 64。
fn deserialize_pet_size<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, default_pet_size)
}

/// 解析默认为 true 的布尔偏好。
fn deserialize_default_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, default_true)
}

/// 解析默认为 false 的布尔偏好。
fn deserialize_default_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, || false)
}

/// 解析 petWindow 对象本身，畸形对象只回退桌宠子设置。
fn deserialize_pet_window_preferences<'de, D>(
    deserializer: D,
) -> Result<PetWindowPreferences, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, PetWindowPreferences::default)
}

/// 规范化落盘偏好，旧文件或手工编辑的越界值不会进入运行时。
fn normalize_pet_window_preferences(mut preferences: PetWindowPreferences) -> PetWindowPreferences {
    preferences.focused_slot = preferences.focused_slot.min(11);
    preferences.pet_size = preferences.pet_size.clamp(32, 2_048);
    preferences
}

/// 监控区持久设置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 已启用的 Agent；显式空集合表示暂不自动补写任何工具。
    #[serde(default = "default_enabled_ai_tools")]
    pub enabled_ai_tools: Vec<AiTool>,
    /// 自定义 Hook 配置目录。
    #[serde(default)]
    pub hook_directories: HookConfigDirectories,
    /// 最近一次合法的桌宠浮窗位置；缺失表示使用默认几何。
    #[serde(default)]
    pub pet_overlay_position: Option<PetOverlayPosition>,
    /// 桌宠布局、分页、大小、置顶与锁定偏好。
    #[serde(default, deserialize_with = "deserialize_pet_window_preferences")]
    pub pet_window: PetWindowPreferences,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            enabled_ai_tools: default_enabled_ai_tools(),
            hook_directories: HookConfigDirectories::default(),
            pet_overlay_position: None,
            pet_window: PetWindowPreferences::default(),
        }
    }
}

/// 首次启动或旧设置缺失字段时，与源程序一致默认启用 Codex、Claude Code、Cursor。
fn default_enabled_ai_tools() -> Vec<AiTool> {
    vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
}

/// 设置文件路径。
pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("monitor-settings.json")
}

/// 读取监控设置；缺失时返回默认集合，显式空集合保持为空。
pub fn load_monitor_settings(config_dir: &Path) -> Result<MonitorSettings, HookError> {
    let _guard = monitor_settings_lock().read().map_err(|_| {
        HookError::new("error.monitor.settingsReadFailed")
            .param("detail", "monitor settings lock poisoned")
    })?;
    load_monitor_settings_unlocked(config_dir)
}

/// 桌宠运行时读取失败时使用安全默认偏好，保证损坏设置不会阻断默认显示与托盘恢复。
/// 主界面继续调用 [`load_monitor_settings`]，因此仍会明确暴露原始设置错误。
pub fn load_pet_runtime_settings(config_dir: &Path) -> MonitorSettings {
    load_monitor_settings(config_dir).unwrap_or_else(|error| {
        tracing::warn!(
            code = error.code,
            "invalid monitor settings; using pet runtime defaults"
        );
        MonitorSettings::default()
    })
}

/// 在调用方已持有读锁或写锁时读取并规范化监控设置。
fn load_monitor_settings_unlocked(config_dir: &Path) -> Result<MonitorSettings, HookError> {
    let path = settings_path(config_dir);
    if !path.exists() {
        return Ok(MonitorSettings::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|error| {
        HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string())
    })?;
    let mut settings: MonitorSettings = serde_json::from_str(&raw).map_err(|error| {
        HookError::new("error.monitor.settingsInvalid").param("detail", error.to_string())
    })?;
    settings.enabled_ai_tools = normalize_enabled_ai_tools(&settings.enabled_ai_tools);
    settings.pet_window = normalize_pet_window_preferences(settings.pet_window);
    Ok(settings)
}

/// 保存监控设置并返回落盘后的规范化结果，调用方无需再读一次。
pub fn save_monitor_settings(
    config_dir: &Path,
    settings: &MonitorSettings,
) -> Result<MonitorSettings, HookError> {
    let _guard = monitor_settings_lock().write().map_err(|_| {
        HookError::new("error.monitor.settingsWriteFailed")
            .param("detail", "monitor settings lock poisoned")
    })?;
    save_monitor_settings_unlocked(config_dir, settings)
}

/// 在调用方已持有写锁时规范化并原子替换监控设置文件。
fn save_monitor_settings_unlocked(
    config_dir: &Path,
    settings: &MonitorSettings,
) -> Result<MonitorSettings, HookError> {
    std::fs::create_dir_all(config_dir).map_err(|error| {
        HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string())
    })?;
    let normalized = MonitorSettings {
        enabled_ai_tools: normalize_enabled_ai_tools(&settings.enabled_ai_tools),
        hook_directories: settings.hook_directories.clone(),
        pet_overlay_position: settings.pet_overlay_position,
        pet_window: normalize_pet_window_preferences(settings.pet_window),
    };
    let raw = serde_json::to_string_pretty(&normalized).map_err(|error| {
        HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string())
    })?;
    super::atomic_file::write_monitor_file_atomically(
        &settings_path(config_dir),
        raw.as_bytes(),
        "error.monitor.settingsWriteFailed",
    )?;
    Ok(normalized)
}

/// 读改写单个偏好字段并返回规范化结果；偏好命令共用，避免各自重复 load→save→load。
pub fn update_monitor_settings(
    config_dir: &Path,
    mutate: impl FnOnce(&mut MonitorSettings),
) -> Result<MonitorSettings, HookError> {
    let _guard = monitor_settings_lock().write().map_err(|_| {
        HookError::new("error.monitor.settingsWriteFailed")
            .param("detail", "monitor settings lock poisoned")
    })?;
    let mut settings = load_monitor_settings_unlocked(config_dir)?;
    mutate(&mut settings);
    save_monitor_settings_unlocked(config_dir, &settings)
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
