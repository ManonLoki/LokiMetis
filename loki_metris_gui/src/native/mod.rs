//! 原生能力统一持有生命周期；平台对象在 UI 线程创建，后台输入通过有界轮询映射。

pub(crate) mod windowing;
pub(crate) mod keyboard_entry;
mod tray;
mod tray_hide;
#[cfg(target_os = "linux")]
mod tray_linux;
#[cfg(target_os = "linux")]
pub(crate) mod linux;
mod menu;
pub mod autostart;
pub mod notifications;
pub mod links;
pub mod instance;
pub(crate) mod startup;
pub mod shortcuts;
#[cfg(windows)]
mod windows;
#[cfg(all(windows, test))]
pub(crate) use windows::allows_geometry_save;

use gpui_kit::{AnyWindowHandle, App, Global, Task};
use std::time::Duration;

/// 平台窗口与已选服务由唯一 Global 持有，不增加第二条事件循环。
pub struct Services {
    main:AnyWindowHandle,
    /// 主窗后端在安装时捕获；关闭回调已借用Window，不能再次update同一窗口探测句柄。
    #[cfg(target_os = "linux")]
    main_is_x11: bool,
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    shell: gpui_kit::WeakEntity<crate::app::Shell>,
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    window_host: Option<windowing::WindowHost>,
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    window_revision: u64,
    #[cfg(any(target_os="macos", target_os="linux", all(windows, not(test))))]
    window_action_generation: u64,
    /// Linux 显式隐藏不同于遮挡/最小化；托盘入口消失时只恢复真实隐藏的主窗。
    #[cfg(target_os = "linux")]
    main_hidden: bool,
    _poll: Task<()>,
    /// 测试窗口没有原生宿主事务，显示时直接操作捕获的 HWND。
    #[cfg(all(windows, test))]
    window: Option<windows::MainWindow>,
    tray: Option<tray::Tray>,
    /// 虚拟平台只替换恢复入口事实，不创建真实托盘或伪装原生显隐已通过。
    #[cfg(all(target_os = "linux", test))]
    tray_restore_available_for_test: Option<bool>,
    pub autostart: autostart::Service,
    pub notifications: notifications::Service,
    _instance: instance::Instance,
    pub shortcuts: shortcuts::Service,
}
impl Global for Services {}
impl Services {
    /// Dock/重开复用真实主窗，不新建壳层或丢失当前草稿。
    pub fn main_window(&self) -> AnyWindowHandle { self.main }
}

/// 主窗口创建后安装宿主机制；不注册登录项，不申请通知权限，不绑定默认热键。
pub fn install(handle: AnyWindowHandle, shell: gpui_kit::WeakEntity<crate::app::Shell>, locale: &str, instance: instance::Instance, receiver: std::sync::mpsc::Receiver<()>, notifications: notifications::Service, cx: &mut App) {
    menu::install(locale, cx);
    #[cfg(target_os = "linux")]
    let main_is_x11 = handle.update(cx, |_, window, _| linux::x11_window_id(window).is_ok()).unwrap_or(false);
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    let window_host = shell.update(cx, |shell, _| shell.window_host()).ok().flatten();
    #[cfg(windows)]
    let native_window = handle.update(cx, |_, window, _| windows::MainWindow::capture(window)).ok().and_then(Result::ok);
    let _ = locale;
    #[cfg(not(any(target_os="macos", all(windows, not(test)))))]
    let _ = shell;
    #[cfg(windows)]
    let allows_tray = native_window.is_some();
    #[cfg(not(windows))]
    let allows_tray = true;
    let tray = if allows_tray { tray::Tray::new(locale, cx.background_executor().clone()).ok() } else { None };
    if tray.is_none() { tracing::warn!(event = "tray_unavailable"); }
    let autostart = autostart::Service::new();
    let mut revision = 0;
    let mut last_notification_key=None;
    let mut last_mouse_state=None;
    #[cfg(target_os = "linux")]
    let mut last_tray_available = false;
    #[cfg(target_os="macos")]
    let mut last_focus_revision = 0;
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update(|cx| {
                if !cx.has_global::<Services>() { return }
                #[cfg(any(target_os="macos", all(windows, not(test))))]
                {
                    #[cfg(target_os="macos")]
                    {
                    let focus_revision = cx.global::<Services>().window_host.as_ref().map(windowing::WindowHost::focus_revision);
                    if let Some(revision) = focus_revision.filter(|revision| *revision != last_focus_revision) {
                        // 确认本轮先读取的版本；刷新期间的新通知必须留到下一轮。
                        last_focus_revision = revision;
                        menu::refresh(cx);
                    }
                    }
                    let service = cx.global_mut::<Services>();
                    let revision = service.window_host.as_ref().and_then(windowing::WindowHost::settled_revision);
                    if let Some(revision) = revision.filter(|revision| *revision != service.window_revision) {
                        service.window_revision = revision;
                        let shell = service.shell.clone();
                        let _ = handle.update(cx, |_, window, cx| { let _ = shell.update(cx, |shell, cx| shell.poll_native_window(window, cx)); });
                    }
                }
                if receiver.try_iter().take(64).count() > 0 { show(handle, cx); }
                #[cfg(target_os = "linux")]
                {
                    let available = tray_restore_available(cx);
                    if last_tray_available && !available && cx.global::<Services>().main_hidden { show(handle, cx); }
                    last_tray_available = available;
                }
                let events = cx.global::<Services>().tray.as_ref().map(|tray| tray.events()).unwrap_or_default();
                for event in events { match event { tray::Event::Show => show(handle, cx), tray::Event::Quit => cx.quit(), tray::Event::TogglePet => crate::pet::toggle(cx), tray::Event::OpenPetSettings => crate::pet::open_settings(cx), tray::Event::ToggleMousePassthrough => crate::pet::toggle_mouse_passthrough(cx), tray::Event::ToggleNotification=> {let enabled=notification_state(cx).enabled;set_notification(!enabled,cx);}, tray::Event::ToggleAutostart=>toggle_autostart(cx) } }
                let mouse_state=crate::pet::mouse_passthrough_state(cx);
                if last_mouse_state != Some(mouse_state) {
                    if let Some(tray)=cx.global_mut::<Services>().tray.as_mut() {tray.set_mouse_passthrough(mouse_state);}
                    last_mouse_state=Some(mouse_state);
                }
                if let Some((key,state))=cx.global::<Services>().notifications.state_if_changed(last_notification_key) {
                    if let Some(tray)=cx.global_mut::<Services>().tray.as_mut() {tray.set_notification(&state);}
                    menu::refresh(cx);last_notification_key=Some(key);cx.refresh_windows();
                }
                let current = cx.global::<Services>().autostart.state().revision;
                if current != revision {
                    revision = current;
                    let state = autostart_state(cx);
                    if let Some(tray) = cx.global_mut::<Services>().tray.as_mut() { tray.set_autostart(&state); }
                    menu::refresh(cx); cx.refresh_windows();
                }
                let handlers = cx.global::<Services>().shortcuts.pending();
                for dispatch in handlers { dispatch(cx); }
            });
        }
    });
    cx.set_global(Services {
        main:handle,
        #[cfg(target_os = "linux")]
        main_is_x11,
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        shell,
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        window_host,
        #[cfg(any(target_os="macos", all(windows, not(test))))]
        window_revision: 0,
        #[cfg(any(target_os="macos", target_os="linux", all(windows, not(test))))]
        window_action_generation: 0,
        #[cfg(target_os = "linux")]
        main_hidden: false,
        _poll: poll,
        #[cfg(all(windows, test))]
        window: native_window,
        tray,
        #[cfg(all(target_os = "linux", test))]
        tray_restore_available_for_test: None,
        autostart,
        notifications,
        _instance: instance,
        shortcuts: shortcuts::Service::new(),
    });
    menu::refresh(cx);
    cx.on_app_quit(|cx| {
        let services = cx.remove_global::<Services>();
        async move { drop(services); }
    }).detach();
}

/// 原生主窗的 key 状态是菜单可用性的唯一依据，尚未安装宿主时保持禁用。
#[cfg(target_os = "macos")]
fn main_window_is_key(cx: &App) -> bool {
    cx.try_global::<Services>().and_then(|services| services.window_host.as_ref())
        .is_some_and(windowing::WindowHost::is_key)
}

/// 激活回调先释放窗口借用，再读取最新 key 状态刷新菜单，避免排队期间焦点失效。
#[cfg(target_os = "macos")]
pub(crate) fn refresh_menu_after_activation(cx: &mut App) {
    cx.defer(menu::refresh);
}

/// 主动显示统一捕获入口鼠标屏；原生修改由前台 task 在借用之外执行。
pub fn show(handle: AnyWindowHandle, cx: &mut App) {
    #[cfg(all(any(target_os="macos", windows), not(test)))]
    {
        let desktop = windowing::capture_desktop(cx);
        let Some(shell) = cx.try_global::<Services>().map(|service| service.shell.clone()) else { return; };
        match desktop {
            Ok(desktop) => show_on_desktop(handle, desktop, cx),
            Err(_) => {
                tracing::warn!(event="window_desktop_unavailable");
                let generation = next_window_action(cx);
                let host = handle.update(cx, |_, window, cx| shell.update(cx, |shell, cx| shell.prepare_window_hide(window, cx)).ok().flatten()).ok().flatten();
                cx.spawn(async move |cx| {
                    let live = cx.update(|cx| window_action_current(generation, cx) && handle.update(cx, |_, _, _| ()).is_ok());
                    if live && let Some(host) = host { host.show(); cx.update(|cx| cx.activate(true)); }
                }).detach();
            }
        }
    }
    #[cfg(any(not(any(target_os="macos", windows)), test))]
    {
    #[cfg(windows)]
    let native = cx.try_global::<Services>().and_then(|services| services.window);
    #[cfg(target_os = "linux")]
    let generation = next_window_action(cx);
    cx.defer(move |cx| {
        #[cfg(target_os = "linux")]
        if !window_action_current(generation, cx) { return; }
        #[cfg(windows)]
        if let Some(window) = native { window.show(); }
        cx.activate(true);
        let _ = handle.update(cx, |_, window, cx| {
            #[cfg(target_os = "linux")]
            if linux::x11_window_id(window).is_ok() {
                if crate::pet::host::set_visible(window, true).is_err() {
                    tracing::warn!(event="main_window_show_failed");
                }
                // 最小化时 WM 的父框可能尚未映射；仍须发 _NET_ACTIVE_WINDOW 解除隐藏。
                if cx.has_global::<Services>() { cx.global_mut::<Services>().main_hidden = false; }
            }
            #[cfg(not(target_os = "linux"))]
            let _ = cx;
            window.activate_window();
        });
    });
    }
}

/// 启动入口已捕获指针时直接传递，避免创建辅助窗之后改变本次选屏。
#[cfg(any(target_os="macos", all(windows, not(test))))]
pub(crate) fn show_on_desktop(handle: AnyWindowHandle, desktop: windowing::DesktopSnapshot, cx: &mut App) {
    next_window_action(cx);
    let Some(shell) = cx.try_global::<Services>().map(|service| service.shell.clone()) else { return; };
    let _ = handle.update(cx, |_, window, cx| {
        let _ = shell.update(cx, |shell, cx| shell.request_window_restore(desktop, window, cx));
    });
}

/// 仅最小化接收按键的主窗；先取消恢复租约，再于全部 GPUI 借用之外调用 AppKit。
#[cfg(target_os = "macos")]
pub(crate) fn minimize_main(cx: &mut App) {
    let Some((main, shell, host)) = cx.try_global::<Services>().and_then(|services| {
        services.window_host.clone().map(|host| (services.main, services.shell.clone(), host))
    }) else { return; };
    // AppKit 的 mainWindow 可能仍指向失焦主窗，必须按真实 keyWindow 判断。
    if !host.is_key() { return; }
    cx.defer(move |cx| {
        cx.spawn(async move |cx| {
            if !host.is_key() { return; }
            let ready = cx.update(|cx| main.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.prepare_window_hide(window, cx)).ok().flatten()
            }).ok().flatten());
            if let Some(host) = ready { host.minimize_if_key(); }
        }).detach();
    });
}

/// 托盘实际成功才隐藏，失败允许正常关闭，不留下无法恢复的进程。
pub fn hide_if_available(cx: &mut App) -> bool {
    #[cfg(target_os = "linux")]
    {
        if !cx.try_global::<Services>().is_some_and(|service| service.main_is_x11) { return false; }
    }
    let available = tray_restore_available(cx);
    tracing::info!(event="tray_hide_requested", services_present=cx.has_global::<Services>(),
        tray_present=cx.try_global::<Services>().is_some_and(|service| service.tray.is_some()),
        restore_entry_available=available);
    let Some(request) = tray_hide::Request::begin(|| available) else { return false };
    #[cfg(all(windows, test))]
    let native = cx.global::<Services>().window;
    #[cfg(not(all(windows, test)))]
    let main=cx.global::<Services>().main;
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    {
        let generation = next_window_action(cx);
        let shell = cx.global::<Services>().shell.clone();
        let host = shell.update(cx, |shell, _| shell.cancel_window_restore()).ok().flatten();
        cx.spawn(async move |cx| {
            let (live, available) = cx.update(|cx| {
                let live = window_action_current(generation, cx) && main.update(cx, |_, _, _| ()).is_ok();
                (live, live && tray_restore_available(cx))
            });
            request.finish(live, || available, || { if let Some(host) = host { host.hide(); } });
        }).detach();
        true
    }
    #[cfg(not(any(target_os="macos", all(windows, not(test)))))]
    {
    #[cfg(target_os = "linux")]
    let generation = next_window_action(cx);
    cx.defer(move |cx| {
        #[cfg(target_os = "linux")]
        let current = window_action_current(generation, cx);
        #[cfg(not(target_os = "linux"))]
        let current = true;
        let available = tray_restore_available(cx);
        request.finish(current, || available, || {
        #[cfg(windows)]
        if let Some(window) = native { window.hide(); }
        #[cfg(target_os = "linux")]
        if main.update(cx, |_, window, _| crate::pet::host::set_visible(window, false)).is_ok_and(|result| result.is_ok()) {
            cx.global_mut::<Services>().main_hidden = true;
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        let _=main.update(cx,|_,window,_|crate::pet::host::set_visible(window,false));
        });
    });
    true
    }
}

/// 只在关闭边界读取系统托盘位置，不增加后台轮询或第二份托盘状态。
pub(crate) fn tray_restore_available(cx: &App) -> bool {
    #[cfg(all(target_os = "linux", test))]
    if let Some(available) = cx.try_global::<Services>().and_then(|service| service.tray_restore_available_for_test) { return available; }
    cx.try_global::<Services>().and_then(|services| services.tray.as_ref()).is_some_and(tray::Tray::restore_entry_available)
}

/// 服务级显示代次约束延迟 fallback show/hide，后来的相反意图撤销旧命令。
#[cfg(any(target_os="macos", target_os="linux", all(windows, not(test))))]
fn next_window_action(cx: &mut App) -> u64 {
    if !cx.has_global::<Services>() { return 0; }
    let service = cx.global_mut::<Services>();
    service.window_action_generation = service.window_action_generation.wrapping_add(1);
    service.window_action_generation
}

/// 退出移除服务或代次变化时，迟到原生命令保持零副作用。
#[cfg(any(target_os="macos", target_os="linux", all(windows, not(test))))]
fn window_action_current(generation: u64, cx: &App) -> bool {
    cx.try_global::<Services>().map_or(generation == 0, |service| service.window_action_generation == generation)
}
/// 语言切换更新已创建的双语托盘菜单。
pub fn set_language(locale: &str, cx: &mut App) {
    menu::set_language(locale, cx);
    if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.set_language(locale);}
    crate::pet::set_language(locale,cx);
    if cx.has_global::<Services>() && let Some(tray) = &mut cx.global_mut::<Services>().tray { tray.set_language(locale); }
}
/// 设置视图仅读取内存快照，实际 OS 状态由后台 worker 更新。
pub fn autostart_state(cx: &App) -> autostart::State {
    cx.try_global::<Services>().map(|services| services.autostart.state()).unwrap_or_default()
}
/// 事件直接来自自启控件，初始化不会调用此 mutation。
pub fn set_autostart(enabled: bool, cx: &App) {
    if let Some(services) = cx.try_global::<Services>() { services.autostart.set(enabled); }
}
/// 原生菜单依据同一 OS 事实切换；未知/错误时只重读，忙碌时不提交额外写入。
pub(super) fn toggle_autostart(cx: &App) {
    let state = autostart_state(cx);
    if state.busy { return; }
    if state.error || state.enabled.is_none() { reload_autostart(cx); }
    else if let Some(enabled) = state.enabled { set_autostart(!enabled, cx); }
}
/// 读取失败时提供显式重试，不能乐观假定系统状态。
pub fn reload_autostart(cx: &App) {
    if let Some(services) = cx.try_global::<Services>() { services.autostart.reload(); }
}

/// macOS 首次绘字前仅在进程内关闭附加笔画膨胀，与参考灰度抗锯齿一致。
#[cfg(target_os="macos")]
pub(crate) fn configure_font_smoothing() {
    use objc2_foundation::{NSUserDefaults,NSArgumentDomain,NSMutableDictionary,NSNumber,ns_string};
    let defaults=NSUserDefaults::standardUserDefaults();
    // SAFETY: 读取 Foundation 导出的不可变静态域名，借用在进程内始终有效；volatile 域不落盘且保留已有参数。
    let domain=unsafe {NSArgumentDomain};
    let current=defaults.volatileDomainForName(domain);
    let arguments=NSMutableDictionary::dictionaryWithDictionary(&current);
    let zero=NSNumber::new_i32(0);
    arguments.insert(ns_string!("AppleFontSmoothing"),zero.as_ref());
    // SAFETY: 字典值类型为 Foundation 支持的 NSNumber，仅修改当前进程搜索列表。
    unsafe {defaults.setVolatileDomain_forName(&arguments,domain);}
}

/// 托盘只按用户会话可见性改标签，不把系统遮挡误当作隐藏。
pub fn set_pet_visibility(visible:bool,cx:&mut App) {if cx.has_global::<Services>()&& let Some(tray)=cx.global_mut::<Services>().tray.as_mut() {tray.set_pet_visibility(visible);}}

/// 设置页只读取应用级通知偏好，离页不丢失真实保存结果。
pub fn notification_state(cx:&App)->notifications::State {cx.try_global::<Services>().map(|services|services.notifications.state()).unwrap_or_default()}

/// 系统授权与偏好保存串行归属原生服务；初始化从不调用。
pub fn set_notification(enabled:bool,cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|services|services.notifications.clone()) {service.set(enabled,cx);}}

/// 激活或进入设置只触发有界只读权限查询。
pub fn refresh_notification_permission(cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.refresh_permission(cx);}}
/// 明确按钮重试由同一应用 owner 处理。
pub fn retry_notifications(cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.retry(cx);}}

#[cfg(all(target_os = "linux", test))]
mod close_tests;
