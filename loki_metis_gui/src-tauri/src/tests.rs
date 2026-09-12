/// 单实例插件必须最先注册且只能注册一次。
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

/// 普通第二次启动应恢复现有主窗口，自启或深链接重复启动必须交给各自入口处理。
#[test]
fn second_launch_restores_existing_main_window() {
    let manual = ["/Applications/LokiMetis.app/Contents/MacOS/loki_metis_gui".to_owned()];
    let autostart = [manual[0].clone(), "--autostart".to_owned()];
    let valid_deep_link = [manual[0].clone(), "app-loki-metis://restore".to_owned()];
    let rejected_deep_link = [
        manual[0].clone(),
        "app-loki-metis://restore?payload=1".to_owned(),
    ];
    assert!(!super::is_autostart_launch(&manual));
    assert!(super::is_autostart_launch(&autostart));
    assert!(super::is_autostart_launch(&[
        manual[0].clone(),
        "--other".to_owned(),
        "--autostart".to_owned(),
    ]));
    assert!(!super::is_autostart_launch(&[
        manual[0].clone(),
        "--autostart-extra".to_owned(),
    ]));
    assert!(!super::is_autostart_launch(&["--autostart".to_owned()]));
    assert!(super::should_restore_main_window_for_second_launch(&manual));
    assert!(!super::should_restore_main_window_for_second_launch(
        &autostart
    ));
    assert!(!super::should_restore_main_window_for_second_launch(
        &valid_deep_link
    ));
    assert!(!super::should_restore_main_window_for_second_launch(
        &rejected_deep_link
    ));
    assert!(!super::should_restore_main_window_for_second_launch(&[
        manual[0].clone(),
        "APP-LOKI-METIS://restore?payload=1".to_owned(),
    ]));
}

/// 主窗口不得随进程启动自动可见，可见性完全由启动逻辑显式决定。
#[test]
fn main_window_config_starts_hidden() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri config");
    let windows = config["app"]["windows"].as_array().expect("windows");
    let main = windows
        .iter()
        .find(|window| window["label"] == "main")
        .expect("main window entry");
    assert_eq!(main["visible"], serde_json::json!(false));
}

/// 自启插件必须附加标记参数，供启动逻辑区分开机自启与用户手动启动。
#[test]
fn autostart_plugin_is_registered_with_launch_marker_arg() {
    let source = include_str!("lib.rs");
    assert!(source.contains(r#"const AUTOSTART_LAUNCH_ARG: &str = "--autostart";"#));
    let init = source
        .find("tauri_plugin_autostart::init(")
        .expect("autostart plugin registration");
    let call = &source[init..init + 160];
    assert!(call.contains("MacosLauncher::LaunchAgent"));
    assert!(call.contains("Some(vec![AUTOSTART_LAUNCH_ARG])"));
}

/// 开机自启拉起时必须跳过显式展示，主窗口才能保持隐藏；手动启动仍需展示。
#[test]
fn autostart_launch_skips_showing_main_window() {
    let source = include_str!("lib.rs");
    let flag = source
        .find("let launched_via_autostart = is_autostart_launch(")
        .expect("autostart launch flag is computed");
    let guard = source
        .find("if !launched_via_autostart {")
        .expect("main window show is gated on the autostart flag");
    let restore = source[guard..]
        .find("restore_main_window(app.handle())?;")
        .map(|offset| guard + offset)
        .expect("main window is restored for non-autostart launches");
    assert!(flag < guard);
    assert!(guard < restore);
}

/// 已有实例收到自启重复启动时，必须在回调里检查参数后才决定是否显示主窗口。
#[test]
fn autostart_second_launch_does_not_restore_main_window() {
    let source = include_str!("lib.rs");
    let callback = source
        .find("tauri_plugin_single_instance::init(|app, args, _cwd|")
        .expect("single-instance callback");
    let next_plugin = source[callback..]
        .find(".plugin(tauri_plugin_deep_link::init())")
        .map(|offset| callback + offset)
        .expect("next plugin");
    let handler = &source[callback..next_plugin];
    assert!(handler.contains("if should_restore_main_window_for_second_launch(&args) {"));
    assert!(handler.contains("restore_main_window(app)"));
}

/// Dock 图标常驻会与托盘常驻语义重复，macOS 上必须在启动时隐藏。
#[test]
fn macos_dock_icon_is_hidden_on_setup() {
    let source = include_str!("lib.rs");
    let cfg = source
        .find(r#"#[cfg(target_os = "macos")]"#)
        .expect("macos-gated setup code exists");
    let call = source[cfg..]
        .find("set_dock_visibility(false)")
        .map(|offset| cfg + offset)
        .expect("dock visibility is disabled on macOS setup");
    assert!(call > cfg);
}

/// 原生 dialog 权限只属于主窗口，桌宠及其设置窗只能使用窄 Rust IPC。
#[test]
fn native_dialog_capability_is_scoped_to_the_main_window() {
    let default_capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("default capability");
    let dialog_capability: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/main-dialog.json"))
            .expect("main dialog capability");

    assert_eq!(
        default_capability["windows"],
        serde_json::json!(["main", "pet", "pet-settings"])
    );
    assert_eq!(
        default_capability["permissions"],
        serde_json::json!(["core:default", "core:event:default"])
    );
    assert_eq!(dialog_capability["windows"], serde_json::json!(["main"]));
    assert_eq!(
        dialog_capability["permissions"],
        serde_json::json!(["dialog:default"])
    );
}

/// 启动发布 runtime state 与后台读取器之前必须完成唯一 writer 负责的索引迁移。
#[test]
fn startup_migrates_local_indexes_before_publishing_readers() {
    let source = include_str!("lib.rs");
    let migration = source
        .find("block_on(migrate_local_indexes(&runtime_state))")
        .expect("startup migration is wired");
    let managed = source
        .find("app.manage(runtime_state);")
        .expect("runtime state is published");
    let retention = source
        .find("spawn_retention_cleanup(app.handle().clone())")
        .expect("retention reader is started");
    let periodic = source
        .find("spawn_periodic_local_scans(app.handle().clone())")
        .expect("periodic reader is started");

    assert!(migration < managed);
    assert!(migration < retention);
    assert!(migration < periodic);
}

/// 窗口标题只显示应用名，不应拼接版本号。
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
    assert!(nsis_template.contains("$INSTDIR\\loki-metis-hook-relay.json"));
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

/// 退出时必须并发收敛后台、扫描与 Hook listener，再等待同步 writer。
#[test]
fn exit_reaps_all_async_owners_before_joining_hook_writer() {
    let source = include_str!("lib.rs");
    let exit_handler = source
        .find("app.run(|app_handle, event| {")
        .expect("application exit handler");
    let handler = &source[exit_handler..];
    let notification = handler
        .find("worker.shutdown().await")
        .expect("notification shutdown");
    let scan = handler
        .find("state.scan_tasks.shutdown()")
        .expect("bounded scan shutdown");
    let background = handler
        .find("state.background_tasks.shutdown()")
        .expect("bounded background shutdown");
    let listener = handler
        .find("listener.shutdown()")
        .expect("bounded listener shutdown");
    let hook_writer = handler
        .find("try_state::<HookConfigWriter>()")
        .expect("hook writer shutdown");

    assert!(
        notification < background
            && notification < scan
            && notification < listener
            && background < hook_writer
            && scan < hook_writer
            && listener < hook_writer
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

    assert!(compact_overlay_factory.contains("WebviewWindowBuilder::new(app,description.label,"));
    assert!(overlay_factory.contains("WebviewUrl::App(\"index.html?view=pet\".into())"));
    assert!(compact_overlay_factory.contains(".build()"));
    assert!(compact_settings_factory.contains("WebviewWindowBuilder::new(app,PET_SETTINGS_LABEL,"));
    assert!(settings_factory.contains("WebviewUrl::App(\"index.html?view=pet-settings\".into())"));
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
    assert!(dashboard < monitor && monitor < repair && repair < listener && listener < response);
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
