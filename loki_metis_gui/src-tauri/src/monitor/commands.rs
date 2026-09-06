//! 监控区 Tauri 命令：能力、设置、Hook 写入、中继状态与本机图片。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use loki_metis_core::{
    AiProfileDraft, AiProfileDraftSet, AiTool, HookConfigLocation, HookConfigWriteResult,
    HookError, MonitorImageGallery,
};
use tauri::{AppHandle, Manager, State};

use super::{
    HookConfigWriter, HookListenerControl, HookRelayStatus, MonitorCapabilities, MonitorSettings,
    PetOverlayWindowDescription, close_pet_overlay_window, delete_monitor_image,
    emit_pet_window_state_changed, list_hook_config_locations, list_monitor_image_gallery,
    load_monitor_settings, load_profile_drafts, monitor_capabilities, overlay_image_bytes,
    save_monitor_image, save_profile_draft, start_pet_overlay_dragging, update_monitor_settings,
    validate_hook_config_directory, write_hook_config,
};

fn config_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path().app_config_dir().map_err(|error| {
        HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string())
    })
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path().app_data_dir().map_err(|error| {
        HookError::new("error.monitor.imagesReadFailed").param("detail", error.to_string())
    })
}

/// 已保存展示仍引用图片时阻止删除，避免产生无法修复的悬空配置。
fn ensure_image_is_unused(profiles: &AiProfileDraftSet, image_id: &str) -> Result<(), HookError> {
    if profiles
        .drafts
        .iter()
        .flat_map(|draft| &draft.hooks)
        .any(|hook| hook.image == image_id)
    {
        return Err(HookError::new("error.monitor.imageInUse"));
    }
    Ok(())
}

/// 显式保存启用集合；仅在 JSON 本身损坏时以默认设置为基线修复。
fn save_enabled_tools_with_invalid_json_recovery(
    config_dir: &Path,
    tools: Vec<AiTool>,
) -> Result<MonitorSettings, HookError> {
    match update_monitor_settings(config_dir, |settings| {
        settings.enabled_ai_tools = tools.clone();
    }) {
        Ok(settings) => Ok(settings),
        Err(error) if error.code == "error.monitor.settingsInvalid" => {
            let mut settings = MonitorSettings::default();
            settings.enabled_ai_tools = tools;
            super::settings::save_monitor_settings(config_dir, &settings)
        }
        Err(error) => Err(error),
    }
}

/// 由 Tauri 路径解析器取得跨平台用户主目录，禁止 adapter 自行猜测环境变量。
fn home_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path().home_dir().map_err(|error| {
        HookError::new("error.hooks.homeDirectoryUnavailable").param("detail", error.to_string())
    })
}

/// 返回全部 Agent 的静态监控能力。
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
    hook_writer: State<'_, HookConfigWriter>,
    hook_listener: State<'_, HookListenerControl>,
) -> Result<MonitorSettings, HookError> {
    let config_dir = config_dir(&app)?;
    let settings = save_enabled_tools_with_invalid_json_recovery(&config_dir, tools)?;
    hook_writer.request_enabled(settings.clone());
    if hook_listener.replace_enabled_tools(&settings.enabled_ai_tools) {
        emit_pet_window_state_changed(&app);
    }
    Ok(settings)
}

/// 保存某工具的自定义 Hook 目录。
#[tauri::command]
pub fn save_hook_config_directory(
    app: AppHandle,
    tool: AiTool,
    directory: String,
    hook_writer: State<'_, HookConfigWriter>,
) -> Result<HookConfigLocation, HookError> {
    let directory = validate_hook_config_directory(&directory)?;
    let home_directory = home_dir(&app)?;
    let settings = update_monitor_settings(&config_dir(&app)?, |settings| {
        settings.hook_directories.set(tool, directory);
    })?;
    let location = list_hook_config_locations(&settings, &home_directory)
        .into_iter()
        .find(|location| location.tool == tool)
        .ok_or_else(|| HookError::new("error.hooks.locationNotFound"))?;
    hook_writer.request_enabled(settings);
    Ok(location)
}

/// 列出全部 Agent 的 Hook 配置定位。
#[tauri::command]
pub fn list_monitor_hook_locations(app: AppHandle) -> Result<Vec<HookConfigLocation>, HookError> {
    let settings = load_monitor_settings(&config_dir(&app)?)?;
    Ok(list_hook_config_locations(&settings, &home_dir(&app)?))
}

/// 为指定工具写入本机 Hook 配置。
#[tauri::command]
pub fn write_monitor_hook_config(
    app: AppHandle,
    tool: AiTool,
) -> Result<HookConfigWriteResult, HookError> {
    let settings = load_monitor_settings(&config_dir(&app)?)?;
    let executable = std::env::current_exe().map_err(|error| {
        HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
    })?;
    write_hook_config(&settings, tool, &executable, &home_dir(&app)?)
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
    let gallery = save_monitor_image(&data_dir(&app)?, &filename, &bytes)?;
    emit_pet_window_state_changed(&app);
    Ok(gallery)
}

/// 删除本机监控图片并返回更新后的图库。
#[tauri::command]
pub fn delete_monitor_image_cmd(
    app: AppHandle,
    id: String,
) -> Result<MonitorImageGallery, HookError> {
    let profiles = load_profile_drafts(&config_dir(&app)?)?;
    ensure_image_is_unused(&profiles, &id)?;
    let gallery = delete_monitor_image(&data_dir(&app)?, &id)?;
    emit_pet_window_state_changed(&app);
    Ok(gallery)
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
    let draft = save_profile_draft(&config_dir(&app)?, &data_dir(&app)?, profile)?;
    emit_pet_window_state_changed(&app);
    Ok(draft)
}

/// 读取桌宠槽位图片字节。
#[tauri::command]
pub fn get_monitor_image_bytes(app: AppHandle, id: String) -> Result<Vec<u8>, HookError> {
    overlay_image_bytes(&data_dir(&app)?, &id)
}

/// 关闭桌宠悬浮窗。
#[tauri::command]
pub fn close_pet_overlay(app: AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = close_pet_overlay_window(&app)?;
    if let Err(error) = crate::tray::refresh_pet_overlay_label(&app) {
        tracing::warn!(%error, "failed to refresh pet overlay tray label after close");
    }
    Ok(description)
}

/// 拖动无边框桌宠悬浮窗。
#[tauri::command]
pub fn start_pet_overlay_drag(app: AppHandle) -> Result<(), String> {
    start_pet_overlay_dragging(&app)
}

#[cfg(test)]
mod tests {
    use loki_metis_core::{AiProfileDraft, AiProfileDraftSet, AiTool};
    use tempfile::tempdir;

    use super::{ensure_image_is_unused, save_enabled_tools_with_invalid_json_recovery};

    /// 被任一行为引用的图片必须保留，未引用图片仍可删除。
    #[test]
    fn referenced_monitor_image_cannot_be_deleted() {
        let mut draft = AiProfileDraft::default_for(AiTool::Codex);
        draft.hooks[0].image = "used-image".to_owned();
        let profiles = AiProfileDraftSet {
            drafts: vec![draft],
        };

        let error = ensure_image_is_unused(&profiles, "used-image").expect_err("in use");
        assert_eq!(error.code, "error.monitor.imageInUse");
        ensure_image_is_unused(&profiles, "other-image").expect("unused image");
    }

    /// 用户操作可以修复损坏 JSON，并只启用用户这次明确选择的集合。
    #[test]
    fn enabled_tools_save_repairs_invalid_settings_json() {
        let root = tempdir().expect("temp");
        std::fs::write(super::super::settings::settings_path(root.path()), b"{")
            .expect("invalid settings");

        let saved = save_enabled_tools_with_invalid_json_recovery(root.path(), vec![AiTool::Grok])
            .expect("repair");
        assert_eq!(saved.enabled_ai_tools, vec![AiTool::Grok]);
        assert_eq!(
            super::super::settings::load_monitor_settings(root.path())
                .expect("reload")
                .enabled_ai_tools,
            vec![AiTool::Grok]
        );
    }
}
