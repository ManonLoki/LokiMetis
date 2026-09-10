//! 桌宠原生窗口：创建、显隐、几何持久化与 position-first 读模型。

use std::path::Path;

use loki_metis_core::{
    HookError, PetLayout, PetOverlayPosition, PetOverlayWindowSpec, PetOverlayWorkArea,
    pet_overlay_window_spec, resolve_pet_overlay_position,
};
use serde::Serialize;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::runtime::AppRuntimeState;

use super::pet_geometry::{
    apply_pet_constraints, apply_pet_size, clamp_pet_window_to_work_area, normalize_pet_resize,
    pet_size_range,
};
use super::settings::{
    MonitorSettings, load_monitor_settings, load_pet_runtime_settings, update_monitor_settings,
};

/// 桌宠设置窗口固定标签。
pub const PET_SETTINGS_LABEL: &str = "pet-settings";
/// 首次定位时离主显示器工作区右下边缘的逻辑像素。
const PET_DEFAULT_INSET: f64 = 16.0;
/// 仅当布局与单格大小仍匹配事件快照时写入新大小，避免旧窗口事件覆盖新设置。
pub(super) fn replace_pet_size_if_unchanged(
    settings: &mut MonitorSettings,
    expected_layout: PetLayout,
    expected_size: u16,
    applied_size: u16,
) -> bool {
    if settings.pet_window.layout != expected_layout
        || settings.pet_window.pet_size != expected_size
    {
        return false;
    }
    settings.pet_window.pet_size = applied_size;
    true
}

/// 已交付的悬浮窗描述，命令与测试共用。
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
    /// 是否默认始终置顶。
    pub always_on_top: bool,
    /// 是否跳过任务栏。
    pub skip_taskbar: bool,
    /// 是否允许用户调整窗口大小。
    pub resizable: bool,
    /// 是否在所有工作区可见。
    pub visible_on_all_workspaces: bool,
    /// 是否绘制原生阴影。
    pub shadow: bool,
    /// 是否隐藏主窗口。
    pub hide_main_window: bool,
    /// 是否停止 Hook listener。
    pub stop_hook_listener: bool,
    /// 默认宽度。
    pub default_width: f64,
    /// 默认高度。
    pub default_height: f64,
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
        resizable: spec.resizable,
        visible_on_all_workspaces: true,
        shadow: false,
        hide_main_window: spec.hide_main_window,
        stop_hook_listener: spec.stop_hook_listener,
        default_width: spec.default_width,
        default_height: spec.default_height,
    }
}

/// 该窗口标签是否属于桌宠悬浮窗。
pub fn is_pet_overlay_label(window_label: &str) -> bool {
    window_label == pet_overlay_window_description().label
}

/// 该窗口标签是否属于桌宠设置窗。
pub fn is_pet_settings_label(window_label: &str) -> bool {
    window_label == PET_SETTINGS_LABEL
}

/// 把 pet 窗口的 Moved 事件转成待持久化快照；其它窗口或无效尺寸忽略。
pub fn pet_overlay_move_snapshot(
    window_label: &str,
    event_position: Option<PetOverlayPosition>,
    outer_size: (u32, u32),
) -> Option<(PetOverlayPosition, (u32, u32))> {
    if !is_pet_overlay_label(window_label) {
        return None;
    }
    let position = event_position?;
    if outer_size.0 == 0 || outer_size.1 == 0 {
        return None;
    }
    Some((position, outer_size))
}

/// 从宿主显示器读取工作区，供位置规范化使用。
pub fn pet_overlay_work_areas<R: Runtime>(app: &AppHandle<R>) -> Vec<PetOverlayWorkArea> {
    let monitors = app
        .get_webview_window(pet_overlay_window_description().label)
        .and_then(|window| window.available_monitors().ok())
        .or_else(|| app.available_monitors().ok())
        .unwrap_or_default();
    monitors
        .into_iter()
        .map(|monitor| {
            let area = monitor.work_area();
            PetOverlayWorkArea {
                x: area.position.x,
                y: area.position.y,
                width: area.size.width,
                height: area.size.height,
            }
        })
        .collect()
}

/// 返回桌宠当前有效外框物理尺寸。
fn live_pet_overlay_outer_size<R: Runtime>(window: &WebviewWindow<R>) -> Option<(u32, u32)> {
    window
        .outer_size()
        .ok()
        .filter(|size| size.width > 0 && size.height > 0)
        .map(|size| (size.width, size.height))
}

/// 首次启动时把桌宠放到主显示器工作区右下角并保留 16 逻辑像素。
fn default_pet_overlay_position<R: Runtime>(
    app: &AppHandle<R>,
    overlay_size: (u32, u32),
) -> Option<PetOverlayPosition> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let area = monitor.work_area();
    let inset = (PET_DEFAULT_INSET * monitor.scale_factor()).round() as i64;
    let x =
        i64::from(area.position.x) + i64::from(area.size.width) - i64::from(overlay_size.0) - inset;
    let y = i64::from(area.position.y) + i64::from(area.size.height)
        - i64::from(overlay_size.1)
        - inset;
    Some(PetOverlayPosition {
        x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
    })
}

/// 应用已保存位置；缺失或离屏时使用主屏右下角默认位置。
fn apply_saved_pet_overlay_position<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let Some(overlay_size) = live_pet_overlay_outer_size(window) else {
        return;
    };
    let stored = load_monitor_settings(&config_dir)
        .ok()
        .and_then(|settings| settings.pet_overlay_position);
    let work_areas = pet_overlay_work_areas(app);
    let position = resolve_pet_overlay_position(stored, overlay_size, &work_areas)
        .or_else(|| default_pet_overlay_position(app, overlay_size));
    if let Some(position) = position {
        let _ = window.set_position(PhysicalPosition::new(position.x, position.y));
    }
}

/// 把布局、大小、置顶、锁定与显示器约束同步到原生桌宠窗口。
fn apply_pet_window_preferences<R: Runtime>(
    window: &WebviewWindow<R>,
    settings: &super::settings::MonitorSettings,
) -> Result<u16, String> {
    window
        .set_always_on_top(settings.pet_window.always_on_top)
        .map_err(|error| error.to_string())?;
    window
        .set_resizable(!settings.pet_window.locked)
        .map_err(|error| error.to_string())?;
    let _ = window.set_visible_on_all_workspaces(true);
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_shadow(false);
    let applied = apply_pet_size(
        window,
        settings.pet_window.layout,
        settings.pet_window.pet_size,
    )?;
    apply_pet_constraints(window, settings.pet_window.layout).map_err(|error| error.to_string())?;
    Ok(applied)
}

/// 打开或聚焦桌宠悬浮窗；主窗口与 listener 始终保持原状。
pub fn show_or_create_pet_overlay(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    if description.hide_main_window || description.stop_hook_listener {
        return Err("pet overlay must coexist with the main window and hook listener".to_owned());
    }
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| error.to_string())?;
    let settings = load_pet_runtime_settings(&config_dir);
    let window = if let Some(existing) = app.get_webview_window(description.label) {
        existing
    } else {
        WebviewWindowBuilder::new(
            app,
            description.label,
            WebviewUrl::App("index.html?view=pet".into()),
        )
        .title("LokiMetis 桌宠")
        .inner_size(description.default_width, description.default_height)
        .min_inner_size(64.0, 64.0)
        .decorations(description.decorations)
        .transparent(description.transparent)
        .always_on_top(settings.pet_window.always_on_top)
        .visible_on_all_workspaces(description.visible_on_all_workspaces)
        .skip_taskbar(description.skip_taskbar)
        .shadow(description.shadow)
        .resizable(description.resizable && !settings.pet_window.locked)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?
    };
    let applied_size = apply_pet_window_preferences(&window, &settings)?;
    if applied_size != settings.pet_window.pet_size {
        let mut changed = false;
        match update_monitor_settings(&config_dir, |current| {
            changed = replace_pet_size_if_unchanged(
                current,
                settings.pet_window.layout,
                settings.pet_window.pet_size,
                applied_size,
            );
        }) {
            Ok(_) if changed => super::pet_events::emit_pet_window_state_changed(app),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "failed to persist the clamped pet size"),
        }
    }
    apply_saved_pet_overlay_position(app, &window);
    clamp_pet_window_to_work_area(&window);
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    Ok(description)
}

/// 显示独立桌宠设置窗，并在每次打开时居中到桌宠当前显示器。
pub fn show_pet_settings_window(app: &AppHandle) -> Result<(), String> {
    let pet = app
        .get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    let monitor = pet
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or_else(|| pet.primary_monitor().ok().flatten())
        .ok_or_else(|| "unable to determine the pet monitor".to_owned())?;
    let settings = if let Some(existing) = app.get_webview_window(PET_SETTINGS_LABEL) {
        existing
    } else {
        WebviewWindowBuilder::new(
            app,
            PET_SETTINGS_LABEL,
            WebviewUrl::App("index.html?view=pet-settings".into()),
        )
        .title("LokiMetis 桌宠设置")
        .inner_size(320.0, 470.0)
        .min_inner_size(280.0, 440.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?
    };
    let area = monitor.work_area();
    let size = settings.outer_size().map_err(|error| error.to_string())?;
    let x = i64::from(area.position.x)
        + (i64::from(area.size.width).saturating_sub(i64::from(size.width)) / 2);
    let y = i64::from(area.position.y)
        + (i64::from(area.size.height).saturating_sub(i64::from(size.height)) / 2);
    settings
        .set_position(PhysicalPosition::new(x as i32, y as i32))
        .map_err(|error| error.to_string())?;
    settings.show().map_err(|error| error.to_string())?;
    settings.set_focus().map_err(|error| error.to_string())
}

/// 关闭并销毁桌宠设置窗；保留旧函数名以兼容既有 IPC 映射。
pub fn hide_pet_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(PET_SETTINGS_LABEL) {
        window.destroy().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 把宿主读回的物理位置与实际 outer_size 交给 core 规范化后写入本机设置。
pub fn persist_pet_overlay_position(
    config_dir: &Path,
    position: PetOverlayPosition,
    overlay_size: (u32, u32),
    work_areas: &[PetOverlayWorkArea],
) -> Result<Option<PetOverlayPosition>, HookError> {
    let resolved = resolve_pet_overlay_position(Some(position), overlay_size, work_areas);
    let saved = update_monitor_settings(config_dir, |settings| {
        settings.pet_overlay_position = resolved;
    })?;
    Ok(saved.pet_overlay_position)
}

/// 原生 `WindowEvent::Moved` 入口：按实际物理尺寸规范化后防抖写入。
pub fn schedule_pet_overlay_position_persist<R: Runtime>(
    window: &tauri::Window<R>,
    event_position: PetOverlayPosition,
) {
    if !is_pet_overlay_label(window.label()) {
        return;
    }
    let outer_size = window
        .outer_size()
        .map(|size| (size.width, size.height))
        .unwrap_or((0, 0));
    let Some((position, outer_size)) =
        pet_overlay_move_snapshot(window.label(), Some(event_position), outer_size)
    else {
        return;
    };
    let app = window.app_handle().clone();
    let _ = app
        .state::<AppRuntimeState>()
        .pet_move_debounce
        .submit(position, outer_size);
}

/// 原生 `WindowEvent::Resized` 入口：立即修正宽高比，再防抖保存单格大小。
pub fn handle_pet_overlay_resized<R: Runtime>(window: &tauri::Window<R>, size: PhysicalSize<u32>) {
    if !is_pet_overlay_label(window.label()) {
        return;
    }
    let app = window.app_handle().clone();
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let settings = load_pet_runtime_settings(&config_dir);
    if settings.pet_window.locked {
        return;
    }
    let Some(webview) = app.get_webview_window(pet_overlay_window_description().label) else {
        return;
    };
    let pet_size = normalize_pet_resize(
        &webview,
        settings.pet_window.layout,
        size,
        settings.pet_window.pet_size,
    );
    clamp_pet_window_to_work_area(&webview);
    let layout = settings.pet_window.layout;
    let _ = app.state::<AppRuntimeState>().pet_resize_debounce.submit(
        layout,
        settings.pet_window.pet_size,
        pet_size,
    );
}

/// 跨显示器移动后重算约束，并在小屏上收敛当前大小。
pub fn constrain_pet_overlay_to_current_monitor<R: Runtime>(window: &tauri::Window<R>) {
    if !is_pet_overlay_label(window.label()) {
        return;
    }
    let app = window.app_handle();
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let settings = load_pet_runtime_settings(&config_dir);
    let Some(webview) = app.get_webview_window(pet_overlay_window_description().label) else {
        return;
    };
    if let Ok(size) = apply_pet_size(
        &webview,
        settings.pet_window.layout,
        settings.pet_window.pet_size,
    ) && size != settings.pet_window.pet_size
    {
        let expected_layout = settings.pet_window.layout;
        let expected_size = settings.pet_window.pet_size;
        let _ = update_monitor_settings(&config_dir, |current| {
            let _ = replace_pet_size_if_unchanged(current, expected_layout, expected_size, size);
        });
    }
    super::pet_events::emit_pet_window_state_changed(app);
}

/// 查询桌宠悬浮窗当前是否可见；窗口不存在视为关闭。
pub fn pet_overlay_window_is_open(app: &AppHandle) -> bool {
    app.get_webview_window(pet_overlay_window_description().label)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// 隐藏桌宠和设置窗，主窗口保持原状。
pub fn close_pet_overlay_window(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    hide_pet_settings_window(app)?;
    if let Some(window) = app.get_webview_window(description.label) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(description)
}

/// 开始拖动无边框桌宠；锁定时静默忽略。
pub fn start_pet_overlay_dragging(app: &AppHandle) -> Result<(), String> {
    let settings = load_pet_runtime_settings(
        &app.path()
            .app_config_dir()
            .map_err(|error| error.to_string())?,
    );
    if settings.pet_window.locked {
        return Ok(());
    }
    app.get_webview_window(pet_overlay_window_description().label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?
        .start_dragging()
        .map_err(|error| error.to_string())
}

/// 查询原生窗口当前显示器允许的单格尺寸区间。
pub fn current_pet_size_range(app: &AppHandle, layout: PetLayout) -> (u16, u16) {
    app.get_webview_window(pet_overlay_window_description().label)
        .map(|window| pet_size_range(&window, layout))
        .unwrap_or_else(|| super::pet_geometry::pet_size_range_fallback(layout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::PET_OVERLAY_WINDOW_SPEC;

    /// 桌宠原生窗口描述应符合 AI Monitor 宿主契约。
    #[test]
    fn pet_overlay_window_description_matches_aimonitor_host_contract() {
        let description = pet_overlay_window_description();
        assert_eq!(description.label, "pet");
        assert_eq!(description.url_path, "/pet");
        assert!(!description.decorations);
        assert!(description.transparent);
        assert!(description.always_on_top);
        assert!(description.resizable);
        assert!(description.visible_on_all_workspaces);
        assert!(description.skip_taskbar);
        assert!(!description.shadow);
        assert!(!description.hide_main_window);
        assert!(!description.stop_hook_listener);
        assert_eq!(description.default_width, 128.0);
        assert_eq!(description.default_height, 128.0);
        assert_eq!(description.resizable, PET_OVERLAY_WINDOW_SPEC.resizable);
    }

    /// 移动事件应过滤非桌宠窗口与零尺寸快照。
    #[test]
    fn moved_event_filters_non_pet_and_zero_sized_windows() {
        let position = PetOverlayPosition { x: 80, y: 80 };
        assert_eq!(
            pet_overlay_move_snapshot("main", Some(position), (128, 128)),
            None
        );
        assert_eq!(
            pet_overlay_move_snapshot("pet", Some(position), (0, 128)),
            None
        );
        assert_eq!(
            pet_overlay_move_snapshot("pet", Some(position), (128, 128)),
            Some((position, (128, 128)))
        );
    }

    /// 迟到的缩放快照不得覆盖已经改变的布局或单格大小。
    #[test]
    fn stale_pet_size_snapshot_is_rejected() {
        let mut settings = MonitorSettings::default();
        assert!(!replace_pet_size_if_unchanged(
            &mut settings,
            PetLayout::Row,
            64,
            96,
        ));
        settings.pet_window.pet_size = 80;
        assert!(!replace_pet_size_if_unchanged(
            &mut settings,
            PetLayout::Grid,
            64,
            96,
        ));
        assert_eq!(settings.pet_window.pet_size, 80);
        assert!(replace_pet_size_if_unchanged(
            &mut settings,
            PetLayout::Grid,
            80,
            96,
        ));
        assert_eq!(settings.pet_window.pet_size, 96);
    }

    /// 设置窗关闭必须销毁旧 WebView，后续打开仍保留按需创建分支。
    #[test]
    fn pet_settings_close_destroys_webview_and_reopen_can_recreate_it() {
        let source = include_str!("pet_window.rs");
        let show_start = source
            .find("pub fn show_pet_settings_window(")
            .expect("pet settings show function");
        let close_start = source
            .find("pub fn hide_pet_settings_window(")
            .expect("pet settings close function");
        let show = &source[show_start..close_start];
        let close_end = source[close_start..]
            .find("/// 把宿主读回的物理位置")
            .map(|offset| close_start + offset)
            .expect("next function boundary");
        let close = &source[close_start..close_end];

        assert!(show.contains("app.get_webview_window(PET_SETTINGS_LABEL)"));
        assert!(show.contains("WebviewWindowBuilder::new("));
        assert!(show.contains(".build()"));
        assert!(close.contains("window.destroy()"));
        assert!(!close.contains("window.hide()"));
    }
}
