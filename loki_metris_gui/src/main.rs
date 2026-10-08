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

mod native;

use gpui_kit::{App, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

/// 同步主线程持有原生事件循环，退出钩子完成偏好与日志收尾，循环返回只作一次性回退。
fn main() -> std::process::ExitCode {
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
    let shutdown = lifecycle::Shutdown::new(writer.clone(), log_guard);
    let usage_service = match usage_service::UsageService::new() {
        Ok(service) => service,
        Err(code) => { tracing::error!(event = code); return std::process::ExitCode::FAILURE; }
    };
    shutdown.borrow_mut().own_usage(usage_service.clone());
    let native_shutdown = shutdown.clone();
    gpui_kit::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        use gpui_kit::component as gpui_component;
        rust_i18n::extend!(gpui_component);
        gpui_kit::init(cx);
        cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Bold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Geist-ExtraBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-ExtraBold.ttf")),
        ]).expect("无法注册内嵌字体");
        cx.set_app_identity("com.loki-metris.app", "LokiMetris");
        lifecycle::install(cx, native_shutdown);
        cx.set_quit_mode(QuitMode::Explicit);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        }).detach();
        let fallback = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let bounds = preferences.window_bounds(cx).unwrap_or(WindowBounds::Windowed(fallback));
        let (handle, _shell) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitlebarOptions { title: Some("LokiMetris".into()), ..Default::default() }),
            app_id: Some("com.loki-metris.app".into()),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| {
            let mut shell = app::Shell::new(preferences, writer, window, cx);
            shell.attach_usage(usage_service, cx);
            shell
        }))
        .expect("无法创建 GPUI 窗口");
        native::install(handle, locale, instance, instance_receiver, cx);
        cx.activate(true);
    });
    shutdown.borrow_mut().finish();
    std::process::ExitCode::SUCCESS
}
