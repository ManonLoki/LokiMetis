//! 虚拟测试保留同回合创建时序；真实目标平台由独立原生事务定位。

use super::super::{Controller, host, settings};
use gpui_kit::{App, AppContext, Bounds, Entity, WindowBounds, WindowOptions, base::Root, point, px, size};
use loki_metris_core::PetWindowPreferences;

/// 保留测试平台同回合创建时序和现有宿主行为，入口已由显示租约排除过期意图。
pub(super) fn open(owner: Entity<Controller>, cx: &mut App) {
    let area = owner.read(cx).overlay.and_then(|handle| {
        handle.update(cx, |_, window, _| host::work_area(window)).ok()
    }).unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    let origin = point(area.origin.x + (area.size.width - px(320.)) / 2.,
        area.origin.y + (area.size.height - px(470.)) / 2.);
    if let Some(handle) = owner.read(cx).settings_window
        && handle.update(cx, |_, window, cx| {
            if let Err(error) = host::move_to(window, f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y))) {
                tracing::warn!(%error, "pet settings recenter failed");
            }
            window.bounds_changed(cx);
            window.activate_window();
        }).is_ok() { return; }
    let controller = owner.clone();
    let result = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(320.), px(470.))))),
        window_min_size: Some(size(px(280.), px(440.))), titlebar: None,
        is_resizable: false, is_minimizable: false, ..Default::default()
    }, |window, cx| {
        let _ = host::apply(window, PetWindowPreferences::default(), true);
        let _ = host::move_to(window, f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y)));
        let view = cx.new(|cx| settings::Settings::new(controller, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    if let Ok(handle) = result {
        owner.update(cx, |owner, _| owner.settings_window = Some(handle.into()));
    }
}
