//! 监控区 Tauri 命令：能力、设置、Hook 写入、中继状态与本机图片。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use loki_metis_core::{
    AiProfileDraft, AiProfileDraftSet, AiTool, HookConfigLocation, HookConfigWriteResult,
    HookError, MonitorImageGallery, is_public_monitor_tool, public_monitor_ai_tools,
};
use tauri::{AppHandle, Manager, State};

use crate::dto::{PrivacySettingsDto, UsageClientKindDto};
use crate::runtime::AppRuntimeState;
use crate::tray::refresh_tray_daily_token_title;

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

/// 拒绝通过公开 IPC 操作暂未发布的 Hook 协议，同时不删除其内部配置数据。
fn ensure_public_monitor_tool(tool: AiTool) -> Result<(), HookError> {
    if is_public_monitor_tool(tool) {
        Ok(())
    } else {
        Err(HookError::new("error.hooks.toolUnavailable"))
    }
}

/// 按统一目录重建 IPC 快照，不改写磁盘中的隐藏协议草稿，便于以后恢复。
fn public_profile_drafts(profiles: AiProfileDraftSet) -> AiProfileDraftSet {
    AiProfileDraftSet {
        drafts: public_monitor_ai_tools()
            .filter_map(|tool| {
                profiles
                    .drafts
                    .iter()
                    .find(|profile| profile.tool == tool)
                    .cloned()
            })
            .collect(),
    }
}

/// 基于完整合法快照保存启用集合；损坏文件不得被默认值静默覆盖。
fn save_enabled_tools(config_dir: &Path, tools: Vec<AiTool>) -> Result<MonitorSettings, HookError> {
    update_monitor_settings(config_dir, |settings| {
        settings.enabled_ai_tools = tools.clone();
    })
}

/// 统一 Agent 选择保存后返回两个既有查询需要的权威快照。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnabledAiSelectionResult {
    /// Hooks、桌宠与动态能力页使用的监控设置。
    pub monitor_settings: MonitorSettings,
    /// 看板查询使用的兼容设置投影。
    pub privacy_settings: PrivacySettingsDto,
}

/// 由 Tauri 路径解析器取得跨平台用户主目录，禁止 adapter 自行猜测环境变量。
fn home_dir(app: &AppHandle) -> Result<PathBuf, HookError> {
    app.path().home_dir().map_err(|error| {
        HookError::new("error.hooks.homeDirectoryUnavailable").param("detail", error.to_string())
    })
}

/// 返回统一目录中当前公开的静态监控能力。
#[tauri::command]
pub fn get_monitor_capabilities() -> MonitorCapabilities {
    monitor_capabilities()
}

/// 读取监控设置。
#[tauri::command]
pub fn get_monitor_settings(app: AppHandle) -> Result<MonitorSettings, HookError> {
    load_monitor_settings(&config_dir(&app)?)
}

/// 一次保存全局 Agent 选择，并在任一持久层失败时回滚已写入的一侧。
#[tauri::command]
pub async fn save_enabled_ai_selection(
    app: AppHandle,
    client: UsageClientKindDto,
    tools: Vec<AiTool>,
    state: State<'_, AppRuntimeState>,
    hook_writer: State<'_, HookConfigWriter>,
    hook_listener: State<'_, HookListenerControl>,
) -> Result<EnabledAiSelectionResult, HookError> {
    let config_dir = config_dir(&app)?;
    let previous_dashboard = state.enabled_ai_tools_from_dashboard().await;
    state.set_enabled_ai_tools(&tools).await.map_err(|detail| {
        HookError::new("error.monitor.settingsWriteFailed").param("detail", detail)
    })?;
    let settings = match save_enabled_tools(&config_dir, tools) {
        Ok(settings) => settings,
        Err(error) => {
            if state
                .set_enabled_ai_tools(&previous_dashboard)
                .await
                .is_err()
            {
                tracing::error!("统一 Agent 选择保存失败后无法回滚看板设置");
            }
            return Err(error);
        }
    };
    hook_writer.request_enabled(settings.clone());
    if hook_listener.replace_enabled_tools(&settings.enabled_ai_tools) {
        emit_pet_window_state_changed(&app);
    }
    refresh_tray_daily_token_title(&app).await;
    Ok(EnabledAiSelectionResult {
        monitor_settings: settings,
        privacy_settings: state.privacy_settings(client).await,
    })
}

/// 保存某工具的自定义 Hook 目录。
#[tauri::command]
pub fn save_hook_config_directory(
    app: AppHandle,
    tool: AiTool,
    directory: String,
    hook_writer: State<'_, HookConfigWriter>,
) -> Result<HookConfigLocation, HookError> {
    ensure_public_monitor_tool(tool)?;
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

/// 列出统一目录中当前公开 Agent 的 Hook 配置定位。
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
    ensure_public_monitor_tool(tool)?;
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
    load_profile_drafts(&config_dir(&app)?).map(public_profile_drafts)
}

/// 保存一个 Agent 的展示草稿。
#[tauri::command]
pub fn save_monitor_profile_draft(
    app: AppHandle,
    profile: AiProfileDraft,
) -> Result<AiProfileDraft, HookError> {
    ensure_public_monitor_tool(profile.tool)?;
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

    use super::{
        ensure_image_is_unused, ensure_public_monitor_tool, public_profile_drafts,
        save_enabled_tools,
    };

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

    /// 损坏 JSON 不能被一次启用操作用默认设置覆盖，避免丢失其它持久偏好。
    #[test]
    fn enabled_tools_save_rejects_invalid_settings_json() {
        let root = tempdir().expect("temp");
        std::fs::write(super::super::settings::settings_path(root.path()), b"{")
            .expect("invalid settings");

        let error = save_enabled_tools(root.path(), vec![AiTool::Grok])
            .expect_err("invalid settings remain visible");
        assert_eq!(error.code, "error.monitor.settingsInvalid");
        assert_eq!(
            std::fs::read(super::super::settings::settings_path(root.path()))
                .expect("invalid file remains untouched"),
            b"{"
        );
    }

    /// 公开命令只接受统一目录五项，隐藏协议不能绕过界面直接修改。
    #[test]
    fn public_monitor_commands_reject_hidden_protocols() {
        for tool in [
            AiTool::Codex,
            AiTool::ClaudeCode,
            AiTool::Cursor,
            AiTool::Grok,
            AiTool::WorkBuddy,
        ] {
            ensure_public_monitor_tool(tool).expect("public tool");
        }
        assert_eq!(
            ensure_public_monitor_tool(AiTool::OpenCode)
                .expect_err("hidden tool")
                .code,
            "error.hooks.toolUnavailable"
        );
    }

    /// 草稿 IPC 仅返回五项公开工具，内部隐藏草稿仍由原始集合持有。
    #[test]
    fn profile_ipc_filters_hidden_tools_without_mutating_the_source_set() {
        let original = AiProfileDraftSet {
            drafts: vec![
                AiProfileDraft::default_for(AiTool::OpenCode),
                AiProfileDraft::default_for(AiTool::WorkBuddy),
                AiProfileDraft::default_for(AiTool::Cursor),
                AiProfileDraft::default_for(AiTool::Grok),
                AiProfileDraft::default_for(AiTool::Codex),
                AiProfileDraft::default_for(AiTool::ClaudeCode),
            ],
        };

        let public = public_profile_drafts(original.clone());

        assert_eq!(original.drafts.len(), 6);
        assert_eq!(
            public
                .drafts
                .iter()
                .map(|profile| profile.tool)
                .collect::<Vec<_>>(),
            vec![
                AiTool::Codex,
                AiTool::ClaudeCode,
                AiTool::Cursor,
                AiTool::Grok,
                AiTool::WorkBuddy,
            ]
        );
    }
}
