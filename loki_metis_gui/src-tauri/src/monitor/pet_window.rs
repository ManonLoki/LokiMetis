//! 桌宠独立悬浮窗：规格映射、创建/关闭与宫格读模型。

use loki_metis_core::{
    AiTool, HookError, PetOverlayImageRef, PetOverlayView, PetOverlayWindowSpec, ai_tool_name,
    pet_overlay_window_spec, project_pet_overlay_slots,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use super::images::{list_monitor_images, read_monitor_image};
use std::path::Path;

/// 已交付的悬浮窗描述，命令与测试共用，避免另写对照实现。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayWindowDescription {
    /// 窗口标签。
    pub label: &'static str,
    /// 前端路径。
    pub url_path: &'static str,
    /// 是否绘制系统标题栏。
    pub decorations: bool,
    /// 是否透明。
    pub transparent: bool,
    /// 是否始终置顶。
    pub always_on_top: bool,
    /// 是否跳过任务栏。
    pub skip_taskbar: bool,
    /// 是否隐藏主窗口。
    pub hide_main_window: bool,
    /// 是否停止 Hook listener。
    pub stop_hook_listener: bool,
    /// 默认宽度。
    pub default_width: f64,
    /// 默认高度。
    pub default_height: f64,
}

/// 前端宫格槽位。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlaySlotDto {
    /// 工具。
    pub tool: AiTool,
    /// 展示名。
    pub name: String,
    /// 是否有图。
    pub occupied: bool,
    /// 图库 ID。
    pub image_id: Option<String>,
}

/// 前端宫格快照。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayViewDto {
    /// 四槽。
    pub slots: Vec<PetOverlaySlotDto>,
}

/// 返回已交付悬浮窗规格映射。
pub fn pet_overlay_window_description() -> PetOverlayWindowDescription {
    let spec: PetOverlayWindowSpec = pet_overlay_window_spec();
    PetOverlayWindowDescription {
        label: spec.label,
        url_path: "/pet",
        decorations: spec.decorations,
        transparent: spec.transparent,
        always_on_top: spec.always_on_top,
        skip_taskbar: spec.skip_taskbar,
        hide_main_window: spec.hide_main_window,
        stop_hook_listener: spec.stop_hook_listener,
        default_width: spec.default_width,
        default_height: spec.default_height,
    }
}

/// 把本机图库投影为桌宠宫格 DTO。
pub fn pet_overlay_view_from_images(app_data_dir: &Path) -> Result<PetOverlayViewDto, HookError> {
    let records = list_monitor_images(app_data_dir)?;
    let images: Vec<PetOverlayImageRef> = records
        .iter()
        .map(|record| PetOverlayImageRef {
            tool: None,
            label: record.filename.clone(),
            image_key: record.id.clone(),
        })
        .collect();
    Ok(to_view_dto(project_pet_overlay_slots(&images)))
}

/// 打开或聚焦桌宠悬浮窗；不隐藏主窗口、不停止 listener。
pub fn show_or_create_pet_overlay(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    if description.hide_main_window {
        return Err("pet overlay must keep the main window visible".to_owned());
    }
    if description.stop_hook_listener {
        return Err("pet overlay must not stop the hook listener".to_owned());
    }
    if let Some(existing) = app.get_webview_window(description.label) {
        existing.show().map_err(|error| error.to_string())?;
        existing.unminimize().map_err(|error| error.to_string())?;
        existing
            .set_always_on_top(description.always_on_top)
            .map_err(|error| error.to_string())?;
        let _ = existing.set_focus();
        return Ok(description);
    }
    WebviewWindowBuilder::new(
        app,
        description.label,
        WebviewUrl::App("index.html".into()),
    )
    .title("LokiMetis")
    .inner_size(description.default_width, description.default_height)
    .decorations(description.decorations)
    .transparent(description.transparent)
    .always_on_top(description.always_on_top)
    .skip_taskbar(description.skip_taskbar)
    .resizable(false)
    .visible(true)
    .build()
    .map_err(|error| error.to_string())?;
    Ok(description)
}

/// 隐藏桌宠悬浮窗，主窗口保持原状，不销毁窗口以便再次打开。
pub fn close_pet_overlay_window(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    if let Some(window) = app.get_webview_window(description.label) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(description)
}

/// 开始拖动无边框悬浮窗。
pub fn start_pet_overlay_dragging(app: &AppHandle) -> Result<(), String> {
    let description = pet_overlay_window_description();
    let window = app
        .get_webview_window(description.label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    window.start_dragging().map_err(|error| error.to_string())
}

fn to_view_dto(view: PetOverlayView) -> PetOverlayViewDto {
    PetOverlayViewDto {
        slots: view
            .slots
            .into_iter()
            .map(|slot| PetOverlaySlotDto {
                name: if slot.name.is_empty() {
                    ai_tool_name(slot.tool).to_owned()
                } else {
                    slot.name
                },
                tool: slot.tool,
                occupied: slot.occupied,
                image_id: slot.image_key,
            })
            .collect(),
    }
}

/// 读取一张本机监控图字节。
pub fn overlay_image_bytes(app_data_dir: &Path, id: &str) -> Result<Vec<u8>, HookError> {
    read_monitor_image(app_data_dir, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::{PET_OVERLAY_WINDOW_SPEC, pet_overlay_window_spec};
    use tempfile::tempdir;

    #[test]
    fn pet_overlay_window_description_matches_core_spec() {
        let spec = pet_overlay_window_spec();
        let description = pet_overlay_window_description();
        assert_eq!(description.label, spec.label);
        assert!(!description.decorations);
        assert!(description.transparent);
        assert!(description.always_on_top);
        assert!(!description.hide_main_window);
        assert!(!description.stop_hook_listener);
        assert_eq!(description.url_path, "/pet");
        assert_eq!(description.decorations, PET_OVERLAY_WINDOW_SPEC.decorations);
        assert_eq!(
            description.hide_main_window,
            PET_OVERLAY_WINDOW_SPEC.hide_main_window
        );
    }

    #[test]
    fn pet_overlay_view_maps_local_images_and_rejects_cursor() {
        let root = tempdir().expect("temp");
        let dir = root.path();
        super::super::save_monitor_image(dir, "codex.png", b"png-bytes").expect("codex");
        super::super::save_monitor_image(dir, "cursor.png", b"cursor").expect("cursor");
        let view = pet_overlay_view_from_images(dir).expect("view");
        assert_eq!(view.slots.len(), 4);
        assert_eq!(view.slots[0].name, "Codex");
        assert!(view.slots[0].occupied);
        assert!(view.slots[0].image_id.is_some());
        assert!(!view.slots[1].occupied);
        assert_eq!(view.slots[1].name, "Claude Code");
        assert_eq!(view.slots[2].name, "Grok Build");
        assert_eq!(view.slots[3].name, "WorkBuddy");
        let joined = view
            .slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!joined.to_ascii_lowercase().contains("cursor"));
    }
}
