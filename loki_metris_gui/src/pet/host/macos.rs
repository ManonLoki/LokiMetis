//! AppKit仅在GPUI主线程借用活窗；坐标使用主显示器顶边统一换算。

use gpui_kit::{Bounds, Window, point, px, size};
use loki_metris_core::{PetSizeRange, PetWindowPreferences, pet_window_dimensions};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_app_kit::{NSScreen, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 每次从GPUI活窗重新取得保留引用，不跨线程保存裸指针。
fn native(window: &Window) -> anyhow::Result<Retained<NSWindow>> {
    anyhow::ensure!(
        MainThreadMarker::new().is_some(),
        "window operation must run on main thread"
    );
    let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .as_raw()
    else {
        anyhow::bail!("AppKit handle unavailable")
    };
    // SAFETY: GPUI在本调用期间拥有NSView，当前为Cocoa主线程；window()返回保留引用。
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    view.window()
        .ok_or_else(|| anyhow::anyhow!("NSView is not attached to a window"))
}
/// NSScreen列表首项固定为菜单栏主屏，与GPUI后端使用同一顶边。
fn top() -> f64 {
    let mtm = MainThreadMarker::new().expect("UI thread");
    NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
        .unwrap_or(1440.)
}
/// 当前窗口所在屏的可用区域，排除菜单栏和Dock。
pub fn work_area(window: &Window) -> Bounds<gpui_kit::Pixels> {
    native(window)
        .ok()
        .and_then(|window| window.screen())
        .map(|screen| {
            let rect = screen.visibleFrame();
            Bounds::new(
                point(
                    px(rect.origin.x as f32),
                    px((top() - rect.origin.y - rect.size.height) as f32),
                ),
                size(px(rect.size.width as f32), px(rect.size.height as f32)),
            )
        })
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))))
}
/// 活窗左上角的全局逻辑坐标，避免GPUI屏幕局部bounds被持久化。
pub fn bounds(window: &Window) -> Bounds<gpui_kit::Pixels> {
    native(window)
        .map(|window| {
            let rect = window.frame();
            Bounds::new(
                point(
                    px(rect.origin.x as f32),
                    px((top() - rect.origin.y - rect.size.height) as f32),
                ),
                size(px(rect.size.width as f32), px(rect.size.height as f32)),
            )
        })
        .unwrap_or_else(|_| window.bounds())
}
/// NSWindow的显示不改变主界面焦点，隐藏不影响同应用其它窗口。
pub fn set_visible(window: &Window, visible: bool) -> anyhow::Result<()> {
    let native = native(window)?;
    if visible {
        native.orderFrontRegardless()
    } else {
        native.orderOut(None)
    }
    Ok(())
}
/// 只移动左上角，不借用当前鼠标屏幕高度。
pub fn move_to(window: &Window, x: f64, y: f64) -> anyhow::Result<()> {
    let native = native(window)?;
    native.setFrameOrigin(NSPoint::new(x, top() - y - native.frame().size.height));
    Ok(())
}
/// 无边框／置顶／锁定在原窗口动态应用，不重建播放实体。
pub fn apply(window: &Window, prefs: PetWindowPreferences, settings: bool) -> anyhow::Result<()> {
    let native = native(window)?;
    let resizable = !prefs.locked && !settings;
    native.setStyleMask(if resizable {
        NSWindowStyleMask::Borderless | NSWindowStyleMask::Resizable
    } else {
        NSWindowStyleMask::Borderless
    });
    native.setLevel(if settings || prefs.always_on_top {
        3
    } else {
        0
    });
    native.setHasShadow(false);
    native.setHidesOnDeactivate(false);
    native.setMovableByWindowBackground(false);
    native.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
    if !settings {
        let area = work_area(window);
        let range = PetSizeRange::for_work_area(
            f64::from(f32::from(area.size.width.min(area.size.height))),
            prefs.layout,
        );
        let (min_w, min_h) = pet_window_dimensions(prefs.layout, range.min);
        let (max_w, max_h) = pet_window_dimensions(prefs.layout, range.max);
        native.setContentMinSize(NSSize::new(min_w, min_h));
        native.setContentMaxSize(NSSize::new(max_w, max_h));
    }
    Ok(())
}
/// macOS的原生拖动由GPUI当前鼠标事件驱动。
pub fn start_drag(window: &Window, cx: &gpui_kit::App) {
    if let Ok(native) = native(window) {
        if let Some(event) = native.currentEvent() {
            // 不持有GPUI后端锁或App/Entity借用跨越Cocoa同步模态拖动循环。
            cx.foreground_executor()
                .spawn(async move {
                    native.performWindowDragWithEvent(&event);
                })
                .detach();
        }
    }
}

/// 同一次原生frame事务修改尺寸和全局左上角，避免异步resize导致顶边漂移。
pub fn set_geometry(window: &mut Window, bounds: Bounds<gpui_kit::Pixels>) -> anyhow::Result<()> {
    let native = native(window)?;
    let width = f64::from(f32::from(bounds.size.width));
    let height = f64::from(f32::from(bounds.size.height));
    native.setFrame_display(
        NSRect::new(
            NSPoint::new(
                f64::from(f32::from(bounds.origin.x)),
                top() - f64::from(f32::from(bounds.origin.y)) - height,
            ),
            NSSize::new(width, height),
        ),
        true,
    );
    Ok(())
}

/// 按每块显示器自己的scale解释本产品保存的全局坐标，优先左上角所在屏。
pub fn restoration_area(
    window: &Window,
    stored: Option<loki_metris_core::PetOverlayPosition>,
    prefs: PetWindowPreferences,
) -> (Bounds<gpui_kit::Pixels>, f64, bool) {
    let fallback = (work_area(window), f64::from(window.scale_factor()), false);
    let Some(stored) = stored else {
        return fallback;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return fallback;
    };
    let mut candidate = None;
    for screen in NSScreen::screens(mtm).iter() {
        let rect = screen.visibleFrame();
        let scale = screen.backingScaleFactor();
        let area = Bounds::new(
            point(
                px(rect.origin.x as f32),
                px((top() - rect.origin.y - rect.size.height) as f32),
            ),
            size(px(rect.size.width as f32), px(rect.size.height as f32)),
        );
        let range =
            PetSizeRange::for_work_area(rect.size.width.min(rect.size.height), prefs.layout);
        let (width, height) = pet_window_dimensions(prefs.layout, range.constrain(prefs.pet_size));
        let physical = loki_metris_core::PetOverlayWorkArea {
            x: (rect.origin.x * scale).round() as i32,
            y: ((top() - rect.origin.y - rect.size.height) * scale).round() as i32,
            width: (rect.size.width * scale).round() as u32,
            height: (rect.size.height * scale).round() as u32,
        };
        if loki_metris_core::resolve_pet_overlay_position(
            Some(stored),
            (
                (width * scale).round() as u32,
                (height * scale).round() as u32,
            ),
            &[physical],
        )
        .is_some()
        {
            let inside = f64::from(stored.x) / scale >= rect.origin.x
                && f64::from(stored.x) / scale < rect.origin.x + rect.size.width
                && f64::from(stored.y) / scale >= top() - rect.origin.y - rect.size.height
                && f64::from(stored.y) / scale < top() - rect.origin.y;
            if candidate.is_none() || inside {
                candidate = Some((area, scale, true));
                if inside {
                    break;
                }
            }
        }
    }
    candidate.unwrap_or(fallback)
}
