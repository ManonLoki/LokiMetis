mod agent_client;
mod autostart;
mod backend;
mod bundled_resources;
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
mod performance_evidence;
mod privacy_store;
mod release_notes;
mod runtime;
mod scan_state;
mod settings;
mod skins;
mod source_commands;
mod statistics_view;
mod tray;
mod windowing;
#[cfg(test)]
mod windows_manifest_tests;

use loki_metis_core::{HookError, normalize_enabled_ai_tools};
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
    list_root_candidates, migrate_local_indexes, refresh_local_indexes, reindex_source_root,
    set_retention_days, set_scan_interval, spawn_periodic_local_scans, spawn_retention_cleanup,
    start_root_discovery,
};
use deep_link::install_deep_link;
use locale::{LocaleState, get_system_locale, resolve_system_locale, set_interface_language};
use logging::install_logging;
pub use monitor::run_hook_relay_if_requested;
use monitor::{
    HookConfigWriter, HookListenerControl, PET_SETTINGS_LABEL, close_pet_overlay,
    delete_monitor_image_cmd, focus_first_populated_pet_page, get_hook_relay_status,
    get_monitor_capabilities, get_monitor_image_bytes, get_monitor_settings, get_pet_overlay_view,
    get_pet_window_state, hide_pet_settings, install_pet_window_debounce_workers,
    list_monitor_hook_locations, list_monitor_images_cmd, list_monitor_profile_drafts,
    load_monitor_settings, pet_overlay_window_description, resize_pet_step,
    save_enabled_ai_selection, save_hook_config_directory, save_monitor_image_cmd,
    save_monitor_profile_draft, set_pet_always_on_top, set_pet_layout, set_pet_locked,
    set_pet_size, show_main_window, show_or_create_pet_overlay, show_pet_settings,
    spawn_hook_listener, start_pet_overlay_drag, turn_pet_page, update_monitor_settings,
    write_monitor_hook_config,
};
use notifications::{
    NotificationWorker, get_system_notification_setting, install_notification_worker,
    set_system_notification_enabled,
};
use performance_evidence::{
    PerformanceEvidenceState, finish_performance_evidence, get_performance_evidence_status,
    record_performance_evidence,
};
use release_notes::load_release_notes;
use settings::HostSettingsState;
use skins::commands::{
    cancel_codex_operation, cancel_skin_import, commit_skin_import, convert_skin_to_theme,
    create_user_theme, delete_skin, delete_skins, export_skin_package, force_launch_skin_host,
    install_skin, launch_skin_host, list_skin_host_instances, list_skins, open_skin_directory,
    prepare_skin_import, prepare_skin_zip_paths, probe_skin_host_instance,
    restart_skin_host_instance, skin_catalog_changed, skin_creation_prompt,
    skin_host_runtime_status, skin_status, supports_windows_workbuddy_recovery, uninstall_skin,
};
use source_commands::{
    manual_add_source_root, remove_source_root, rename_source_root, set_primary_source_root,
    set_source_root_enabled,
};
use tray::{handle_window, install_tray};
use windowing::{ensure_main_window_is_recoverable, restore_main_window};

rust_i18n::i18n!("locales", fallback = "en-US");

/// 应用名；窗口标题只用它，不带版本号。
const APPLICATION_NAME: &str = "LokiMetis";
/// 开机自启插件在自启进程附加的标记参数，用于区分自启启动与用户手动启动。
const AUTOSTART_LAUNCH_ARG: &str = "--autostart";
/// 只在显式本机性能验收进程中注入，供轻量入口同步判定是否观测。
const PERFORMANCE_EVIDENCE_INITIALIZATION_SCRIPT: &str = "Object.defineProperty(window,'__LOKI_METIS_PERFORMANCE_EVIDENCE__',{configurable:false,enumerable:false,value:true,writable:false});";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// 暴露给前端的稳定应用名称、版本与初始化元数据。
struct AppMetadata {
    application_name: &'static str,
    version: &'static str,
    product_definition_required: bool,
    title: String,
}

#[tauri::command]
/// 返回由编译时版本和核心初始化状态组成的应用元数据。
async fn get_app_metadata() -> AppMetadata {
    let status = loki_metis_core::scaffold_status().await;
    AppMetadata {
        application_name: APPLICATION_NAME,
        version: env!("CARGO_PKG_VERSION"),
        product_definition_required: status.product_definition_required,
        title: APPLICATION_NAME.to_owned(),
    }
}

/// 升级时合并旧看板与 Hooks 选择，并让两份兼容存储在启动监听器前收敛。
async fn reconcile_initial_enabled_ai_selection(
    state: &runtime::AppRuntimeState,
    config_dir: &std::path::Path,
    previous: monitor::MonitorSettings,
) -> Result<monitor::MonitorSettings, HookError> {
    let mut selected = state.enabled_ai_tools_from_dashboard().await;
    selected.extend(previous.enabled_ai_tools.iter().copied());
    let selected = normalize_enabled_ai_tools(&selected);
    let saved = if selected == previous.enabled_ai_tools {
        previous.clone()
    } else {
        update_monitor_settings(config_dir, |settings| {
            settings.enabled_ai_tools = selected.clone();
        })?
    };
    if let Err(detail) = state.set_enabled_ai_tools(&selected).await {
        if saved.enabled_ai_tools != previous.enabled_ai_tools
            && update_monitor_settings(config_dir, |settings| {
                settings.enabled_ai_tools = previous.enabled_ai_tools.clone();
            })
            .is_err()
        {
            tracing::error!("统一 Agent 选择启动迁移失败后无法回滚 Hooks 设置");
        }
        return Err(HookError::new("error.monitor.settingsWriteFailed").param("detail", detail));
    }
    Ok(saved)
}

/// 运行唯一 GUI adapter，并统一拥有完整原生生命周期。
#[rustfmt::skip]
pub fn run() {
    let performance_evidence_state = PerformanceEvidenceState::from_environment()
        .expect("failed to initialize local performance evidence");
    let performance_evidence_enabled = performance_evidence_state.is_enabled();
    let builder = tauri::Builder::default();
    let builder = if performance_evidence_enabled {
        builder.append_invoke_initialization_script(PERFORMANCE_EVIDENCE_INITIALIZATION_SCRIPT)
    } else {
        builder
    };
    let builder = builder
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_LAUNCH_ARG]),
        ))
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished
                && let Err(error) = ensure_main_window_is_recoverable(webview.app_handle())
            {
                tracing::error!(%error, "failed to recover the main window after page load");
            }
        })
        .setup(move |app| {
            // Dock 图标常驻会与托盘常驻语义重复；应用生命周期完全由托盘承载。
            #[cfg(target_os = "macos")]
            if let Err(error) = app.handle().set_dock_visibility(false) {
                tracing::warn!(%error, "failed to hide the dock icon");
            }
            // 自启插件会在系统自启进程上附加该标记参数，用于区分自启与用户手动启动。
            let launched_via_autostart = std::env::args().any(|arg| arg == AUTOSTART_LAUNCH_ARG);
            // 性能证据是显式本机测试通道；路径在构建 WebView 之前已失败关闭校验。
            app.manage(performance_evidence_state);
            install_logging(app)?;
            let settings_path = app.path().app_config_dir()?.join("host-settings.json");
            let settings_state = HostSettingsState::new(settings_path);
            let initial_settings = tauri::async_runtime::block_on(settings_state.read());
            let initial_language = resolve_system_locale(initial_settings.interface_language());
            rust_i18n::set_locale(&initial_language);
            app.manage(settings_state);
            app.manage(LocaleState::new(Some(initial_language)));
            let app_data_dir = app.path().app_data_dir()?;
            let builtin_skins = bundled_resources::resolve_bundled_resource(
                app,
                "builtin-skins",
            )?;
            let skin_service = skins::SkinService::new(builtin_skins, app_data_dir.join("skins"));
            skin_service.initialize()?;
            tracing::info!(
                skin_count = skin_service.list_skins()?.len(),
                "skin catalog initialized"
            );
            app.manage(skin_service);
            let runtime_state = runtime::AppRuntimeState::new(app_data_dir);
            // 所有查询连接都严格只读；必须先在唯一 writer 许可内创建并迁移索引，
            // 再发布 runtime state 或启动任何会读取本机数据库的后台任务。
            tauri::async_runtime::block_on(migrate_local_indexes(&runtime_state))
                .map_err(std::io::Error::other)?;
            let monitor_config_dir = app.path().app_config_dir()?;
            let initial_monitor_settings = load_monitor_settings(&monitor_config_dir)
                .and_then(|settings| tauri::async_runtime::block_on(
                    reconcile_initial_enabled_ai_selection(
                        &runtime_state,
                        &monitor_config_dir,
                        settings,
                    ),
                ));
            tauri::async_runtime::block_on(install_pet_window_debounce_workers(
                app.handle().clone(),
                monitor_config_dir.clone(),
                &runtime_state,
            ))
            .map_err(std::io::Error::other)?;
            app.manage(runtime_state);
            spawn_retention_cleanup(app.handle().clone());
            install_notification_worker(app.handle());
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
            let hook_config_writer = if performance_evidence_enabled {
                HookConfigWriter::disabled()
            } else {
                match app.path().home_dir() {
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
                }
            };
            app.manage(hook_config_writer);
            if let Err(error) = show_or_create_pet_overlay(app.handle()) {
                tracing::warn!(%error, "failed to show the default pet overlay");
            }
            install_tray(app)?;
            spawn_periodic_local_scans(app.handle().clone());
            install_deep_link(app.handle());
            ensure_main_window_is_recoverable(app.handle())?;
            // 开机自启拉起时主窗口默认保持隐藏，只有托盘和桌宠可见；用户手动启动仍照常显示。
            if !launched_via_autostart {
                restore_main_window(app.handle())?;
            }
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
            set_scan_interval,
            set_retention_days,
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
            save_enabled_ai_selection,
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
            show_main_window,
            skin_status,
            list_skins,
            skin_catalog_changed,
            skin_creation_prompt,
            create_user_theme,
            convert_skin_to_theme,
            export_skin_package,
            prepare_skin_import,
            prepare_skin_zip_paths,
            commit_skin_import,
            cancel_skin_import,
            open_skin_directory,
            delete_skin,
            delete_skins,
            skin_host_runtime_status,
            list_skin_host_instances,
            probe_skin_host_instance,
            restart_skin_host_instance,
            launch_skin_host,
            supports_windows_workbuddy_recovery,
            force_launch_skin_host,
            cancel_codex_operation,
            install_skin,
            uninstall_skin,
            get_performance_evidence_status,
            record_performance_evidence,
            finish_performance_evidence
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("failed to build LokiMetis desktop application");
    app.run(|app_handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            // 先让各 owner 在同一异步调度轮中发出取消，再并发等待固定时限。
            let notification_worker = app_handle.try_state::<NotificationWorker>();
            let runtime_state = app_handle.try_state::<runtime::AppRuntimeState>();
            let hook_listener = app_handle.try_state::<HookListenerControl>();
            let skin_service = app_handle.try_state::<skins::SkinService>();
            tauri::async_runtime::block_on(async {
                let notification_shutdown = async {
                    if let Some(worker) = notification_worker.as_ref() {
                        worker.shutdown().await;
                    }
                };
                let background_shutdown = async {
                    if let Some(state) = runtime_state.as_ref() {
                        state.background_tasks.shutdown().await;
                    }
                };
                let scan_shutdown = async {
                    if let Some(state) = runtime_state.as_ref() {
                        state.scan_tasks.shutdown().await;
                    }
                };
                let listener_shutdown = async {
                    if let Some(listener) = hook_listener.as_ref() {
                        listener.shutdown().await;
                    }
                };
                let skin_shutdown = async {
                    if let Some(service) = skin_service.as_ref() {
                        service.shutdown().await;
                    }
                };
                tokio::join!(
                    notification_shutdown,
                    background_shutdown,
                    scan_shutdown,
                    listener_shutdown,
                    skin_shutdown
                );
            });
            if let Some(worker) = app_handle.try_state::<HookConfigWriter>() {
                worker.shutdown();
            }
        }
    });
}

#[cfg(test)]
mod tests;
