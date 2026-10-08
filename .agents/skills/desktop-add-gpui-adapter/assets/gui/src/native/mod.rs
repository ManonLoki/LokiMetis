//! 原生能力统一持有生命周期；平台对象在 UI 线程创建，后台输入通过有界轮询映射。

@@NATIVE_SUBMODULES@@
#[cfg(windows)]
mod windows;

use gpui_kit::{AnyWindowHandle, App, Global, Task};
use std::time::Duration;

/// 平台窗口与已选服务由唯一 Global 持有，不增加第二条事件循环。
pub struct Services {
    _poll: Task<()>,
    #[cfg(windows)]
    window: Option<windows::MainWindow>,
@@NATIVE_FIELDS@@
}
impl Global for Services {}

/// 主窗口创建后安装宿主机制；不注册登录项，不申请通知权限，不绑定默认热键。
pub fn install(handle: AnyWindowHandle, locale: &str, @@NATIVE_INSTANCE_ARGUMENTS@@ cx: &mut App) {
    #[cfg(windows)]
    let native_window = handle.update(cx, |_, window, _| windows::MainWindow::capture(window)).ok().and_then(Result::ok);
@@NATIVE_CREATE@@
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_millis(40)).await;
            cx.update(|cx| {
                if !cx.has_global::<Services>() { return }
@@NATIVE_POLL@@
            });
        }
    });
    cx.set_global(Services {
        _poll: poll,
        #[cfg(windows)]
        window: native_window,
@@NATIVE_DEFAULTS@@
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

@@NATIVE_TRAY_METHODS@@
@@NATIVE_AUTOSTART_METHODS@@
