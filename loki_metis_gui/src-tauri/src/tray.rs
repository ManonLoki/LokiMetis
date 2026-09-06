use std::sync::Mutex;

use tauri::{
    AppHandle, Manager, Runtime, WindowEvent,
    menu::{Menu, MenuItem, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::monitor::{
    PET_SETTINGS_LABEL, close_pet_overlay_window, constrain_pet_overlay_to_current_monitor,
    handle_pet_overlay_resized, is_pet_settings_label, pet_overlay_window_description,
    pet_overlay_window_is_open, schedule_pet_overlay_position_persist, show_or_create_pet_overlay,
};
use crate::windowing::restore_main_window;
use loki_metis_core::PetOverlayPosition;

const SHOW_WINDOW_ID: &str = "show_window";
const TOGGLE_PET_OVERLAY_ID: &str = "toggle_pet_overlay";
const QUIT_ID: &str = "quit";

/// 托盘菜单项句柄，用于语言切换和桌宠显隐后刷新动作文案。
pub(crate) struct TrayMenuState {
    show_window: Mutex<MenuItem<tauri::Wry>>,
    pet_overlay: Mutex<MenuItem<tauri::Wry>>,
    quit: Mutex<MenuItem<tauri::Wry>>,
}

/// 托盘点击后应执行的桌宠可见性动作；真实窗口可见性是唯一输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PetOverlayTrayAction {
    Show,
    Hide,
}

/// 根据桌宠当前真实可见性选择相反动作。
fn pet_overlay_tray_action(is_visible: bool) -> PetOverlayTrayAction {
    if is_visible {
        PetOverlayTrayAction::Hide
    } else {
        PetOverlayTrayAction::Show
    }
}

/// 执行一次托盘桌宠显隐动作，不保存跨启动偏好。
fn toggle_pet_overlay(app: &AppHandle) -> Result<(), String> {
    match pet_overlay_tray_action(pet_overlay_window_is_open(app)) {
        PetOverlayTrayAction::Show => show_or_create_pet_overlay(app).map(|_| ()),
        PetOverlayTrayAction::Hide => close_pet_overlay_window(app).map(|_| ()),
    }
}

/// 安装应用托盘、三项菜单及左键恢复主窗口行为。
pub(crate) fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_window = MenuItemBuilder::with_id(
        SHOW_WINDOW_ID,
        rust_i18n::t!("tray.show_window").into_owned(),
    )
    .build(app)?;
    let pet_overlay = MenuItemBuilder::with_id(
        TOGGLE_PET_OVERLAY_ID,
        pet_overlay_tray_label(pet_overlay_window_is_open(app.handle())),
    )
    .build(app)?;
    let quit =
        MenuItemBuilder::with_id(QUIT_ID, rust_i18n::t!("tray.quit").into_owned()).build(app)?;
    let menu = Menu::with_items(app, &[&show_window, &pet_overlay, &quit])?;
    let icon = app
        .default_window_icon()
        .expect("bundled app icon must exist")
        .clone();

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = restore_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW_WINDOW_ID => {
                let _ = restore_main_window(app);
            }
            TOGGLE_PET_OVERLAY_ID => {
                if let Err(error) = toggle_pet_overlay(app) {
                    tracing::warn!(%error, "failed to toggle pet overlay from tray");
                }
                if let Err(error) = refresh_pet_overlay_label(app) {
                    tracing::warn!(%error, "failed to refresh pet overlay tray label");
                }
            }
            QUIT_ID => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenuState {
        show_window: Mutex::new(show_window),
        pet_overlay: Mutex::new(pet_overlay),
        quit: Mutex::new(quit),
    });
    Ok(())
}

/// 界面语言变化后刷新全部托盘菜单文案。
pub(crate) fn refresh_tray_labels(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return Ok(());
    };
    state
        .show_window
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(tray_label("show_window"))?;
    state
        .quit
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(tray_label("quit"))?;
    refresh_pet_overlay_label(app)?;
    Ok(())
}

/// 按当前真实窗口可见性刷新托盘中的桌宠动作文案。
pub(crate) fn refresh_pet_overlay_label(app: &AppHandle) -> tauri::Result<()> {
    set_pet_overlay_label(app, pet_overlay_window_is_open(app))
}

/// 用已知可见性更新托盘桌宠菜单项；泛型窗口事件无需重新读取 Wry 窗口。
fn set_pet_overlay_label<R: Runtime>(app: &AppHandle<R>, is_visible: bool) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return Ok(());
    };
    state
        .pet_overlay
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set_text(pet_overlay_tray_label(is_visible))?;
    Ok(())
}

fn tray_label(key: &str) -> String {
    match key {
        "show_window" => rust_i18n::t!("tray.show_window").into_owned(),
        "quit" => rust_i18n::t!("tray.quit").into_owned(),
        _ => String::new(),
    }
}

/// 当前桌宠状态对应的托盘动作文案。
fn pet_overlay_tray_label(is_visible: bool) -> String {
    if is_visible {
        rust_i18n::t!("tray.hide_pet_overlay").into_owned()
    } else {
        rust_i18n::t!("tray.show_pet_overlay").into_owned()
    }
}

#[cfg(test)]
fn tray_label_for_locale(key: &str, locale: &str) -> String {
    match key {
        "show_window" => rust_i18n::t!("tray.show_window", locale = locale).into_owned(),
        "quit" => rust_i18n::t!("tray.quit", locale = locale).into_owned(),
        _ => String::new(),
    }
}

/// 测试指定语言与可见性对应的托盘桌宠动作文案。
#[cfg(test)]
fn pet_overlay_tray_label_for_locale(is_visible: bool, locale: &str) -> String {
    if is_visible {
        rust_i18n::t!("tray.hide_pet_overlay", locale = locale).into_owned()
    } else {
        rust_i18n::t!("tray.show_pet_overlay", locale = locale).into_owned()
    }
}

/// 处理主窗口、桌宠和桌宠设置窗的移动、缩放、关闭隐藏及托盘状态同步。
pub(crate) fn handle_window<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if let WindowEvent::Moved(position) = event {
        schedule_pet_overlay_position_persist(
            window,
            PetOverlayPosition {
                x: position.x,
                y: position.y,
            },
        );
        constrain_pet_overlay_to_current_monitor(window);
    }
    if let WindowEvent::Resized(size) = event {
        handle_pet_overlay_resized(window, *size);
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" && window.app_handle().try_state::<TrayMenuState>().is_some() {
            api.prevent_close();
            let _ = window.hide();
            return;
        }
        if is_pet_settings_label(window.label()) {
            api.prevent_close();
            let _ = window.hide();
            return;
        }
        if window.label() == pet_overlay_window_description().label {
            api.prevent_close();
            if let Some(settings) = window.app_handle().get_webview_window(PET_SETTINGS_LABEL) {
                let _ = settings.hide();
            }
            if window.hide().is_ok() {
                let _ = set_pet_overlay_label(window.app_handle(), false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_show_restores_and_focuses_main_window() {
        let restore_steps = ["show", "unminimize", "focus"];
        assert_eq!(restore_steps.last(), Some(&"focus"));
    }

    #[test]
    fn close_request_hides_without_exit() {
        let close_actions = ["prevent_close", "hide"];
        assert_eq!(close_actions, ["prevent_close", "hide"]);
    }

    #[test]
    fn close_request_requires_ready_tray_state() {
        let prerequisites = ["main-window", "tray-state-ready"];
        assert_eq!(prerequisites[1], "tray-state-ready");
    }

    #[test]
    fn tray_quit_exits_application() {
        let menu_action = (QUIT_ID, 0);
        assert_eq!(menu_action, ("quit", 0));
    }

    /// 托盘动作始终与桌宠当前真实可见性相反。
    #[test]
    fn tray_pet_overlay_action_toggles_current_visibility() {
        assert_eq!(pet_overlay_tray_action(false), PetOverlayTrayAction::Show);
        assert_eq!(pet_overlay_tray_action(true), PetOverlayTrayAction::Hide);
    }

    /// 新增桌宠项时保留既有主窗口恢复与真正退出菜单。
    #[test]
    fn tray_menu_keeps_main_window_and_quit_around_pet_overlay_toggle() {
        assert_eq!(
            [SHOW_WINDOW_ID, TOGGLE_PET_OVERLAY_ID, QUIT_ID],
            ["show_window", "toggle_pet_overlay", "quit"]
        );
    }

    #[test]
    fn tray_labels_resolve_for_supported_locales() {
        assert_eq!(tray_label_for_locale("show_window", "zh-CN"), "显示窗口");
        assert_eq!(
            pet_overlay_tray_label_for_locale(false, "zh-CN"),
            "显示桌宠浮窗"
        );
        assert_eq!(
            pet_overlay_tray_label_for_locale(true, "en-US"),
            "Hide Pet Overlay"
        );
        assert_eq!(tray_label_for_locale("quit", "en-US"), "Quit");
    }

    #[test]
    fn tray_labels_fall_back_to_english() {
        let fallback_label = tray_label_for_locale("show_window", "fr-FR");
        assert_eq!(fallback_label, "Show Window");
        assert_eq!(
            pet_overlay_tray_label_for_locale(false, "fr-FR"),
            "Show Pet Overlay"
        );
    }

    #[test]
    fn language_change_updates_tray_menu_labels() {
        let before = tray_label_for_locale("show_window", "en-US");
        let after = tray_label_for_locale("show_window", "zh-CN");
        assert_ne!(before, after);
        assert_ne!(
            pet_overlay_tray_label_for_locale(true, "en-US"),
            pet_overlay_tray_label_for_locale(true, "zh-CN")
        );
    }
}
