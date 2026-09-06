//! 桌宠设置、分页和窗口交互命令。

use std::sync::{Arc, RwLock};

use loki_metis_core::{
    HookError, PetLayout, pet_overlay_page_count, project_pet_overlay_from_drafts,
    wrap_pet_overlay_page,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::images::readable_monitor_image_ids;
use super::listener::HookRelayStatus;
use super::pet_events::emit_pet_window_state_changed;
use super::pet_geometry::{apply_pet_size, clamp_pet_window_to_work_area};
use super::pet_view::{PetOverlaySlotDto, PetOverlayViewDto, pet_overlay_view_from_drafts};
use super::pet_window::{
    current_pet_size_range, hide_pet_settings_window, pet_overlay_window_description,
    show_pet_settings_window,
};
use super::profiles::load_profile_drafts;
use super::settings::{load_monitor_settings, load_pet_runtime_settings, update_monitor_settings};

/// 桌宠设置窗和浮窗共同读取的完整状态。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetWindowStateDto {
    /// 当前布局。
    pub layout: PetLayout,
    /// 是否锁定拖拽和缩放。
    pub locked: bool,
    /// 从零开始的当前页。
    pub page_index: usize,
    /// 当前布局总页数。
    pub page_count: usize,
    /// 当前页是否有图片。
    pub page_has_image: bool,
    /// 十二个位置中是否有图片。
    pub has_any_image: bool,
    /// 当前页纯位置槽位。
    pub slots: Vec<PetOverlaySlotDto>,
    /// 单格逻辑像素边长。
    pub pet_size: u16,
    /// 当前显示器上的最小单格边长。
    pub size_min: u16,
    /// 当前显示器上的最大单格边长。
    pub size_max: u16,
    /// 是否始终置顶。
    pub always_on_top: bool,
}

/// 桌宠翻页方向。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PetPageDirection {
    /// 上一页。
    Previous,
    /// 下一页。
    Next,
}

/// 桌宠步进缩放方向。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PetResizeDirection {
    /// 放大。
    Grow,
    /// 缩小。
    Shrink,
}

/// 从配置中的 focused slot 推导规范化页码。
fn current_page(layout: PetLayout, focused_slot: u8) -> usize {
    (usize::from(focused_slot) / layout.capacity()) % pet_overlay_page_count(layout)
}

/// 读取桌宠渲染和设置所需的完整状态。
#[tauri::command]
pub fn get_pet_window_state(
    app: AppHandle,
    status: State<'_, Arc<RwLock<HookRelayStatus>>>,
) -> Result<PetWindowStateDto, HookError> {
    let config_dir = app.path().app_config_dir().map_err(|error| {
        HookError::new("error.monitor.settingsReadFailed").param("detail", error.to_string())
    })?;
    let app_data_dir = app.path().app_data_dir().map_err(|error| {
        HookError::new("error.monitor.imagesReadFailed").param("detail", error.to_string())
    })?;
    let settings = load_pet_runtime_settings(&config_dir);
    let states = status
        .read()
        .map(|guard| guard.pet_states.clone())
        .map_err(|_| HookError::new("error.monitor.relayStatusUnavailable"))?;
    let view = pet_overlay_view_from_drafts(
        &config_dir,
        &app_data_dir,
        &states,
        settings.pet_window.layout,
        current_page(settings.pet_window.layout, settings.pet_window.focused_slot),
    )?;
    let (size_min, size_max) = current_pet_size_range(&app, settings.pet_window.layout);
    Ok(state_from_view(view, &settings, size_min, size_max))
}

/// 保留旧浮窗查询命令，并让它返回与新设置状态一致的当前页。
#[tauri::command]
pub fn get_pet_overlay_view(
    app: AppHandle,
    status: State<'_, Arc<RwLock<HookRelayStatus>>>,
) -> Result<PetOverlayViewDto, HookError> {
    let state = get_pet_window_state(app, status)?;
    Ok(PetOverlayViewDto {
        layout: state.layout,
        page_index: state.page_index,
        page_count: state.page_count,
        page_has_image: state.page_has_image,
        has_any_image: state.has_any_image,
        slots: state.slots,
    })
}

/// 打开独立桌宠设置窗口。
#[tauri::command]
pub fn show_pet_settings(app: AppHandle) -> Result<(), String> {
    show_pet_settings_window(&app)
}

/// 隐藏独立桌宠设置窗口。
#[tauri::command]
pub fn hide_pet_settings(app: AppHandle) -> Result<(), String> {
    hide_pet_settings_window(&app)
}

/// 切换六种桌宠布局并按当前显示器约束窗口尺寸。
#[tauri::command]
pub fn set_pet_layout(app: AppHandle, layout: PetLayout) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let previous = load_monitor_settings(&config_dir).map_err(|error| error.to_string())?;
    let window = app
        .get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    let applied = apply_pet_size(&window, layout, previous.pet_window.pet_size)?;
    let saved = update_monitor_settings(&config_dir, |settings| {
        settings.pet_window.layout = layout;
        settings.pet_window.pet_size = applied;
        let page = current_page(layout, settings.pet_window.focused_slot);
        settings.pet_window.focused_slot = (page * layout.capacity()).min(11) as u8;
    });
    if let Err(error) = saved {
        let _ = apply_pet_size(
            &window,
            previous.pet_window.layout,
            previous.pet_window.pet_size,
        );
        return Err(error.to_string());
    }
    clamp_pet_window_to_work_area(&window);
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 设置桌宠单格大小；越界值显式拒绝。
#[tauri::command]
pub fn set_pet_size(app: AppHandle, size: u16) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let previous = load_monitor_settings(&config_dir).map_err(|error| error.to_string())?;
    let (min, max) = current_pet_size_range(&app, previous.pet_window.layout);
    if !(min..=max).contains(&size) {
        return Err(format!("pet size must be between {min} and {max}"));
    }
    let window = app
        .get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    let applied = apply_pet_size(&window, previous.pet_window.layout, size)?;
    let saved = update_monitor_settings(&config_dir, |settings| {
        settings.pet_window.pet_size = applied;
    });
    if let Err(error) = saved {
        let _ = apply_pet_size(
            &window,
            previous.pet_window.layout,
            previous.pet_window.pet_size,
        );
        return Err(error.to_string());
    }
    clamp_pet_window_to_work_area(&window);
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 设置桌宠始终置顶状态，原生调用成功后才落盘。
#[tauri::command]
pub fn set_pet_always_on_top(app: AppHandle, enabled: bool) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let previous = load_monitor_settings(&config_dir).map_err(|error| error.to_string())?;
    let window = app
        .get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    window
        .set_always_on_top(enabled)
        .map_err(|error| error.to_string())?;
    let saved = update_monitor_settings(&config_dir, |settings| {
        settings.pet_window.always_on_top = enabled;
    });
    if let Err(error) = saved {
        let _ = window.set_always_on_top(previous.pet_window.always_on_top);
        return Err(error.to_string());
    }
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 设置桌宠锁定状态；锁定同时关闭原生边缘缩放。
#[tauri::command]
pub fn set_pet_locked(app: AppHandle, locked: bool) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let previous = load_monitor_settings(&config_dir).map_err(|error| error.to_string())?;
    let window = app
        .get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    window
        .set_resizable(!locked)
        .map_err(|error| error.to_string())?;
    let saved = update_monitor_settings(&config_dir, |settings| {
        settings.pet_window.locked = locked;
    });
    if let Err(error) = saved {
        let _ = window.set_resizable(!previous.pet_window.locked);
        return Err(error.to_string());
    }
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 循环切换上一页或下一页，并保存新页首位置。
#[tauri::command]
pub fn turn_pet_page(app: AppHandle, direction: PetPageDirection) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    update_monitor_settings(&config_dir, |settings| {
        let layout = settings.pet_window.layout;
        let page = current_page(layout, settings.pet_window.focused_slot);
        let delta = match direction {
            PetPageDirection::Previous => -1,
            PetPageDirection::Next => 1,
        };
        let next = wrap_pet_overlay_page(layout, page, delta);
        settings.pet_window.focused_slot = (next * layout.capacity()) as u8;
    })
    .map_err(|error| error.to_string())?;
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 当前页无图时跳到第一个含图的位置所在页。
#[tauri::command]
pub fn focus_first_populated_pet_page(
    app: AppHandle,
    status: State<'_, Arc<RwLock<HookRelayStatus>>>,
) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let settings = load_pet_runtime_settings(&config_dir);
    let states = status
        .read()
        .map(|guard| guard.pet_states.clone())
        .map_err(|_| "hook relay status unavailable".to_owned())?;
    let drafts = load_profile_drafts(&config_dir).map_err(|error| error.to_string())?;
    let readable_ids =
        readable_monitor_image_ids(&app_data_dir).map_err(|error| error.to_string())?;
    let view = project_pet_overlay_from_drafts(&drafts.drafts, &states);
    let start = current_page(settings.pet_window.layout, settings.pet_window.focused_slot)
        * settings.pet_window.layout.capacity();
    let end = (start + settings.pet_window.layout.capacity()).min(view.slots.len());
    let has_readable_image = |slot: &loki_metis_core::PetOverlaySlot| {
        slot.tile
            .as_ref()
            .and_then(|tile| tile.image_key.as_ref())
            .is_some_and(|image_id| readable_ids.contains(image_id))
    };
    if view.slots[start..end].iter().any(has_readable_image) {
        return Ok(());
    }
    let Some(slot) = view.slots.iter().position(has_readable_image) else {
        return Ok(());
    };
    update_monitor_settings(&config_dir, |settings| {
        settings.pet_window.focused_slot = slot as u8;
    })
    .map_err(|error| error.to_string())?;
    emit_pet_window_state_changed(&app);
    Ok(())
}

/// 以 AIMonitorDesktop 的 24 像素步长缩放桌宠。
#[tauri::command]
pub fn resize_pet_step(app: AppHandle, direction: PetResizeDirection) -> Result<(), String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let settings = load_pet_runtime_settings(&config_dir);
    if settings.pet_window.locked {
        return Ok(());
    }
    let (min, max) = current_pet_size_range(&app, settings.pet_window.layout);
    let delta = match direction {
        PetResizeDirection::Grow => 24_i32,
        PetResizeDirection::Shrink => -24_i32,
    };
    let size = (i32::from(settings.pet_window.pet_size) + delta)
        .clamp(i32::from(min), i32::from(max)) as u16;
    set_pet_size(app, size)
}

/// 恢复主窗口；不隐藏桌宠，保持 LokiMetis 的双窗共存差异。
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    hide_pet_settings_window(&app)?;
    crate::windowing::restore_main_window(&app).map_err(|error| error.to_string())
}

/// 把分页投影与持久偏好合并为完整状态 DTO。
fn state_from_view(
    view: PetOverlayViewDto,
    settings: &super::settings::MonitorSettings,
    size_min: u16,
    size_max: u16,
) -> PetWindowStateDto {
    PetWindowStateDto {
        layout: view.layout,
        locked: settings.pet_window.locked,
        page_index: view.page_index,
        page_count: view.page_count,
        page_has_image: view.page_has_image,
        has_any_image: view.has_any_image,
        slots: view.slots,
        pet_size: settings.pet_window.pet_size.clamp(size_min, size_max),
        size_min,
        size_max,
        always_on_top: settings.pet_window.always_on_top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_slot_maps_to_layout_page() {
        assert_eq!(current_page(PetLayout::Grid, 0), 0);
        assert_eq!(current_page(PetLayout::Grid, 8), 2);
        assert_eq!(current_page(PetLayout::Single, 11), 11);
        assert_eq!(current_page(PetLayout::Row3, 11), 3);
    }

    #[test]
    fn command_directions_follow_frontend_wire_values() {
        assert_eq!(
            serde_json::from_str::<PetPageDirection>("\"next\"").expect("next"),
            PetPageDirection::Next
        );
        assert_eq!(
            serde_json::from_str::<PetResizeDirection>("\"shrink\"").expect("shrink"),
            PetResizeDirection::Shrink
        );
    }
}
