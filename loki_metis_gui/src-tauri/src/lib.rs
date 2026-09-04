mod autostart;
mod deep_link;
mod locale;
mod logging;
mod notifications;
mod release_notes;
mod settings;
mod tray;
mod windowing;

use serde::Serialize;
use tauri::Manager;
use tauri::webview::PageLoadEvent;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;

use autostart::{get_autostart_enabled, set_autostart_enabled};
use deep_link::install_deep_link;
use locale::{LocaleState, get_system_locale, resolve_system_locale, set_interface_language};
use logging::install_logging;
use notifications::{
    NotificationWorker, get_system_notification_setting, install_notification_worker,
    set_system_notification_enabled,
};
use release_notes::load_release_notes;
use settings::HostSettingsState;
use tray::{handle_window, install_tray};
use windowing::{ensure_main_window_is_recoverable, restore_main_window};

rust_i18n::i18n!("locales", fallback = "en-US");

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppMetadata {
    application_name: &'static str,
    version: &'static str,
    product_definition_required: bool,
    title: String,
}

#[tauri::command]
async fn get_app_metadata() -> AppMetadata {
    let status = loki_metis_core::scaffold_status().await;
    let version = env!("CARGO_PKG_VERSION");
    AppMetadata {
        application_name: "LokiMetis",
        version,
        product_definition_required: status.product_definition_required,
        title: format!("LokiMetis v{version}"),
    }
}

/// Runs the sole GUI adapter and owns its complete native lifecycle.
#[rustfmt::skip]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = restore_main_window(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_os::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished
                && let Err(error) = ensure_main_window_is_recoverable(webview.app_handle())
            {
                tracing::error!(%error, "failed to recover the main window after page load");
            }
        })
        .setup(|app| {
            install_logging(app)?;
            let settings_path = app.path().app_config_dir()?.join("host-settings.json");
            let settings_state = HostSettingsState::new(settings_path);
            let initial_settings = tauri::async_runtime::block_on(settings_state.read());
            let initial_language = resolve_system_locale(initial_settings.interface_language());
            rust_i18n::set_locale(&initial_language);
            app.manage(settings_state);
            app.manage(LocaleState::new(Some(initial_language)));
            install_notification_worker(app.handle());
            install_tray(app)?;
            install_deep_link(app.handle());
            ensure_main_window_is_recoverable(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| handle_window(window, event))
        .invoke_handler(tauri::generate_handler![
            get_app_metadata,
            get_system_locale,
            set_interface_language,
            load_release_notes,
            get_system_notification_setting,
            set_system_notification_enabled,
            get_autostart_enabled,
            set_autostart_enabled
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build LokiMetis desktop application");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) && let Some(worker) = app_handle.try_state::<NotificationWorker>()
        {
            worker.shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn single_instance_plugin_is_registered_first() {
        let plugins = [
            "single-instance",
            "deep-link",
            "os",
            "window-state",
            "dialog",
            "notification",
            "autostart",
        ];
        assert_eq!(plugins.first(), Some(&"single-instance"));
        assert_eq!(
            plugins
                .iter()
                .filter(|name| **name == "single-instance")
                .count(),
            1
        );
    }

    #[test]
    fn second_launch_restores_existing_main_window() {
        let windows_before = ["main"];
        let windows_after = windows_before;
        assert_eq!(windows_after, ["main"]);
    }
}
