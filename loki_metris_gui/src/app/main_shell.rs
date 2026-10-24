//! 根视图对原生层 [`crate::native::MainShell`] 端口的实现：把宿主事件转入主窗状态逻辑。

use gpui_kit::WeakEntity;
#[cfg(any(target_os="macos", all(windows, not(test))))]
use gpui_kit::{App, Window};
#[cfg(any(target_os="macos", all(windows, not(test))))]
use crate::native::windowing::{DesktopSnapshot, WindowHost};

use super::Shell;

/// 原生层只持有根视图弱引用；根视图释放后各方法返回 None 或不产生副作用。
impl crate::native::MainShell for WeakEntity<Shell> {
    /// 读取根视图创建时捕获的主窗原生宿主。
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    fn window_host(&self, cx: &mut App) -> Option<WindowHost> {
        self.update(cx, |shell, _| shell.window_host()).ok().flatten()
    }

    /// 把新的宿主几何修订同步进主窗状态。
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    fn poll_native_window(&self, window: &Window, cx: &mut App) {
        let _ = self.update(cx, |shell, cx| shell.poll_native_window(window, cx));
    }

    /// 隐藏前由根视图取消恢复租约并交出宿主。
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    fn prepare_window_hide(&self, window: &Window, cx: &mut App) -> Option<WindowHost> {
        self.update(cx, |shell, cx| shell.prepare_window_hide(window, cx)).ok().flatten()
    }

    /// 按桌面快照请求根视图恢复主窗。
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    fn request_window_restore(&self, desktop: DesktopSnapshot, window: &Window, cx: &mut App) {
        let _ = self.update(cx, |shell, cx| shell.request_window_restore(desktop, window, cx));
    }

    /// 取消等待中的恢复并交出宿主。
    #[cfg(any(target_os="macos", all(windows, not(test))))]
    fn cancel_window_restore(&self, cx: &mut App) -> Option<WindowHost> {
        self.update(cx, |shell, _| shell.cancel_window_restore()).ok().flatten()
    }
}
