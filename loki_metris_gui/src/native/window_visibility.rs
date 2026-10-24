//! 主窗与辅助窗共用的原生显隐驱动：map/unmap 当前活窗，不把 activate 当作显示。
//!
//! 只有 Linux X11/XWayland 与非目标平台需要本驱动；macOS、Windows 的显隐由各自
//! 窗口宿主负责。测试构建统一替换为可观察、可注入失败的线程内替身。

/// 托盘显示和隐藏命令实际 map/unmap 当前活窗；纯 Wayland 无法捕获 X11 句柄时返回错误。
#[cfg(all(target_os = "linux", not(test)))]
pub(crate) fn set_visible(window: &gpui_kit::Window, visible: bool) -> anyhow::Result<()> {
    super::linux::X11Window::capture(window)?.set_visible(visible)
}

/// 当前产品目标平台之外不假报原生窗口显隐能力。
#[cfg(all(not(any(windows, target_os = "macos", target_os = "linux")), not(test)))]
pub(crate) fn set_visible(_: &gpui_kit::Window, _: bool) -> anyhow::Result<()> {
    anyhow::bail!("auxiliary windows unsupported")
}

#[cfg(test)]
pub(crate) use test_double::*;

#[cfg(test)]
mod test_double {
    use gpui_kit::Window;
    std::thread_local! {
        /// 仅虚拟宿主记录原生显隐命令，不代表真实操作系统可见性。
        static VISIBILITY: std::cell::RefCell<std::collections::VecDeque<(gpui_kit::AnyWindowHandle, bool)>> = Default::default();
        /// 下一次显隐请求失败，验证主窗恢复不能把宿主失败当成已显示。
        static VISIBILITY_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    /// 单次显隐失败只作用于当前测试线程，不影响后续恢复请求；仅 Linux 主窗恢复回归使用。
    #[cfg(target_os = "linux")]
    pub(crate) fn fail_next_visibility() { VISIBILITY_FAILURE.with(|failure| failure.set(true)); }
    /// 取出当前测试活窗的命令记录，有界缓存防止其它用例无限累积。
    pub(crate) fn take_visibility_events(handle: gpui_kit::AnyWindowHandle) -> Vec<bool> {
        VISIBILITY.with(|events| {
            let mut events = events.borrow_mut();
            let found = events.iter().filter(|(window, _)| *window == handle).map(|(_, visible)| *visible).collect();
            events.retain(|(window, _)| *window != handle);
            found
        })
    }
    /// 假平台只验证GPUI实体/窗口时序，实际原生属性由Computer Use检查。
    pub(crate) fn set_visible(window: &Window, visible: bool) -> anyhow::Result<()> {
        if VISIBILITY_FAILURE.with(|failure| failure.replace(false)) { anyhow::bail!("injected native visibility failure"); }
        VISIBILITY.with(|events| {
            let mut events = events.borrow_mut();
            if events.len() >= 64 { events.pop_front(); }
            events.push_back((window.window_handle(), visible));
        });
        Ok(())
    }
}
