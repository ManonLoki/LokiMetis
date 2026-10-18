//! 原生 GPUI 入口；前台只处理窗口与展示，偏好文件由独立后台工作线程读写。
//!
//! # 依赖层级
//!
//! 上层只依赖下层；业务规则、口径与状态机只存在于 `loki_metris_core`。
//!
//! 1. 领域核心 `loki_metris_core`：用量口径、自然日窗口、聚合与页面模型、Hook 状态机、桌宠布局、
//!    设置校验与数据根策略；不依赖 GPUI 或任何宿主 API。
//! 2. 驱动层，彼此不互相依赖，只依赖 core：
//!    - `backend`：Agent 会话文件发现、JSONL 解析、SQLite 增量索引与扫描编排，WorkBuddy 只读统计；
//!    - `monitor`：Hook 配置原子写入、环回 listener、命令型中继、图库与图片解码播放；
//!    - `skins`：Codex/WorkBuddy 换肤资源校验、宿主进程身份与 CDP 注入；
//!    - `preferences`：窗口与外观偏好的后台单线程写入。
//! 3. 服务层 `usage_service`：应用拥有的单 writer 扫描队列、只读查询邮箱、今日摘要与里程碑通知 worker，
//!    以及换肤、监控、数据目录写操作的后台 owner。它把驱动结果交给 core 生成页面模型，再以 oneshot 回执交付。
//! 4. 展示与宿主层，共享 GPUI 主线程：`app`（主窗口 Shell 与各页面）、`pet`（桌宠浮窗）、
//!    `native`（托盘、单实例、自启、系统通知、原生窗口几何）。数据读写一律经服务层；只直接复用驱动层的
//!    DTO 类型与图片播放组件。`native` 持有 `app::Shell` 与桌宠的弱引用，仅用于恢复和显隐窗口。
//! 5. 进程级 `lifecycle`：退出时依次收尾服务、桌宠、偏好与日志。
//!
//! # 主要业务流向
//!
//! - 用量看板：Agent 会话文件 → `backend::local_index` 增量解析入库 → `usage_service::scan` 单 writer 提交覆盖事实
//!   → `usage_service::query` 只读加载并调用 core 视图（概览、统计、图表、调用页）→ `app::dashboard` 经
//!   `usage_service::poll_receipt` 取回并渲染。
//! - Hook 监控与桌宠：Agent 触发 Hook → 本程序以中继模式启动（`monitor::relay`）→ 环回 `monitor::listener`
//!   → core Hook 状态机 → `pet` 浮窗与 `app::monitor` 工作台展示。
//! - 用量里程碑通知：`usage_service::notifications` 读取今日摘要并推进 core 里程碑状态机 → `native::notifications` 投递。
//! - 换肤：`app::skins` 发起意图 → `usage_service::skins` 后台阻塞岛 → `skins` 校验资源并经 CDP 注入宿主页面。

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
mod notification_events;
mod pet;
mod skins;

use gpui_kit::{App, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

/// 同步主线程持有原生事件循环，退出钩子完成偏好与日志收尾，循环返回只作一次性回退。
fn main() -> std::process::ExitCode {
    if let Some(code)=monitor::relay::run_hook_relay_if_requested() {return std::process::ExitCode::from(code as u8)}
    #[cfg(target_os="macos")]
    native::configure_font_smoothing();
    let starts_hidden = native::startup::starts_hidden();
    let (instance_sender, instance_receiver) = std::sync::mpsc::sync_channel(1);
    let directory = match preferences::instance_directory() { Ok(path) => path, Err(_) => return std::process::ExitCode::FAILURE };
    let instance = match native::instance::Instance::acquire(&directory, instance_sender, !starts_hidden) {
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
    #[cfg(target_os = "linux")]
    native::linux::prepare_icon();
    let application = gpui_kit::application().with_assets(assets::Assets);
    // 普通 Wayland toplevel 无全局定位/置顶合同；使用本会话 X11/XWayland 保留桌宠行为。
    #[cfg(target_os = "linux")]
    let application = application.with_windowing(gpui_kit::WindowingRequest::Windowed(
        native::linux::desktop_environment(gpui_kit::GraphicalEnvironment::detect()),
    ));
    // macOS 的 Headless 仅选择 Accessory 激活策略，仍可显示原生窗口；在事件循环前避免 Dock 闪现。
    #[cfg(target_os = "macos")]
    let application = application.with_windowing(gpui_kit::WindowingRequest::Headless);
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
        // 只有原生显示器选择路径会改写初始窗口参数。
        #[cfg_attr(not(any(target_os="macos", all(windows, not(test)))), allow(unused_mut))]
        let mut options = WindowOptions {
            show: !starts_hidden,
            focus: !starts_hidden,
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(960.), px(640.))),
            titlebar: Some(TitlebarOptions { title: Some("LokiMetris".into()), ..Default::default() }),
            app_id: Some("com.loki-metris.app".into()),
            ..Default::default()
        };
        #[cfg(any(target_os="macos", all(windows, not(test))))]
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
        #[cfg(target_os = "linux")]
        let _ = handle.update(cx, |_, window, _| {
            if native::linux::install_icon(window).is_err() {
                tracing::warn!(event="linux_window_icon_unavailable");
            }
        });
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        if startup_desktop.is_none() {
            let _ = handle.update(cx, |_, window, cx| shell.update(cx, |shell, cx| shell.release_initial_window_restore(window, cx)));
        }
        native_shutdown.borrow_mut().own_window(handle, shell.downgrade());
        let main_id=handle.window_id();
        cx.on_window_closed(move|cx,closed| {
            // 主窗确实被销毁表示托盘恢复不可用；辅助窗不能留下无入口进程。
            if closed==main_id||cx.windows().is_empty() {
                tracing::info!(event="application_quit_requested", source="window_closed", main_closed=closed==main_id);
                cx.quit();
            }
        }).detach();
        notifications.attach(usage_service.clone(),locale,handle,cx);
        native::install(handle, shell.downgrade(), locale, instance, instance_receiver, notifications, cx);
        pet::install(handle,usage_service,locale,cx);
        if !native::startup::show_main(starts_hidden, native::tray_restore_available(cx)) {
            tracing::info!(event = "application_started_hidden");
            return;
        }
        // 托盘创建失败时恢复普通应用身份，保证可见主窗仍有 Dock/退出入口。
        #[cfg(target_os = "macos")]
        if !native::tray_restore_available(cx) { cx.set_activation_policy(gpui_kit::ActivationPolicy::Regular); }
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        if let Some(desktop) = startup_desktop { native::show_on_desktop(handle, desktop, cx); } else { native::show(handle, cx); }
        #[cfg(not(any(target_os="macos", all(windows, not(test)))))]
        cx.activate(true);
    });
    shutdown.borrow_mut().finish();
    std::process::ExitCode::SUCCESS
}
