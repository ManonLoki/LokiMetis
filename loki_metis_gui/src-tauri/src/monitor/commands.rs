//! 监控区 Tauri 命令：能力、设置、Hook 写入、中继状态与本机图片。

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use loki_metis_core::{AiTool, HookConfigLocation, HookConfigWriteResult, HookError};
use tauri::{AppHandle, Manager, State};

use super::{
    HookRelayStatus, MonitorCapabilities, MonitorImageRecord, MonitorSettings,
    PetOverlayViewDto, PetOverlayWindowDescription, close_pet_overlay_window,
    delete_monitor_image, list_hook_config_locations, list_monitor_images, load_monitor_settings,
    monitor_capabilities, overlay_image_bytes, pet_overlay_view_from_images,
    save_monitor_image, save_monitor_settings, show_or_create_pet_overlay,
    start_pet_overlay_dragging, write_hook_config,
};

fn config_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path()
        .app_config_dir()
        .map_err(|error| HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string()))
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path()
        .app_data_dir()
        .map_err(|error| HookError::new("error.monitor.imagesReadFailed").param("detail", error.to_string()))
}

/// 返回四项 Agent 的静态监控能力。
#[tauri::command]
pub fn get_monitor_capabilities() -> MonitorCapabilities {
    monitor_capabilities()
}

/// 读取监控设置。
#[tauri::command]
pub fn get_monitor_settings(app: AppHandle) -> Result<MonitorSettings, HookError> {
    load_monitor_settings(&config_dir(&app)?)
}

/// 保存已启用 Agent。
#[tauri::command]
pub fn save_monitor_enabled_tools(
    app: AppHandle,
    tools: Vec<AiTool>,
) -> Result<MonitorSettings, HookError> {
    let dir = config_dir(&app)?;
    let mut settings = load_monitor_settings(&dir)?;
    settings.enabled_ai_tools = tools;
    save_monitor_settings(&dir, &settings)?;
    load_monitor_settings(&dir)
}

/// 保存某工具的自定义 Hook 目录。
#[tauri::command]
pub fn save_hook_config_directory(
    app: AppHandle,
    tool: AiTool,
    directory: String,
) -> Result<MonitorSettings, HookError> {
    let dir = config_dir(&app)?;
    let mut settings = load_monitor_settings(&dir)?;
    settings.hook_directories.set(tool, directory);
    save_monitor_settings(&dir, &settings)?;
    load_monitor_settings(&dir)
}

/// 列出四项 Agent 的 Hook 配置定位。
#[tauri::command]
pub fn list_monitor_hook_locations(app: AppHandle) -> Result<Vec<HookConfigLocation>, HookError> {
    let settings = load_monitor_settings(&config_dir(&app)?)?;
    Ok(list_hook_config_locations(&settings))
}

/// 为指定工具写入本机 Hook 配置。
#[tauri::command]
pub fn write_monitor_hook_config(
    app: AppHandle,
    tool: AiTool,
) -> Result<HookConfigWriteResult, HookError> {
    let settings = load_monitor_settings(&config_dir(&app)?)?;
    let executable = std::env::current_exe()
        .map_err(|error| HookError::new("error.hooks.writeFailed").param("detail", error.to_string()))?;
    write_hook_config(&settings, tool, &executable)
}

/// 读取本机 Hook 中继状态。
#[tauri::command]
pub fn get_hook_relay_status(
    status: State<'_, Arc<RwLock<HookRelayStatus>>>,
) -> Result<HookRelayStatus, HookError> {
    status
        .read()
        .map(|guard| guard.clone())
        .map_err(|_| HookError::new("error.monitor.relayStatusUnavailable"))
}

/// 列出本机监控图片。
#[tauri::command]
pub fn list_monitor_images_cmd(app: AppHandle) -> Result<Vec<MonitorImageRecord>, HookError> {
    list_monitor_images(&data_dir(&app)?)
}

/// 保存本机监控图片。
#[tauri::command]
pub fn save_monitor_image_cmd(
    app: AppHandle,
    filename: String,
    bytes: Vec<u8>,
) -> Result<MonitorImageRecord, HookError> {
    save_monitor_image(&data_dir(&app)?, &filename, &bytes)
}

/// 删除本机监控图片。
#[tauri::command]
pub fn delete_monitor_image_cmd(app: AppHandle, id: String) -> Result<(), HookError> {
    delete_monitor_image(&data_dir(&app)?, &id)
}

/// 读取桌宠宫格投影。
#[tauri::command]
pub fn get_pet_overlay_view(app: AppHandle) -> Result<PetOverlayViewDto, HookError> {
    pet_overlay_view_from_images(&data_dir(&app)?)
}

/// 读取桌宠槽位图片字节。
#[tauri::command]
pub fn get_monitor_image_bytes(app: AppHandle, id: String) -> Result<Vec<u8>, HookError> {
    overlay_image_bytes(&data_dir(&app)?, &id)
}

/// 打开桌宠悬浮窗。
#[tauri::command]
pub fn open_pet_overlay(app: AppHandle) -> Result<PetOverlayWindowDescription, String> {
    show_or_create_pet_overlay(&app)
}

/// 关闭桌宠悬浮窗。
#[tauri::command]
pub fn close_pet_overlay(app: AppHandle) -> Result<PetOverlayWindowDescription, String> {
    close_pet_overlay_window(&app)
}

/// 拖动无边框桌宠悬浮窗。
#[tauri::command]
pub fn start_pet_overlay_drag(app: AppHandle) -> Result<(), String> {
    start_pet_overlay_dragging(&app)
}
