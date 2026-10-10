//! 配置窗的显示租约独立于偏好读取，创建不持有 controller 的可变借用。

#[cfg(all(target_os = "macos", not(test)))]
mod macos;
#[cfg(any(not(target_os = "macos"), test))]
mod legacy;
#[cfg(test)]
mod tests;

use super::Controller;
use gpui_kit::{AnyWindowHandle, App, Entity};

/// 一个用户打开动作只捕获一次指针屏；defer 与前台 task 复用这份入口事实。
pub(super) struct Request {
    generation: u64,
    #[cfg(all(target_os = "macos", not(test)))]
    desktop: Option<crate::native::windowing::DesktopSnapshot>,
}

impl Request {
    /// 入口仅复制原生拓扑，不创建窗口或修改其它实体。
    pub(super) fn capture(generation: u64, cx: &App) -> Self {
        #[cfg(any(not(target_os = "macos"), test))]
        let _ = cx;
        Self {
            generation,
            #[cfg(all(target_os = "macos", not(test)))]
            desktop: crate::native::windowing::capture_desktop(cx).map_err(|_| {
                tracing::warn!(event = "pet_settings_desktop_unavailable");
            }).ok(),
        }
    }
}

/// 关闭先于 defer 执行时不产生新窗口，连续打开只执行最新意图。
pub(super) fn open(owner: Entity<Controller>, request: Request, cx: &mut App) {
    if !current(&owner, request.generation, None, cx) { return; }
    #[cfg(all(target_os = "macos", not(test)))]
    macos::open(owner, request, cx);
    #[cfg(any(not(target_os = "macos"), test))]
    legacy::open(owner, cx);
}

/// 精确句柄与代次共同限定显示租约，旧窗的迟到回调不能操作重建的新窗。
fn current(owner: &Entity<Controller>, generation: u64, handle: Option<AnyWindowHandle>, cx: &App) -> bool {
    let owner = owner.read(cx);
    !owner.quitting && owner.settings_window_generation == generation
        && handle.is_none_or(|handle| owner.settings_window == Some(handle))
}
