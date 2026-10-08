//! 原生能力统一持有生命周期；平台对象在 UI 线程创建，后台输入通过有界轮询映射。

mod tray;
mod menu;
pub mod autostart;
pub mod instance;
pub mod shortcuts;
#[cfg(windows)]
mod windows;

use gpui_kit::{AnyWindowHandle, App, Global, Task};
use std::time::Duration;

/// 平台窗口与已选服务由唯一 Global 持有，不增加第二条事件循环。
pub struct Services {
    _poll: Task<()>,
    #[cfg(windows)]
    window: Option<windows::MainWindow>,
    tray: Option<tray::Tray>,
    pub autostart: autostart::Service,
    _instance: instance::Instance,
    pub shortcuts: shortcuts::Service,
}
impl Global for Services {}

/// 主窗口创建后安装宿主机制；不注册登录项，不申请通知权限，不绑定默认热键。
pub fn install(handle: AnyWindowHandle, locale: &str, instance: instance::Instance, receiver: std::sync::mpsc::Receiver<()>,  cx: &mut App) {
    menu::install(locale, cx);
    #[cfg(windows)]
    let native_window = handle.update(cx, |_, window, _| windows::MainWindow::capture(window)).ok().and_then(Result::ok);
    let _ = locale;
    #[cfg(windows)]
    let allows_tray = native_window.is_some();
    #[cfg(not(windows))]
    let allows_tray = true;
    let tray = if allows_tray { tray::Tray::new(locale).ok() } else { None };
    if tray.is_none() { tracing::warn!(event = "tray_unavailable"); }
    let autostart = autostart::Service::new();
    let mut revision = 0;
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update(|cx| {
                if !cx.has_global::<Services>() { return }
                if receiver.try_iter().take(64).count() > 0 { show(handle, cx); }
                let events = cx.global::<Services>().tray.as_ref().map(|tray| tray.events()).unwrap_or_default();
                for event in events { match event { tray::Event::Show => show(handle, cx), tray::Event::Quit => cx.quit() } }
                let current = cx.global::<Services>().autostart.state().revision;
                if current != revision { revision = current; cx.refresh_windows(); }
                let handlers = cx.global::<Services>().shortcuts.pending();
                for dispatch in handlers { dispatch(cx); }
            });
        }
    });
    cx.set_global(Services {
        _poll: poll,
        #[cfg(windows)]
        window: native_window,
        tray,
        autostart,
        _instance: instance,
        shortcuts: shortcuts::Service::new(),
    });
    cx.on_app_quit(|cx| {
        let services = cx.remove_global::<Services>();
        async move { drop(services); }
    }).detach();
}

/// 恢复在当前 App 更新完成后执行，避免原生窗口回调重入借用。
pub fn show(handle: AnyWindowHandle, cx: &mut App) {
    #[cfg(windows)]
    let native = cx.try_global::<Services>().and_then(|services| services.window);
    cx.defer(move |cx| {
        #[cfg(windows)]
        if let Some(window) = native { window.show(); }
        cx.activate(true);
        let _ = handle.update(cx, |_, window, _| window.activate_window());
    });
}

/// 托盘实际成功才隐藏，失败允许正常关闭，不留下无法恢复的进程。
pub fn hide_if_available(cx: &mut App) -> bool {
    if !cx.try_global::<Services>().is_some_and(|services| services.tray.is_some()) { return false }
    #[cfg(windows)]
    let native = cx.global::<Services>().window;
    cx.defer(move |cx| {
        #[cfg(windows)]
        if let Some(window) = native { window.hide(); }
        #[cfg(not(windows))]
        cx.hide();
    });
    true
}
/// 语言切换更新已创建的双语托盘菜单。
pub fn set_language(locale: &str, cx: &mut App) {
    menu::set_language(locale, cx);
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
