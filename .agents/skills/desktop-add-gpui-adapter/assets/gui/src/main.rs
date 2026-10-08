//! 原生 GPUI 入口；前台只处理窗口与展示，偏好文件由独立后台工作线程读写。

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en-US");

mod app;
mod assets;
mod preferences;
mod logging;
mod lifecycle;
mod release_notes;
@@NOTIFICATION_MODULE@@
@@NATIVE_MODULE@@

use gpui_kit::{App, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

/// 同步主线程持有原生事件循环，退出钩子完成偏好与日志收尾，循环返回只作一次性回退。
fn main() -> std::process::ExitCode {
@@INSTANCE_START@@
    let log_guard = match logging::init() {
        Ok(guard) => guard,
        Err(code) => { eprintln!("日志初始化失败: {code}"); return std::process::ExitCode::FAILURE; }
    };
    tracing::info!(event = "application_started");
    // 保留候选内原始更新日志字节供打包核验；中性构建的两项常量都为空。
    std::hint::black_box(release_notes::RELEASE_NOTES_JSON);
    std::hint::black_box(release_notes::RELEASE_NOTES);
    let (preferences, writer) = preferences::load();
@@NATIVE_LOCALE@@
    let shutdown = lifecycle::Shutdown::new(writer.clone(), log_guard);
    let native_shutdown = shutdown.clone();
    gpui_kit::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        use gpui_kit::component as gpui_component;
        rust_i18n::extend!(gpui_component);
        gpui_kit::init(cx);
        cx.set_app_identity("@@APP_IDENTIFIER@@", "@@NAME_EN@@");
        lifecycle::install(cx, native_shutdown);
        cx.set_quit_mode(QuitMode::Explicit);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        }).detach();
        let fallback = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let bounds = preferences.window_bounds(cx).unwrap_or(WindowBounds::Windowed(fallback));
        let (@@NATIVE_HANDLE@@, _shell) = gpui_kit::open_window(WindowOptions {
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitlebarOptions { title: Some("@@NAME_EN@@".into()), ..Default::default() }),
            app_id: Some("@@APP_IDENTIFIER@@".into()),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| app::Shell::new(preferences, writer, window, cx)))
        .expect("无法创建 GPUI 窗口");
@@NATIVE_INSTALL@@
        cx.activate(true);
    });
    shutdown.borrow_mut().finish();
    std::process::ExitCode::SUCCESS
}
