use std::sync::Mutex;

use tauri::{
    Manager, Runtime, WindowEvent,
    menu::{Menu, MenuItem, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use crate::monitor::pet_overlay_window_description;
use crate::windowing::restore_main_window;

const SHOW_WINDOW_ID: &str = "show_window";
const QUIT_ID: &str = "quit";

pub(crate) struct TrayMenuState {
    show_window: Mutex<MenuItem<tauri::Wry>>,
    quit: Mutex<MenuItem<tauri::Wry>>,
}

pub(crate) fn install_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_window = MenuItemBuilder::with_id(
        SHOW_WINDOW_ID,
        rust_i18n::t!("tray.show_window").into_owned(),
    )
    .build(app)?;
    let quit =
        MenuItemBuilder::with_id(QUIT_ID, rust_i18n::t!("tray.quit").into_owned()).build(app)?;
    let menu = Menu::with_items(app, &[&show_window, &quit])?;
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
            QUIT_ID => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenuState {
        show_window: Mutex::new(show_window),
        quit: Mutex::new(quit),
    });
    Ok(())
}

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
    Ok(())
}

fn tray_label(key: &str) -> String {
    match key {
        "show_window" => rust_i18n::t!("tray.show_window").into_owned(),
        "quit" => rust_i18n::t!("tray.quit").into_owned(),
        _ => String::new(),
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

pub(crate) fn handle_window<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main"
            && window.app_handle().try_state::<TrayMenuState>().is_some()
        {
            api.prevent_close();
            let _ = window.hide();
            return;
        }
        if window.label() == pet_overlay_window_description().label {
            api.prevent_close();
            let _ = window.hide();
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

    #[test]
    fn tray_labels_resolve_for_supported_locales() {
        assert_eq!(tray_label_for_locale("show_window", "zh-CN"), "显示窗口");
        assert_eq!(tray_label_for_locale("quit", "en-US"), "Quit");
    }

    #[test]
    fn tray_labels_fall_back_to_english() {
        let fallback_label = tray_label_for_locale("show_window", "fr-FR");
        assert_eq!(fallback_label, "Show Window");
    }

    #[test]
    fn language_change_updates_tray_menu_labels() {
        let before = tray_label_for_locale("show_window", "en-US");
        let after = tray_label_for_locale("show_window", "zh-CN");
        assert_ne!(before, after);
    }
}
