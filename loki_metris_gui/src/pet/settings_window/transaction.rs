//! 原生定位与显示在全部 GPUI 借用之外执行；首帧显式选择入口屏。

use super::{Controller, Request, current};
use crate::native::windowing::{self, DesktopSnapshot, WindowHost};
use gpui_kit::{AnyWindowHandle, App, AppContext, Bounds, Entity, WindowBounds, WindowOptions, base::Root, px, size};
use loki_metris_core::window_placement::{WindowDisplaySelection, WindowFrameInsets, WindowPlacement, WindowPlacementRequest, WindowPlacementRole, restore_window_placement};

/// 创建时使用屏内 DIP 与运行期 display_id，已有窗口也重新执行选屏事务。
pub(super) fn open(owner: Entity<Controller>, request: Request, cx: &mut App) {
    let initial = request.desktop.as_ref().and_then(|desktop| {
        let placement = placement(desktop, None, WindowFrameInsets::default())?;
        let display = desktop.display(&placement.display_key)?;
        Some((WindowBounds::Windowed(windowing::placement_bounds(desktop, &placement)?), display.id))
    });
    let existing = owner.read(cx).settings_window.filter(|handle| handle.update(cx, |_, _, _| ()).is_ok());
    let handle = if let Some(handle) = existing { handle } else {
        let controller = owner.clone();
        let result = cx.open_window(WindowOptions {
            window_bounds: Some(initial.as_ref().map(|(bounds, _)| *bounds).unwrap_or_else(||
                WindowBounds::Windowed(Bounds::centered(None, size(px(320.), px(470.)), cx)))),
            display_id: initial.map(|(_, id)| id),
            window_min_size: Some(size(px(280.), px(440.))), titlebar: None,
            show: false, focus: false, is_resizable: false, is_minimizable: false,
            ..Default::default()
        }, |window, cx| {
            let view = cx.new(|cx| super::super::settings::Settings::new(controller, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        });
        let Ok(handle) = result else {
            tracing::warn!(event = "pet_settings_window_create_failed"); return;
        };
        let handle = handle.into();
        owner.update(cx, |owner, _| owner.settings_window = Some(handle));
        handle
    };
    let host = handle.update(cx, |_, window, _| WindowHost::capture(window)).ok().and_then(Result::ok);
    let generation = request.generation;
    cx.spawn(async move |cx| {
        // 原生方法可能同步重入事件循环，每步重新核对精确窗口，随后释放全部借用。
        let live = |cx: &mut gpui_kit::AsyncApp| cx.update(|cx| {
            current(&owner, generation, Some(handle), cx) && handle.update(cx, |_, _, _| ()).is_ok()
        });
        if !live(cx) { return; }
        let Some(host) = host else {
            tracing::warn!(event = "pet_settings_window_host_unavailable");
            discard_unavailable(&owner, generation, handle, cx); return;
        };
        #[cfg(target_os = "macos")]
        host.configure_auxiliary(false, true);
        #[cfg(windows)]
        if host.configure_auxiliary(false, true).is_err() {
            discard_unavailable(&owner, generation, handle, cx); return;
        }
        if !live(cx) { return; }
        let prepared = cx.update(|cx| handle.update(cx, |_, window, cx| {
            let mut desktop = windowing::capture_topology(cx).ok()?;
            desktop.pointer = request.desktop.and_then(|desktop| desktop.pointer);
            let current = windowing::window_display_key(window, &desktop).ok();
            let plan = placement(&desktop, current.as_deref(), windowing::frame_insets(window).ok()?)?;
            Some((desktop, plan))
        }).ok().flatten());
        if !live(cx) { return; }
        let Some((desktop, plan)) = prepared else {
            tracing::warn!(event = "pet_settings_window_placement_unavailable");
            host.show();
            if live(cx) { cx.update(|cx| cx.activate(true)); }
            return;
        };
        #[cfg(windows)]
        let (desktop, plan) = {
            if host.prime_display(&desktop, &plan).is_err() {
                tracing::warn!(event = "pet_settings_window_prime_failed");
                discard_unavailable(&owner, generation, handle, cx); return;
            }
            if !live(cx) { return; }
            let corrected = cx.update(|cx| handle.update(cx, |_, window, cx| {
                let mut refreshed = windowing::capture_topology(cx).ok()?;
                refreshed.pointer = desktop.pointer.clone();
                let current = windowing::window_display_key(window, &refreshed).ok();
                let corrected = placement(&refreshed, current.as_deref(), windowing::frame_insets(window).ok()?)?;
                Some((refreshed, corrected))
            }).ok().flatten());
            let Some(corrected) = corrected else {
                tracing::warn!(event = "pet_settings_window_replan_failed");
                discard_unavailable(&owner, generation, handle, cx); return;
            };
            corrected
        };
        if !live(cx) { return; }
        let native_zoomed = host.maximized();
        let applied_plan = host.apply_auxiliary(&desktop, &plan).ok();
        let applied = applied_plan.is_some();
        let plan = applied_plan.unwrap_or(plan);
        if !live(cx) { return; }
        let mut actual = cx.update(|cx| handle.update(cx, |_, window, _| {
            windowing::capture_normal(window, &desktop).ok()
        }).ok().flatten());
        let mut readback_turns = 0;
        while applied && readback_turns < 8 && !actual.as_ref().is_some_and(|actual| match_geometry(actual, &plan) == (true, true, true)) {
            cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            if !live(cx) { return; }
            actual = cx.update(|cx| handle.update(cx, |_, window, _| windowing::capture_normal(window, &desktop).ok()).ok().flatten());
            readback_turns += 1;
        }
        let (display_matches, origin_matches, size_matches) = actual.as_ref()
            .map(|actual| match_geometry(actual, &plan)).unwrap_or((false, false, false));
        let verified = applied && display_matches && origin_matches && size_matches;
        if !live(cx) { return; }
        tracing::info!(event = "pet_settings_window_placement", verified,
            applied, native_zoomed, readable = actual.is_some(), display_matches, origin_matches, size_matches,
            readback_turns,
            target_is_pointer = desktop.pointer.as_deref() == Some(plan.display_key.as_str()),
            target_is_primary = desktop.display(&plan.display_key).is_some_and(|display| display.area.primary));
        // 失败仍保留可恢复的设置入口；实际读回失败不会被日志记作定位通过。
        host.show();
        if live(cx) { cx.update(|cx| cx.activate(true)); }
    }).detach();
}

/// 只在尚未匹配时让事件循环完成读回，不以经过时间替代几何成功。
fn match_geometry(actual: &loki_metris_core::window_placement::WindowStateV2, plan: &WindowPlacement) -> (bool, bool, bool) {
    let tolerance = 0.5 / actual.scale.max(1.);
    (actual.display_key == plan.display_key,
        (actual.origin.x - plan.origin.x).abs() <= tolerance && (actual.origin.y - plan.origin.y).abs() <= tolerance,
        (actual.content_size.width - plan.content_size.width).abs() <= tolerance && (actual.content_size.height - plan.content_size.height).abs() <= tolerance)
}

/// 位置与最小屏回退由 core 决策，配置窗不读取主窗或桌宠持久几何。
fn placement(desktop: &DesktopSnapshot, current: Option<&str>, frame_insets: WindowFrameInsets) -> Option<WindowPlacement> {
    restore_window_placement(&desktop.areas(), WindowDisplaySelection {
        pointer: desktop.pointer.as_deref(), current,
    }, WindowPlacementRequest {
        role: WindowPlacementRole::PetConfiguration, saved: None, legacy_content_size: None,
        legacy_maximized: false, frame_insets,
    })
}

/// 无法取得原生对象时撤销本窗，保留桌宠入口供重试；不排入无法复核租约的二次激活。
fn discard_unavailable(owner: &Entity<Controller>, generation: u64, handle: AnyWindowHandle, cx: &mut gpui_kit::AsyncApp) {
    cx.update(|cx| {
        if current(owner, generation, Some(handle), cx) {
            owner.update(cx, |owner, _| owner.settings_window_closed(handle));
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    });
}
