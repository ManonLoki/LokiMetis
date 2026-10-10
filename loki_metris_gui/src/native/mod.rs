//! 原生能力统一持有生命周期；平台对象在 UI 线程创建，后台输入通过有界轮询映射。

#[cfg(target_os="macos")]
pub(crate) mod windowing;
mod tray;
mod menu;
pub mod autostart;
pub mod notifications;
pub mod links;
pub mod instance;
pub mod shortcuts;
#[cfg(windows)]
mod windows;

use gpui_kit::{AnyWindowHandle, App, Global, Task};
use std::time::Duration;

/// 平台窗口与已选服务由唯一 Global 持有，不增加第二条事件循环。
pub struct Services {
    main:AnyWindowHandle,
    #[cfg(target_os="macos")]
    shell: gpui_kit::WeakEntity<crate::app::Shell>,
    #[cfg(target_os="macos")]
    window_host: Option<windowing::WindowHost>,
    #[cfg(target_os="macos")]
    window_revision: u64,
    _poll: Task<()>,
    #[cfg(windows)]
    window: Option<windows::MainWindow>,
    tray: Option<tray::Tray>,
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
    #[cfg(target_os="macos")]
    let window_host = shell.update(cx, |shell, _| shell.window_host()).ok().flatten();
    #[cfg(windows)]
    let native_window = handle.update(cx, |_, window, _| windows::MainWindow::capture(window)).ok().and_then(Result::ok);
    let _ = locale;
    #[cfg(not(target_os="macos"))]
    let _ = shell;
    #[cfg(windows)]
    let allows_tray = native_window.is_some();
    #[cfg(not(windows))]
    let allows_tray = true;
    let tray = if allows_tray { tray::Tray::new(locale).ok() } else { None };
    if tray.is_none() { tracing::warn!(event = "tray_unavailable"); }
    let autostart = autostart::Service::new();
    let mut revision = 0;
    let mut last_notification_key=None;
    let mut last_mouse_state=None;
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update(|cx| {
                if !cx.has_global::<Services>() { return }
                #[cfg(target_os="macos")]
                {
                    let service = cx.global_mut::<Services>();
                    let revision = service.window_host.as_ref().and_then(windowing::WindowHost::settled_revision);
                    if let Some(revision) = revision.filter(|revision| *revision != service.window_revision) {
                        service.window_revision = revision;
                        let shell = service.shell.clone();
                        let _ = handle.update(cx, |_, window, cx| { let _ = shell.update(cx, |shell, cx| shell.poll_native_window(window, cx)); });
                    }
                }
                if receiver.try_iter().take(64).count() > 0 { show(handle, cx); }
                let events = cx.global::<Services>().tray.as_ref().map(|tray| tray.events()).unwrap_or_default();
                for event in events { match event { tray::Event::Show => show(handle, cx), tray::Event::Quit => cx.quit(), tray::Event::TogglePet => crate::pet::toggle(cx), tray::Event::OpenPetSettings => crate::pet::open_settings(cx), tray::Event::ToggleMousePassthrough => crate::pet::toggle_mouse_passthrough(cx), tray::Event::ToggleNotification=> {let enabled=notification_state(cx).enabled;set_notification(!enabled,cx);} } }
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
                if current != revision { revision = current; menu::refresh(cx); cx.refresh_windows(); }
                let handlers = cx.global::<Services>().shortcuts.pending();
                for dispatch in handlers { dispatch(cx); }
            });
        }
    });
    cx.set_global(Services {
        main:handle,
        #[cfg(target_os="macos")]
        shell,
        #[cfg(target_os="macos")]
        window_host,
        #[cfg(target_os="macos")]
        window_revision: 0,
        _poll: poll,
        #[cfg(windows)]
        window: native_window,
        tray,
        autostart,
        notifications,
        _instance: instance,
        shortcuts: shortcuts::Service::new(),
    });
    cx.on_app_quit(|cx| {
        let services = cx.remove_global::<Services>();
        async move { drop(services); }
    }).detach();
}

/// 主动显示统一捕获入口鼠标屏；原生修改由前台 task 在借用之外执行。
pub fn show(handle: AnyWindowHandle, cx: &mut App) {
    #[cfg(all(target_os="macos", not(test)))]
    {
        let desktop = windowing::capture_desktop(cx);
        let Some(shell) = cx.try_global::<Services>().map(|service| service.shell.clone()) else { return; };
        match desktop {
            Ok(desktop) => show_on_desktop(handle, desktop, cx),
            Err(_) => {
                tracing::warn!(event="window_desktop_unavailable");
                let host = handle.update(cx, |_, window, cx| shell.update(cx, |shell, cx| shell.prepare_window_hide(window, cx)).ok().flatten()).ok().flatten();
                cx.spawn(async move |cx| {
                    let live = cx.update(|cx| handle.update(cx, |_, _, _| ()).is_ok());
                    if live { if let Some(host) = host { host.show(); } }
                }).detach();
            }
        }
        return;
    }
    #[cfg(any(not(target_os="macos"), test))]
    {
    #[cfg(windows)]
    let native = cx.try_global::<Services>().and_then(|services| services.window);
    cx.defer(move |cx| {
        #[cfg(windows)]
        if let Some(window) = native { window.show(); }
        cx.activate(true);
        let _ = handle.update(cx, |_, window, _| window.activate_window());
    });
    }
}

/// 启动入口已捕获指针时直接传递，避免创建辅助窗之后改变本次选屏。
#[cfg(target_os="macos")]
pub(crate) fn show_on_desktop(handle: AnyWindowHandle, desktop: windowing::DesktopSnapshot, cx: &mut App) {
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
    if !cx.try_global::<Services>().is_some_and(|services| services.tray.is_some()) { return false }
    #[cfg(windows)]
    let native = cx.global::<Services>().window;
    let main=cx.global::<Services>().main;
    #[cfg(target_os="macos")]
    {
        let shell = cx.global::<Services>().shell.clone();
        let host = shell.update(cx, |shell, _| shell.cancel_window_restore()).ok().flatten();
        cx.spawn(async move |cx| {
            let live = cx.update(|cx| main.update(cx, |_, _, _| ()).is_ok());
            if live { if let Some(host) = host { host.hide(); } }
        }).detach();
        true
    }
    #[cfg(not(target_os="macos"))]
    {
    cx.defer(move |cx| {
        #[cfg(windows)]
        if let Some(window) = native { window.hide(); }
        #[cfg(not(windows))]
        let _=main.update(cx,|_,window,_|crate::pet::host::set_visible(window,false));
    });
    true
    }
}
/// 语言切换更新已创建的双语托盘菜单。
pub fn set_language(locale: &str, cx: &mut App) {
    menu::set_language(locale, cx);
    if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.set_language(locale);}
    crate::pet::set_language(locale,cx);
    if cx.has_global::<Services>() { if let Some(tray) = &mut cx.global_mut::<Services>().tray { tray.set_language(locale); } }
}
/// 设置视图仅读取内存快照，实际 OS 状态由后台 worker 更新。
pub fn autostart_state(cx: &App) -> autostart::State {
    cx.try_global::<Services>().map(|services| services.autostart.state()).unwrap_or_default()
}
/// 事件直接来自自启控件，初始化不会调用此 mutation。
pub fn set_autostart(enabled: bool, cx: &App) {
    if let Some(services) = cx.try_global::<Services>() { services.autostart.set(enabled); }
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
    // 系统静态域名借用有效；volatile 域不落盘且保留已有参数。
    let domain=unsafe {NSArgumentDomain};
    let current=defaults.volatileDomainForName(domain);
    let arguments=NSMutableDictionary::dictionaryWithDictionary(&current);
    let zero=NSNumber::new_i32(0);
    arguments.insert(ns_string!("AppleFontSmoothing"),zero.as_ref());
    // 值类型为 Foundation 支持的 NSNumber，仅修改当前进程搜索列表。
    unsafe {defaults.setVolatileDomain_forName(&arguments,domain);}
}

/// 托盘只按用户会话可见性改标签，不把系统遮挡误当作隐藏。
pub fn set_pet_visibility(visible:bool,cx:&mut App) {if cx.has_global::<Services>() {if let Some(tray)=cx.global_mut::<Services>().tray.as_mut() {tray.set_pet_visibility(visible);}}}

/// 设置页只读取应用级通知偏好，离页不丢失真实保存结果。
pub fn notification_state(cx:&App)->notifications::State {cx.try_global::<Services>().map(|services|services.notifications.state()).unwrap_or_default()}

/// 系统授权与偏好保存串行归属原生服务；初始化从不调用。
pub fn set_notification(enabled:bool,cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|services|services.notifications.clone()) {service.set(enabled,cx);}}

/// 激活或进入设置只触发有界只读权限查询。
pub fn refresh_notification_permission(cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.refresh_permission(cx);}}
/// 明确按钮重试由同一应用 owner 处理。
pub fn retry_notifications(cx:&mut App) {if let Some(service)=cx.try_global::<Services>().map(|s|s.notifications.clone()) {service.retry(cx);}}
