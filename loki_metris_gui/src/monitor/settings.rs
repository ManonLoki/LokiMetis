//! 监控区本机设置：已启用 Agent 与 Hook 目录覆盖。

use std::{
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use loki_metris_core::{
    AiTool, HookConfigDirectories, HookError, PetOverlayPosition,
    normalize_enabled_ai_tools,
};

/// 目录覆盖最多14项，设置读取和原子保存必须共享同一字节上限。
const MAX_SETTINGS_BYTES:u64=128*1024;
use serde::{Deserialize, Serialize};

/// 监控设置文件的进程内读写锁；所有配置目录共用以保证简单且确定的串行化。
static MONITOR_SETTINGS_LOCK: OnceLock<RwLock<()>> = OnceLock::new();

/// 返回监控设置文件的全局读写锁。
fn monitor_settings_lock() -> &'static RwLock<()> {
    MONITOR_SETTINGS_LOCK.get_or_init(|| RwLock::new(()))
}

pub use loki_metris_core::PetWindowPreferences;
use loki_metris_core::normalize_pet_window_preferences;

/// 解析 petWindow 对象本身，畸形对象只回退桌宠子设置。
fn deserialize_pet_window_preferences<'de, D>(
    deserializer: D,
) -> Result<PetWindowPreferences, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(PetWindowPreferences::deserialize(value).unwrap_or_default())
}

/// 逐项解析历史启用列表；未来或已隐藏的字符串不扩大权限，非字符串形状仍视为损坏。
fn deserialize_enabled_ai_tools<'de, D>(deserializer: D) -> Result<Vec<AiTool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let labels = Vec::<String>::deserialize(deserializer)?;
    Ok(labels
        .into_iter()
        .filter_map(|label| serde_json::from_value(serde_json::Value::String(label)).ok())
        .collect())
}

/// 监控区持久设置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 已启用的 Agent；显式空集合表示暂不自动补写任何工具。
    #[serde(
        default = "default_enabled_ai_tools",
        deserialize_with = "deserialize_enabled_ai_tools"
    )]
    pub enabled_ai_tools: Vec<AiTool>,
    /// 自定义 Hook 配置目录。
    #[serde(default)]
    pub hook_directories: HookConfigDirectories,
    /// 最近一次合法的桌宠浮窗位置；缺失表示使用默认几何。
    #[serde(default)]
    pub pet_overlay_position: Option<PetOverlayPosition>,
    /// 版本1兼容 Windows 历史外框坐标；版本2统一为真实客户区画布物理原点。
    #[serde(default = "legacy_position_version", deserialize_with = "deserialize_position_version")]
    pub pet_overlay_position_version: u8,
    /// 桌宠布局、分页、大小、置顶与锁定偏好。
    #[serde(default, deserialize_with = "deserialize_pet_window_preferences")]
    pub pet_window: PetWindowPreferences,
}

impl Default for MonitorSettings {
    /// 返回首次启动使用的工具集合与桌宠偏好。
    fn default() -> Self {
        Self {
            enabled_ai_tools: default_enabled_ai_tools(),
            hook_directories: HookConfigDirectories::default(),
            pet_overlay_position: None,
            pet_overlay_position_version: 1,
            pet_window: PetWindowPreferences::default(),
        }
    }
}

/// 缺失标记的历史文件不能按新的客户区语义再减一次边框。
fn legacy_position_version() -> u8 { 1 }

/// 损坏或未来版本只回退坐标语义，不让整个监控设置无法读取。
fn deserialize_position_version<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    Ok(if serde_json::Value::deserialize(deserializer)?.as_u64() == Some(2) { 2 } else { 1 })
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

/// 在调用方已持有读锁或写锁时读取并规范化监控设置。
fn load_monitor_settings_unlocked(config_dir: &Path) -> Result<MonitorSettings, HookError> {
    let path = settings_path(config_dir);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(MonitorSettings::default()),
        Err(error) => return Err(HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string())),
    };
    // 目录覆盖最多14项；拒绝链接、特殊文件和意外超大配置，避免阻塞整个设置页。
    if !metadata.is_file() || metadata.len() > MAX_SETTINGS_BYTES {
        return Err(HookError::new("error.monitor.settingsInvalid"));
    }
    use std::io::Read;
    let mut raw = String::new();
    std::fs::File::open(&path).and_then(|file| file.take(MAX_SETTINGS_BYTES + 1).read_to_string(&mut raw)).map_err(|error| {
        HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string())
    })?;
    if raw.len() as u64 > MAX_SETTINGS_BYTES { return Err(HookError::new("error.monitor.settingsInvalid")); }
    let mut settings: MonitorSettings = serde_json::from_str(&raw).map_err(|error| {
        HookError::new("error.monitor.settingsInvalid").param("detail", error.to_string())
    })?;
    settings.enabled_ai_tools = normalize_enabled_ai_tools(&settings.enabled_ai_tools);
    settings.pet_window = normalize_pet_window_preferences(settings.pet_window);
    if settings.pet_overlay_position_version != 2 { settings.pet_overlay_position_version = 1; }
    Ok(settings)
}

#[cfg(test)]
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
        pet_overlay_position_version: settings.pet_overlay_position_version,
        pet_window: normalize_pet_window_preferences(settings.pet_window),
    };
    let raw = serde_json::to_string_pretty(&normalized).map_err(|error| {
        HookError::new("error.monitor.settingsWriteFailed").param("detail", error.to_string())
    })?;
    if raw.len() as u64>MAX_SETTINGS_BYTES {return Err(HookError::new("error.monitor.settingsWriteFailed"));}
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

/// 同一写锁内以最新磁盘快照合并桌宠字段；实际值未变时不重写文件或制造提交事实。
pub(crate) fn update_pet_settings_if_changed(
    config_dir: &Path,
    change: Option<(loki_metris_core::PetPreferenceChange, loki_metris_core::PetSizeRange)>,
    position: Option<PetOverlayPosition>,
) -> Result<(MonitorSettings, bool), HookError> {
    let _guard = monitor_settings_lock().write().map_err(|_| {
        HookError::new("error.monitor.settingsWriteFailed")
            .param("detail", "monitor settings lock poisoned")
    })?;
    let mut current = load_monitor_settings_unlocked(config_dir)?;
    let preferences = match change {
        Some((change, range)) => {
            loki_metris_core::change_pet_preferences(current.pet_window, change, range)?
        }
        None => current.pet_window,
    };
    let version = if position.is_some() { 2 } else { current.pet_overlay_position_version };
    let position = position.or(current.pet_overlay_position);
    if preferences == current.pet_window && position == current.pet_overlay_position && version == current.pet_overlay_position_version {
        return Ok((current, false));
    }
    current.pet_window = preferences;
    current.pet_overlay_position = position;
    current.pet_overlay_position_version = version;
    save_monitor_settings_unlocked(config_dir, &current).map(|settings| (settings, true))
}
