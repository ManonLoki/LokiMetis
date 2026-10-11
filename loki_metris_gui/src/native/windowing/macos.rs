//! AppKit 主线程读取真实 screen/frame/content；全局原点保持点坐标，不乘设备 scale。

use super::{DesktopSnapshot, NativeDisplay, NativeWindowState};
use gpui_kit::{App, DisplayId, Window};
use loki_metris_core::window_placement::{WindowDipPoint, WindowDipRect, WindowDipSize, WindowDisplayArea, WindowFrameInsets, capture_window_state};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSEvent, NSScreen, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSNumber, ns_string};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 每次借用活窗重新获取保留引用，既不缓存裸指针也不跨线程调用 AppKit。
pub(super) fn native(window: &Window) -> anyhow::Result<Retained<NSWindow>> {
    anyhow::ensure!(MainThreadMarker::new().is_some(), "window_main_thread_required");
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
        .map_err(|_| anyhow::anyhow!("window_handle_unavailable"))?.as_raw() else {
        anyhow::bail!("window_appkit_handle_unavailable");
    };
    // 安全：GPUI 在借用期间持有 NSView，AppKit 操作限于当前主线程；window 返回保留引用。
    unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() }.window()
        .ok_or_else(|| anyhow::anyhow!("window_detached"))
}

/// NSScreenNumber 与 GPUI DisplayId 均是运行期 CGDirectDisplayID，不能持久化该数值。
fn screen_id(screen: &NSScreen) -> anyhow::Result<DisplayId> {
    let number = screen.deviceDescription().objectForKey(ns_string!("NSScreenNumber"))
        .ok_or_else(|| anyhow::anyhow!("window_screen_id_unavailable"))?
        .downcast::<NSNumber>().map_err(|_| anyhow::anyhow!("window_screen_id_invalid"))?;
    Ok(DisplayId::new(number.unsignedLongLongValue()))
}

/// 当前屏来自活窗原生 screen，不能把保存屏再次当作当前屏。
pub(crate) fn window_display_key(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<String> {
    let screen = native(window)?.screen().ok_or_else(|| anyhow::anyhow!("window_screen_unavailable"))?;
    let id = screen_id(&screen)?;
    desktop.displays.iter().find(|display| display.id == id).map(|display| display.area.key.clone())
        .ok_or_else(|| anyhow::anyhow!("window_screen_mapping_unavailable"))
}

/// 入口统一捕获原生拓扑和指针；缺失真实屏幕时明确失败，不生成假显示器。
pub(crate) fn capture_desktop(cx: &App) -> anyhow::Result<DesktopSnapshot> {
    capture_topology_with_pointer(cx, Some(NSEvent::mouseLocation()))
}

/// 等待系统过渡后只刷新拓扑，入口选屏值由动作持有，不重新读取鼠标。
pub(crate) fn capture_topology(cx: &App) -> anyhow::Result<DesktopSnapshot> {
    capture_topology_with_pointer(cx, None)
}

/// 原生全局坐标只在平台边界使用；被拔除屏幕由上层重新调用 core 回退。
fn capture_topology_with_pointer(cx: &App, pointer: Option<objc2_foundation::NSPoint>) -> anyhow::Result<DesktopSnapshot> {
    let mtm = MainThreadMarker::new().ok_or_else(|| anyhow::anyhow!("window_main_thread_required"))?;
    let screens = NSScreen::screens(mtm);
    let primary = screens.firstObject().ok_or_else(|| anyhow::anyhow!("window_displays_unavailable"))?;
    let primary_frame = primary.frame();
    let global_top = primary_frame.origin.y + primary_frame.size.height;
    let primary_id = screen_id(&primary)?;
    let gpui_displays = cx.displays();
    let mut displays = Vec::new();
    let mut pointer_key = None;
    for screen in screens.iter() {
        let id = screen_id(&screen)?;
        let display = gpui_displays.iter().find(|display| display.id() == id)
            .ok_or_else(|| anyhow::anyhow!("window_screen_mapping_unavailable"))?;
        let key = display.uuid().map_err(|_| anyhow::anyhow!("window_screen_identity_unavailable"))?.to_string();
        anyhow::ensure!(!displays.iter().any(|display: &NativeDisplay| display.area.key == key), "window_screen_identity_ambiguous");
        let frame = screen.frame();
        let visible = screen.visibleFrame();
        let global_origin = WindowDipPoint { x: frame.origin.x, y: global_top - frame.origin.y - frame.size.height };
        let area = WindowDisplayArea {
            key: key.clone(), primary: id == primary_id, scale: screen.backingScaleFactor(),
            work_area: WindowDipRect {
                origin: WindowDipPoint { x: visible.origin.x - frame.origin.x, y: frame.origin.y + frame.size.height - visible.origin.y - visible.size.height },
                size: WindowDipSize { width: visible.size.width, height: visible.size.height },
            },
        };
        if pointer.is_some_and(|pointer| pointer.x >= frame.origin.x && pointer.x < frame.origin.x + frame.size.width
            && pointer.y >= frame.origin.y && pointer.y < frame.origin.y + frame.size.height) {
            pointer_key = Some(key);
        }
        displays.push(NativeDisplay { id, area, global_origin });
    }
    Ok(DesktopSnapshot { displays, pointer: pointer_key, global_top })
}

/// 用实际原生 frame/content 之差取得边框，不将标题栏计入内容持久尺寸。
pub(crate) fn frame_insets(window: &Window) -> anyhow::Result<WindowFrameInsets> {
    let native = native(window)?;
    let frame = native.frame();
    let content = native.contentRectForFrameRect(frame);
    Ok(WindowFrameInsets {
        left: content.origin.x - frame.origin.x,
        right: frame.origin.x + frame.size.width - content.origin.x - content.size.width,
        bottom: content.origin.y - frame.origin.y,
        top: frame.origin.y + frame.size.height - content.origin.y - content.size.height,
    })
}

/// 最终读回普通外框位置及内容尺寸；zoom/fullscreen/miniaturized 不覆盖普通矩形。
pub(crate) fn capture_window(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<NativeWindowState> {
    let native = native(window)?;
    let maximized = native.isZoomed();
    let transient = native.isMiniaturized() || native.styleMask().contains(NSWindowStyleMask::FullScreen);
    if maximized || transient { return Ok(NativeWindowState { normal: None, maximized, transient }); }
    Ok(NativeWindowState { normal: Some(capture_normal(window, desktop)?), maximized, transient })
}

/// 显式普通几何阶段即使恰好等于系统 zoom 矩形，也保存真正应用的普通内容尺寸。
pub(crate) fn capture_normal(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<loki_metris_core::window_placement::WindowStateV2> {
    let native = native(window)?;
    let screen = native.screen().ok_or_else(|| anyhow::anyhow!("window_screen_unavailable"))?;
    let id = screen_id(&screen)?;
    let display = desktop.displays.iter().find(|display| display.id == id)
        .ok_or_else(|| anyhow::anyhow!("window_screen_mapping_unavailable"))?;
    let frame = native.frame();
    let content = native.contentRectForFrameRect(frame);
    let origin = WindowDipPoint {
        x: frame.origin.x - display.global_origin.x,
        y: desktop.global_top - frame.origin.y - frame.size.height - display.global_origin.y,
    };
    let normal = capture_window_state(&display.area, origin,
        WindowDipSize { width: content.size.width, height: content.size.height }, frame_insets(window)?, false)
        .ok_or_else(|| anyhow::anyhow!("window_geometry_invalid"))?;
    Ok(normal)
}
