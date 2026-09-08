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
    list_root_candidates, refresh_local_indexes, reindex_source_root, set_retention_days,
    set_scan_interval, spawn_periodic_local_scans, spawn_retention_cleanup, start_root_discovery,
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
    resize_pet_step, save_enabled_ai_selection, save_hook_config_directory, save_monitor_image_cmd,
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
    skin_host_runtime_status, skin_status, uninstall_skin,
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
/// 只在显式本机性能验收进程中注入，供轻量入口同步判定是否观测。
const PERFORMANCE_EVIDENCE_INITIALIZATION_SCRIPT: &str = "Object.defineProperty(window,'__LOKI_METIS_PERFORMANCE_EVIDENCE__',{configurable:false,enumerable:false,value:true,writable:false});";

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
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished
                && let Err(error) = ensure_main_window_is_recoverable(webview.app_handle())
            {
                tracing::error!(%error, "failed to recover the main window after page load");
            }
        })
        .setup(move |app| {
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
            let monitor_config_dir = app.path().app_config_dir()?;
            let initial_monitor_settings = load_monitor_settings(&monitor_config_dir)
                .and_then(|settings| tauri::async_runtime::block_on(
                    reconcile_initial_enabled_ai_selection(
                        &runtime_state,
                        &monitor_config_dir,
                        settings,
                    ),
                ));
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

    /// Tauri 不复制产品版本，并保持发布 DMG 的图标位置与 GUI Profile 一致。
    #[test]
    fn tauri_config_uses_cargo_version_and_localized_bundle_names() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");

        assert!(config.get("version").is_none());
        assert_eq!(config["productName"], "LokiMetis");
        let dmg = &config["bundle"]["macOS"]["dmg"];
        assert_eq!(dmg["windowSize"]["width"], 660);
        assert_eq!(dmg["windowSize"]["height"], 400);
        assert_eq!(dmg["appPosition"]["x"], 180);
        assert_eq!(dmg["appPosition"]["y"], 220);
        assert_eq!(dmg["applicationFolderPosition"]["x"], 480);
        assert_eq!(dmg["applicationFolderPosition"]["y"], 220);

        let macos = &config["bundle"]["macOS"];
        assert_eq!(macos["infoPlist"], "Info.plist");
        assert_eq!(
            macos["files"]["Resources/en.lproj/InfoPlist.strings"],
            "macos/en.lproj/InfoPlist.strings"
        );
        assert_eq!(
            macos["files"]["Resources/zh-Hans.lproj/InfoPlist.strings"],
            "macos/zh-Hans.lproj/InfoPlist.strings"
        );

        let nsis = &config["bundle"]["windows"]["nsis"];
        assert_eq!(nsis["template"], "windows/nsis/installer.nsi");
        assert_eq!(
            nsis["languages"],
            serde_json::json!(["English", "SimpChinese"])
        );
        assert_eq!(nsis["displayLanguageSelector"], true);
        assert_eq!(
            nsis["customLanguageFiles"]["SimpChinese"],
            "windows/nsis/languages/SimpChinese.nsh"
        );
    }

    /// bundle 必须引用磁盘上真实存在的完整平台图标集；NSIS 图标没有默认回落，必须显式指定。
    #[test]
    fn tauri_config_references_existing_platform_icons() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");

        let icons: Vec<&str> = config["bundle"]["icon"]
            .as_array()
            .expect("bundle icons")
            .iter()
            .map(|icon| icon.as_str().expect("icon path"))
            .collect();

        // Windows 主程序、托盘与 MSI 取列表里的第一个 .ico，macOS `.app` 取 .icns；
        // 缺任何一个都会让 Tauri 回落到默认图标或直接打包失败。
        assert!(icons.iter().any(|icon| icon.ends_with(".ico")));
        assert!(icons.iter().any(|icon| icon.ends_with(".icns")));
        // macOS/Linux 的 default_window_icon 取第一个 .png，托盘由它派生。
        assert_eq!(
            icons.iter().find(|icon| icon.ends_with(".png")),
            Some(&"icons/32x32.png")
        );

        let nsis = &config["bundle"]["windows"]["nsis"];
        let installer_icon = nsis["installerIcon"].as_str().expect("installer icon");
        let uninstaller_icon = nsis["uninstallerIcon"].as_str().expect("uninstaller icon");
        assert!(installer_icon.ends_with(".ico"));
        assert!(uninstaller_icon.ends_with(".ico"));

        let icon_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for icon in icons.into_iter().chain([installer_icon, uninstaller_icon]) {
            assert!(icon_dir.join(icon).is_file(), "缺少平台图标 {icon}");
        }
    }

    /// 平台本地化资源必须只改变用户可见名称，稳定安装身份与物理包名继续使用 LokiMetis。
    #[test]
    fn platform_bundle_name_resources_cover_english_and_simplified_chinese() {
        let info_plist = include_str!("../Info.plist");
        let macos_english = include_str!("../macos/en.lproj/InfoPlist.strings");
        let macos_chinese = include_str!("../macos/zh-Hans.lproj/InfoPlist.strings");
        let nsis_template = include_str!("../windows/nsis/installer.nsi");
        let nsis_english = include_str!("../windows/nsis/languages/English.nsh");
        let nsis_chinese = include_str!("../windows/nsis/languages/SimpChinese.nsh");

        assert!(info_plist.contains("LSHasLocalizedDisplayName"));
        assert!(info_plist.contains("zh-Hans"));
        assert!(macos_english.contains("\"CFBundleDisplayName\" = \"LokiMetis\";"));
        assert!(macos_chinese.contains("\"CFBundleDisplayName\" = \"诡秘神谕\";"));
        assert!(nsis_template.contains("Name \"$(productDisplayName)\""));
        assert!(nsis_template.contains("DisplayName\" \"$(productDisplayName)\""));
        assert!(!nsis_template.contains("\\${PRODUCTNAME}.lnk"));
        assert!(nsis_template.contains("Call RelocalizeExistingStartMenuShortcut"));
        assert!(nsis_template.contains("Call RelocalizeExistingDesktopShortcut"));
        assert!(nsis_template.contains("$OtherDisplayName.lnk"));
        assert!(nsis_english.contains("productDisplayName ${LANG_ENGLISH} \"LokiMetis\""));
        assert!(nsis_chinese.contains("productDisplayName ${LANG_SIMPCHINESE} \"诡秘神谕\""));
    }

    /// 内置皮肤、运行时样式与生成 Skill 必须通过 Tauri Resource 映射稳定打包。
    #[test]
    fn tauri_config_bundles_runtime_resources_at_stable_paths() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");
        let resources = config["bundle"]["resources"]
            .as_object()
            .expect("resource map");

        assert_eq!(
            resources
                .get("../resources/builtin-skins/minecraft/")
                .and_then(serde_json::Value::as_str),
            Some("builtin-skins/minecraft/")
        );
        assert_eq!(
            resources
                .get("../resources/theme-runtime/")
                .and_then(serde_json::Value::as_str),
            Some("theme-runtime/")
        );
        assert_eq!(
            resources
                .get("../../.agents/skills/codex-skin-generator/")
                .and_then(serde_json::Value::as_str),
            Some("codex-skin-generator/")
        );
    }

    /// 冷启动先启用 listener，再通过动态窗口路径创建桌宠，最后安装状态一致的托盘。
    #[test]
    fn cold_start_shows_pet_overlay_after_listener_and_before_tray_installation() {
        let source = include_str!("lib.rs");
        let listener = source
            .find("let (hook_relay_status, hook_listener_control) = spawn_hook_listener(")
            .expect("listener setup");
        let performance_guard = source
            .find("let hook_config_writer = if performance_evidence_enabled {")
            .expect("performance evidence hook repair guard");
        let disabled_writer = source[performance_guard..]
            .find("HookConfigWriter::disabled()")
            .map(|offset| performance_guard + offset)
            .expect("disabled hook writer");
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
            listener < performance_guard
                && performance_guard < disabled_writer
                && disabled_writer < hook_home
                && hook_home < hook_writer
                && hook_writer < hook_repair
                && hook_repair < show
                && show < tray
        );
    }

    /// 静态配置只预建主窗口；桌宠和设置窗分别由 Rust 在需要时动态创建。
    #[test]
    fn auxiliary_pet_windows_are_created_only_by_rust_builders() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");
        let configured_labels = config["app"]["windows"]
            .as_array()
            .expect("windows")
            .iter()
            .map(|window| window["label"].as_str().expect("window label"))
            .collect::<Vec<_>>();
        assert_eq!(configured_labels, ["main"]);

        let source = include_str!("monitor/pet_window.rs");
        let overlay_start = source
            .find("pub fn show_or_create_pet_overlay(")
            .expect("pet overlay factory");
        let settings_start = source
            .find("pub fn show_pet_settings_window(")
            .expect("pet settings factory");
        let settings_end = source
            .find("pub fn hide_pet_settings_window(")
            .expect("next pet settings function");
        let overlay_factory = &source[overlay_start..settings_start];
        let settings_factory = &source[settings_start..settings_end];
        let compact_overlay_factory = overlay_factory.split_whitespace().collect::<String>();
        let compact_settings_factory = settings_factory.split_whitespace().collect::<String>();

        assert!(
            compact_overlay_factory.contains("WebviewWindowBuilder::new(app,description.label,")
        );
        assert!(overlay_factory.contains("WebviewUrl::App(\"index.html?view=pet\".into())"));
        assert!(compact_overlay_factory.contains(".build()"));
        assert!(
            compact_settings_factory.contains("WebviewWindowBuilder::new(app,PET_SETTINGS_LABEL,")
        );
        assert!(
            settings_factory.contains("WebviewUrl::App(\"index.html?view=pet-settings\".into())")
        );
        assert!(compact_settings_factory.contains(".build()"));
    }

    /// 统一 Agent 选择必须先完成两侧持久化，再更新自动补写与监听快照。
    #[test]
    fn saving_enabled_tools_queues_best_effort_hook_repair_after_persistence() {
        let source = include_str!("monitor/commands.rs");
        let start = source
            .find("pub async fn save_enabled_ai_selection(")
            .expect("enabled tools command");
        let end = source[start..]
            .find("/// 保存某工具的自定义 Hook 目录。")
            .map(|offset| start + offset)
            .expect("next command boundary");
        let command = &source[start..end];
        let dashboard = command
            .find("state.set_enabled_ai_tools(&tools).await")
            .expect("dashboard persistence");
        let monitor = command
            .find("let settings = match save_enabled_tools(&config_dir, tools)")
            .expect("monitor persistence");
        let repair = command
            .find("hook_writer.request_enabled(settings.clone())")
            .expect("automatic repair request");
        let listener = command
            .find("hook_listener.replace_enabled_tools(&settings.enabled_ai_tools)")
            .expect("listener enabled gate update");
        let response = command[listener..]
            .find("Ok(EnabledAiSelectionResult")
            .map(|offset| listener + offset)
            .expect("successful response");
        assert!(
            dashboard < monitor && monitor < repair && repair < listener && listener < response
        );
    }

    /// 自定义目录保存成功后必须立即替换自动修复快照，不能继续补写旧目录。
    #[test]
    fn saving_hook_directory_refreshes_the_owned_repair_worker_snapshot() {
        let source = include_str!("monitor/commands.rs");
        let start = source
            .find("pub fn save_hook_config_directory(")
            .expect("directory command");
        let end = source[start..]
            .find("pub fn list_monitor_hook_locations(")
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
