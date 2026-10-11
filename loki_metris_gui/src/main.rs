//! 原生 GPUI 入口；前台只处理窗口与展示，偏好文件由独立后台工作线程读写。

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod backend;
mod usage_service;
mod monitor;
mod assets;
mod preferences;
mod logging;
mod lifecycle;
mod release_notes;
mod fonts;
mod format_tokens;

mod native;
mod pet;
mod skins;

use gpui_kit::{App, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

/// 同步主线程持有原生事件循环，退出钩子完成偏好与日志收尾，循环返回只作一次性回退。
fn main() -> std::process::ExitCode {
    if let Some(code)=monitor::relay::run_hook_relay_if_requested() {return std::process::ExitCode::from(code as u8)}
    #[cfg(target_os="macos")]
    native::configure_font_smoothing();
    let (instance_sender, instance_receiver) = std::sync::mpsc::sync_channel(1);
    let directory = match preferences::instance_directory() { Ok(path) => path, Err(_) => return std::process::ExitCode::FAILURE };
    let instance = match native::instance::Instance::acquire(&directory, instance_sender) {
        Ok(native::instance::Launch::Primary(instance)) => instance,
        Ok(native::instance::Launch::Activated) => return std::process::ExitCode::SUCCESS,
        Err(_) => { eprintln!("single_instance_unavailable"); return std::process::ExitCode::FAILURE; }
    };
    let log_guard = match logging::init() {
        Ok(guard) => guard,
        Err(code) => { eprintln!("日志初始化失败: {code}"); return std::process::ExitCode::FAILURE; }
    };
    tracing::info!(event = "application_started");
    // 保留候选内原始更新日志字节供打包核验；中性构建的两项常量都为空。
    std::hint::black_box(release_notes::RELEASE_NOTES_JSON);
    std::hint::black_box(release_notes::RELEASE_NOTES);
    let (preferences, writer) = preferences::load();
    let locale = preferences.locale();
    let notifications=native::notifications::Service::new(preferences.system_notification,writer.clone());
    let shutdown = lifecycle::Shutdown::new(writer.clone(), log_guard);
    shutdown.borrow_mut().own_notifications(notifications.clone());
    let usage_service = match usage_service::UsageService::new() {
        Ok(service) => service,
        Err(code) => { tracing::error!(event = code); return std::process::ExitCode::FAILURE; }
    };
    if let Err(error)=usage_service.start_monitoring() {tracing::error!(event=error);}
    shutdown.borrow_mut().own_usage(usage_service.clone());
    let native_shutdown = shutdown.clone();
    let application = gpui_kit::application().with_assets(assets::Assets);
    application.on_reopen(|cx| {
        if let Some(handle) = cx.try_global::<native::Services>().map(native::Services::main_window) { native::show(handle, cx); }
    });
    application.run(move |cx: &mut App| {
        use gpui_kit::component as gpui_component;
        rust_i18n::extend!(gpui_component);
        gpui_kit::init(cx);
        fonts::install(cx);
        cx.set_app_identity("com.loki-metris.app", "LokiMetris");
        lifecycle::install(cx, native_shutdown.clone());
        cx.set_quit_mode(QuitMode::Explicit);
        let fallback = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let bounds = preferences.window_bounds(cx).unwrap_or(WindowBounds::Windowed(fallback));
        let mut options = WindowOptions {
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitlebarOptions { title: Some("LokiMetris".into()), ..Default::default() }),
            app_id: Some("com.loki-metris.app".into()),
            ..Default::default()
        };
        #[cfg(target_os="macos")]
        let startup_desktop = native::windowing::capture_desktop(cx).ok().and_then(|desktop| {
            let (bounds, id) = native::windowing::initial_main_bounds(&preferences, &desktop)?;
            options.window_bounds = Some(bounds); options.display_id = Some(id);
            options.show = false; options.focus = false; Some(desktop)
        });
        let (handle, shell) = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| {
            let mut shell = app::Shell::new(preferences, writer, window, cx);
            shell.attach_usage(usage_service.clone(), cx);
            shell
        }))
        .expect("无法创建 GPUI 窗口");
        #[cfg(target_os="macos")]
        if startup_desktop.is_none() {
            let _ = handle.update(cx, |_, window, cx| shell.update(cx, |shell, cx| shell.release_initial_window_restore(window, cx)));
        }
        native_shutdown.borrow_mut().own_window(handle, shell.downgrade());
        let main_id=handle.window_id();
        cx.on_window_closed(move|cx,closed| {
            // 主窗确实被销毁表示托盘恢复不可用；辅助窗不能留下无入口进程。
            if closed==main_id||cx.windows().is_empty() {cx.quit();}
        }).detach();
        notifications.attach(usage_service.clone(),locale,handle.into(),cx);
        native::install(handle, shell.downgrade(), locale, instance, instance_receiver, notifications, cx);
        pet::install(handle,usage_service,locale,cx);
        #[cfg(target_os="macos")]
        if let Some(desktop) = startup_desktop { native::show_on_desktop(handle, desktop, cx); } else { cx.activate(true); }
        #[cfg(not(target_os="macos"))]
        cx.activate(true);
    });
    shutdown.borrow_mut().finish();
    std::process::ExitCode::SUCCESS
}
