//! 监控区 Tauri 命令：能力、设置、Hook 写入、中继状态与本机图片。

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use loki_metis_core::{
    AiProfileDraft, AiProfileDraftSet, AiTool, HookConfigLocation, HookConfigWriteResult, HookError,
    MonitorImageGallery, PetOverlayPosition,
};
use tauri::{AppHandle, Manager, State};

use super::{
    HookRelayStatus, MonitorCapabilities, MonitorSettings, PetOverlayViewDto,
    PetOverlayWindowDescription, close_pet_overlay_window, delete_monitor_image,
    list_hook_config_locations, list_monitor_image_gallery, load_monitor_settings,
    load_profile_drafts, monitor_capabilities, overlay_image_bytes, pet_overlay_view_from_drafts,
    persist_pet_overlay_position, pet_overlay_window_description, pet_overlay_window_is_open,
    pet_overlay_work_areas, read_pet_overlay_position, save_monitor_image, save_monitor_settings,
    save_profile_draft,
    show_or_create_pet_overlay, start_pet_overlay_dragging, write_hook_config,
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

/// 列出本机监控图库快照。
#[tauri::command]
pub fn list_monitor_images_cmd(app: AppHandle) -> Result<MonitorImageGallery, HookError> {
    list_monitor_image_gallery(&data_dir(&app)?)
}

/// 保存本机监控图片并返回更新后的图库。
#[tauri::command]
pub fn save_monitor_image_cmd(
    app: AppHandle,
    filename: String,
    bytes: Vec<u8>,
) -> Result<MonitorImageGallery, HookError> {
    save_monitor_image(&data_dir(&app)?, &filename, &bytes)
}

/// 删除本机监控图片并返回更新后的图库。
#[tauri::command]
pub fn delete_monitor_image_cmd(
    app: AppHandle,
    id: String,
) -> Result<MonitorImageGallery, HookError> {
    delete_monitor_image(&data_dir(&app)?, &id)
}

/// 读取本机展示草稿。
#[tauri::command]
pub fn list_monitor_profile_drafts(app: AppHandle) -> Result<AiProfileDraftSet, HookError> {
    load_profile_drafts(&config_dir(&app)?)
}

/// 保存一个 Agent 的展示草稿。
#[tauri::command]
pub fn save_monitor_profile_draft(
    app: AppHandle,
    profile: AiProfileDraft,
) -> Result<AiProfileDraft, HookError> {
    save_profile_draft(&config_dir(&app)?, &data_dir(&app)?, profile)
}

/// 读取桌宠宫格投影。
#[tauri::command]
pub fn get_pet_overlay_view(
    app: AppHandle,
    status: State<'_, Arc<RwLock<HookRelayStatus>>>,
) -> Result<PetOverlayViewDto, HookError> {
    let behaviors = status
        .read()
        .map(|guard| guard.last_behaviors.clone())
        .map_err(|_| HookError::new("error.monitor.relayStatusUnavailable"))?;
    pet_overlay_view_from_drafts(&config_dir(&app)?, &behaviors)
}

/// 读取桌宠槽位图片字节。
#[tauri::command]
pub fn get_monitor_image_bytes(app: AppHandle, id: String) -> Result<Vec<u8>, HookError> {
    overlay_image_bytes(&data_dir(&app)?, &id)
}

/// 保存兔耳（圆形关闭控件）显示偏好。
#[tauri::command]
pub fn save_pet_close_control_visible(
    app: AppHandle,
    visible: bool,
) -> Result<MonitorSettings, HookError> {
    let dir = config_dir(&app)?;
    let mut settings = load_monitor_settings(&dir)?;
    settings.pet_close_control_visible = visible;
    save_monitor_settings(&dir, &settings)?;
    load_monitor_settings(&dir)
}

/// 查询桌宠悬浮窗当前是否打开。
#[tauri::command]
pub fn is_pet_overlay_open(app: AppHandle) -> bool {
    pet_overlay_window_is_open(&app)
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

/// 从宿主读回当前浮窗位置。
#[tauri::command]
pub fn get_pet_overlay_position(app: AppHandle) -> Result<PetOverlayPosition, String> {
    read_pet_overlay_position(&app)
}

/// 保存浮窗位置；必须用窗口实际物理 outer_size，不能把逻辑默认宽高当物理像素。
#[tauri::command]
pub fn save_pet_overlay_position(
    app: AppHandle,
    position: PetOverlayPosition,
) -> Result<MonitorSettings, HookError> {
    let dir = config_dir(&app)?;
    let overlay_size = app
        .get_webview_window(pet_overlay_window_description().label)
        .and_then(|window| window.outer_size().ok())
        .map(|size| (size.width, size.height))
        .filter(|size| size.0 > 0 && size.1 > 0)
        .ok_or_else(|| HookError::new("error.monitor.settingsWriteFailed"))?;
    persist_pet_overlay_position(&dir, position, overlay_size, &pet_overlay_work_areas(&app))?;
    load_monitor_settings(&dir)
}
