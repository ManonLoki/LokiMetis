mod agent_client;
mod autostart;
mod backend;
mod calls_view;
mod chart_view;
mod combined_view;
mod commands;
mod deep_link;
mod dto;
mod local_view;
mod locale;
mod logging;
mod monitor;
mod notifications;
mod privacy_store;
mod release_notes;
mod runtime;
mod scan_state;
mod settings;
mod source_commands;
mod statistics_view;
mod tray;
mod windowing;

use serde::Serialize;
use tauri::Manager;
use tauri::webview::PageLoadEvent;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;

use autostart::{get_autostart_enabled, set_autostart_enabled};
use commands::{
    add_root_candidate, cancel_root_discovery, clear_local_index, get_local_scan_status,
    get_privacy_settings, get_root_discovery_status, get_source_roots, get_sources,
    get_usage_calls, get_usage_charts, get_usage_overview, get_usage_statistics,
    get_workbuddy_source_status, get_workbuddy_statistics, get_workbuddy_usage_statistics,
    list_root_candidates, refresh_local_indexes, reindex_source_root, set_device_username,
    set_enabled_agents, set_retention_days, set_scan_interval, set_workbuddy_stats_enabled,
    spawn_periodic_local_scans, spawn_retention_cleanup, start_root_discovery,
};
use deep_link::install_deep_link;
use locale::{LocaleState, get_system_locale, resolve_system_locale, set_interface_language};
use logging::install_logging;
pub use monitor::run_hook_relay_if_requested;
use monitor::{
    HookConfigWriter, PET_SETTINGS_LABEL, close_pet_overlay, delete_monitor_image_cmd,
    focus_first_populated_pet_page, get_hook_relay_status, get_monitor_capabilities,
    get_monitor_image_bytes, get_monitor_settings, get_pet_overlay_view, get_pet_window_state,
    hide_pet_settings, list_monitor_hook_locations, list_monitor_images_cmd,
    list_monitor_profile_drafts, load_monitor_settings, pet_overlay_window_description,
    resize_pet_step, save_hook_config_directory, save_monitor_enabled_tools,
    save_monitor_image_cmd, save_monitor_profile_draft, set_pet_always_on_top, set_pet_layout,
    set_pet_locked, set_pet_size, show_main_window, show_or_create_pet_overlay, show_pet_settings,
    spawn_hook_listener, start_pet_overlay_drag, turn_pet_page, write_monitor_hook_config,
};
use notifications::{
    NotificationWorker, get_system_notification_setting, install_notification_worker,
    set_system_notification_enabled,
};
use release_notes::load_release_notes;
use settings::HostSettingsState;
use source_commands::{
    manual_add_source_root, remove_source_root, rename_source_root, set_primary_source_root,
    set_source_root_enabled,
};
use tray::{handle_window, install_tray};
use windowing::{ensure_main_window_is_recoverable, restore_main_window};

rust_i18n::i18n!("locales", fallback = "en-US");

/// 应用名；窗口标题只用它，不带版本号。
const APPLICATION_NAME: &str = "LokiMetis";

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
    AppMetadata {
        application_name: APPLICATION_NAME,
        version: env!("CARGO_PKG_VERSION"),
        product_definition_required: status.product_definition_required,
        title: APPLICATION_NAME.to_owned(),
    }
}

/// 运行唯一 GUI adapter，并统一拥有完整原生生命周期。
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
                .with_denylist(&[pet_overlay_window_description().label, PET_SETTINGS_LABEL])
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
            let app_data_dir = app.path().app_data_dir()?;
            app.manage(runtime::AppRuntimeState::new(app_data_dir));
            spawn_periodic_local_scans(app.handle().clone());
            spawn_retention_cleanup(app.handle().clone());
            install_notification_worker(app.handle());
            let monitor_config_dir = app.path().app_config_dir()?;
            let initial_monitor_settings = load_monitor_settings(&monitor_config_dir);
            let initial_enabled_tools = initial_monitor_settings
                .as_ref()
                .map(|settings| settings.enabled_ai_tools.clone())
                .unwrap_or_default();
            let (hook_relay_status, hook_listener_control) = spawn_hook_listener(
                app.handle().clone(),
                monitor_config_dir.clone(),
                initial_enabled_tools,
            );
            app.manage(hook_relay_status);
            app.manage(hook_listener_control);
            let hook_config_writer = match app.path().home_dir() {
                Ok(hook_home_directory) => {
                    let writer = HookConfigWriter::new(hook_home_directory);
                    match initial_monitor_settings {
                        Ok(settings) => writer.request_enabled(settings),
                        Err(error) => tracing::warn!(
                            code = error.code,
                            "failed to load settings for automatic hook repair"
                        ),
                    }
                    writer
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to resolve Tauri home for automatic hooks");
                    HookConfigWriter::disabled()
                }
            };
            app.manage(hook_config_writer);
            if let Err(error) = show_or_create_pet_overlay(app.handle()) {
                tracing::warn!(%error, "failed to show the default pet overlay");
            }
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
            set_autostart_enabled,
            get_usage_overview,
            get_usage_calls,
            get_usage_statistics,
            get_usage_charts,
            get_sources,
            get_source_roots,
            start_root_discovery,
            get_root_discovery_status,
            list_root_candidates,
            add_root_candidate,
            cancel_root_discovery,
            manual_add_source_root,
            get_local_scan_status,
            refresh_local_indexes,
            reindex_source_root,
            get_privacy_settings,
            set_device_username,
            set_scan_interval,
            set_retention_days,
            set_enabled_agents,
            set_workbuddy_stats_enabled,
            get_workbuddy_statistics,
            get_workbuddy_usage_statistics,
            get_workbuddy_source_status,
            clear_local_index,
            set_source_root_enabled,
            rename_source_root,
            remove_source_root,
            set_primary_source_root,
            get_monitor_capabilities,
            get_monitor_settings,
            save_monitor_enabled_tools,
            save_hook_config_directory,
            list_monitor_hook_locations,
            write_monitor_hook_config,
            get_hook_relay_status,
            list_monitor_images_cmd,
            save_monitor_image_cmd,
            delete_monitor_image_cmd,
            list_monitor_profile_drafts,
            save_monitor_profile_draft,
            get_pet_overlay_view,
            get_pet_window_state,
            get_monitor_image_bytes,
            close_pet_overlay,
            start_pet_overlay_drag,
            show_pet_settings,
            hide_pet_settings,
            set_pet_layout,
            set_pet_size,
            set_pet_always_on_top,
            set_pet_locked,
            turn_pet_page,
            focus_first_populated_pet_page,
            resize_pet_step,
            show_main_window
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
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) && let Some(worker) = app_handle.try_state::<HookConfigWriter>()
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

    #[test]
    fn window_title_is_application_name_without_version() {
        assert_eq!(super::APPLICATION_NAME, "LokiMetis");
        assert!(!super::APPLICATION_NAME.contains(env!("CARGO_PKG_VERSION")));
    }

    /// 冷启动先启用 listener，再通过正常显示路径恢复桌宠位置，最后安装状态一致的托盘。
    #[test]
    fn cold_start_shows_pet_overlay_after_listener_and_before_tray_installation() {
        let source = include_str!("lib.rs");
        let listener = source
            .find("let (hook_relay_status, hook_listener_control) = spawn_hook_listener(")
            .expect("listener setup");
        let hook_home = source
            .find("match app.path().home_dir()")
            .expect("Tauri hook home");
        let hook_writer = source
            .find("HookConfigWriter::new(hook_home_directory)")
            .expect("home-injected hook writer");
        let hook_repair = source
            .find("writer.request_enabled(settings)")
            .expect("automatic hook repair");
        let show = source
            .find("show_or_create_pet_overlay(app.handle())")
            .expect("default pet overlay show");
        let tray = source.find("install_tray(app)?").expect("tray setup");
        assert!(
            listener < hook_home
                && hook_home < hook_writer
                && hook_writer < hook_repair
                && hook_repair < show
                && show < tray
        );

        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");
        let pet = config["app"]["windows"]
            .as_array()
            .expect("windows")
            .iter()
            .find(|window| window["label"] == "pet")
            .expect("pet window");
        assert_eq!(pet["visible"], false);
        assert_eq!(pet["width"], 128);
        assert_eq!(pet["height"], 128);
        assert_eq!(pet["minWidth"], 64);
        assert_eq!(pet["minHeight"], 64);
        assert_eq!(pet["resizable"], true);
        assert_eq!(pet["visibleOnAllWorkspaces"], true);

        let settings = config["app"]["windows"]
            .as_array()
            .expect("windows")
            .iter()
            .find(|window| window["label"] == "pet-settings")
            .expect("pet settings window");
        assert_eq!(settings["visible"], false);
        assert_eq!(settings["width"], 320);
        assert_eq!(settings["height"], 470);
        assert_eq!(settings["minWidth"], 280);
        assert_eq!(settings["minHeight"], 440);
        assert_eq!(settings["resizable"], false);
        assert_eq!(settings["alwaysOnTop"], true);
        assert_eq!(settings["skipTaskbar"], true);
    }

    /// 保存启用工具必须先持久化设置，再把规范化快照交给自动补写 worker。
    #[test]
    fn saving_enabled_tools_queues_best_effort_hook_repair_after_persistence() {
        let source = include_str!("monitor/commands.rs");
        let start = source
            .find("pub fn save_monitor_enabled_tools(")
            .expect("enabled tools command");
        let end = source[start..]
            .find("/// 保存某工具的自定义 Hook 目录。")
            .map(|offset| start + offset)
            .expect("next command boundary");
        let command = &source[start..end];
        let persist = command
            .find("let settings = save_enabled_tools_with_invalid_json_recovery")
            .expect("settings persistence");
        let repair = command
            .find("hook_writer.request_enabled(settings.clone())")
            .expect("automatic repair request");
        let listener = command
            .find("hook_listener.replace_enabled_tools(&settings.enabled_ai_tools)")
            .expect("listener enabled gate update");
        let response = command[listener..]
            .find("Ok(settings)")
            .map(|offset| listener + offset)
            .expect("successful response");
        assert!(persist < repair && repair < listener && listener < response);
    }

    /// 自定义目录保存成功后必须立即替换自动修复快照，不能继续补写旧目录。
    #[test]
    fn saving_hook_directory_refreshes_the_owned_repair_worker_snapshot() {
        let source = include_str!("monitor/commands.rs");
        let start = source
            .find("pub fn save_hook_config_directory(")
            .expect("directory command");
        let end = source[start..]
            .find("/// 列出全部 Agent")
            .map(|offset| start + offset)
            .expect("next command");
        let command = &source[start..end];
        let persist = command
            .find("let settings = update_monitor_settings")
            .expect("settings persistence");
        let repair = command
            .find("hook_writer.request_enabled(settings)")
            .expect("repair snapshot refresh");
        let response = command.find("Ok(location)").expect("successful response");
        assert!(persist < repair && repair < response);
    }
}
