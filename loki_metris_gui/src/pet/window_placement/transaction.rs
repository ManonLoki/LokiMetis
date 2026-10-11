//! 原生外框和显示操作在全部 GPUI 借用之外执行，只接受匹配目标的最终读回。

use super::{Controller, Request, current, finish};
use crate::native::windowing::{self, WindowHost};
use super::plan::{placement, legacy_state, matches};
use gpui_kit::{AnyWindowHandle, App, Entity};

/// 捕获保留的原生对象后释放窗口借用，有界等待仅用于确认原生几何。
pub(super) fn begin(owner: Entity<Controller>, request: Request, handle: AnyWindowHandle, cx: &mut App) {
    let host = handle.update(cx, |_, window, _| WindowHost::capture(window)).ok().and_then(Result::ok);
    let Some(host) = host else { finish(&owner, &request, handle, false, false, cx); return; };
    let task_owner = owner.clone();
    let generation = request.generation;
    let task = cx.spawn(async move |cx| {
        let owner = task_owner;
        let live = |cx: &mut gpui_kit::AsyncApp| cx.update(|cx| current(&owner, generation, Some(handle), request.visible, cx));
        if !live(cx) { return; }
        let (prefs, stored, position_version, restored, confirmed) = cx.update(|cx| {
            let owner = owner.read(cx);
            (owner.settings.pet_window, owner.settings.pet_overlay_position, owner.settings.pet_overlay_position_version,
                owner.geometry_restored, owner.display_state.clone())
        });
        // 先应用真实无边框样式，再读取边距；初始 GPUI style 可能仍带标题栏位。
        #[cfg(target_os = "macos")]
        host.configure_auxiliary(!prefs.locked, prefs.always_on_top);
        #[cfg(windows)]
        if host.configure_auxiliary(!prefs.locked, prefs.always_on_top).is_err() {
            cx.update(|cx| finish(&owner, &request, handle, false, false, cx)); return;
        }
        if !live(cx) { return; }
        if !request.visible { host.hide(); }
        if !live(cx) { return; }
        let prepared = cx.update(|cx| handle.update(cx, |_, window, cx| {
            let mut desktop = windowing::capture_topology(cx).ok()?;
            desktop.pointer = request.pointer.clone();
            let insets = windowing::frame_insets(window).ok()?;
            let saved = confirmed.clone().or_else(|| (!restored).then(|| legacy_state(&desktop, prefs, stored, position_version, insets)).flatten());
            let current = windowing::window_display_key(window, &desktop).ok();
            let (plan, range) = placement(&desktop, prefs, saved.as_ref(), current.as_deref(), insets)?;
            Some((desktop, plan, range))
        }).ok().flatten());
        if !live(cx) { return; }
        let Some((desktop, plan, range)) = prepared else {
            tracing::warn!(event = "pet_window_placement_unavailable");
            if request.reveal { host.show(); }
            cx.update(|cx| finish(&owner, &request, handle, false, request.reveal, cx));
            return;
        };
        #[cfg(windows)]
        let _ = range;
        #[cfg(windows)]
        let (desktop, plan, range) = {
            if host.prime_display(&desktop, &plan).is_err() {
                cx.update(|cx| finish(&owner, &request, handle, false, false, cx)); return;
            }
            if !live(cx) { return; }
            let corrected = cx.update(|cx| handle.update(cx, |_, window, cx| {
                let mut refreshed = windowing::capture_topology(cx).ok()?;
                refreshed.pointer = desktop.pointer.clone();
                let insets = windowing::frame_insets(window).ok()?;
                let saved = confirmed.or_else(|| (!restored).then(|| legacy_state(&refreshed, prefs, stored, position_version, insets)).flatten());
                let current = windowing::window_display_key(window, &refreshed).ok();
                let (plan, range) = placement(&refreshed, prefs, saved.as_ref(), current.as_deref(), insets)?;
                Some((refreshed, plan, range))
            }).ok().flatten());
            let Some(corrected) = corrected else {
                cx.update(|cx| finish(&owner, &request, handle, false, false, cx)); return;
            };
            corrected
        };
        // 设置回执或退出可在原生回调中进入，因此每个操作之前均重新验证。
        #[cfg(target_os = "macos")]
        host.auxiliary_size_limits(super::super::geometry::dimensions(prefs.layout, range.min),
            super::super::geometry::dimensions(prefs.layout, range.max));
        #[cfg(windows)]
        let _ = range;
        if !live(cx) { return; }
        let applied = host.apply_auxiliary(&desktop, &plan).ok();
        let plan = applied.as_ref().unwrap_or(&plan);
        let readback = |cx: &mut gpui_kit::AsyncApp| cx.update(|cx| handle.update(cx, |_, window, _| {
            windowing::capture_normal(window, &desktop).ok()
        }).ok().flatten());
        let mut actual = readback(cx);
        let mut turns = 0;
        while applied.is_some() && !actual.as_ref().is_some_and(|actual| matches(actual, plan)) && turns < 8 {
            cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            if !live(cx) { return; }
            actual = readback(cx);
            turns += 1;
        }
        if !live(cx) { return; }
        // 已在途偏好回执若改变属性，续用同一入口重做，而不显示旧设置。
        let unchanged = cx.update(|cx| owner.read(cx).settings.pet_window == prefs);
        if !unchanged {
            cx.update(|cx| owner.update(cx, |owner, cx| owner.launch_display(cx)));
            return;
        }
        let passthrough = cx.update(|cx| owner.update(cx, |owner, cx| owner.apply_mouse_passthrough(cx).is_ok()));
        if !live(cx) { return; }
        let verified = applied.is_some() && actual.as_ref().is_some_and(|actual| matches(actual, plan)) && passthrough;
        tracing::info!(event = "pet_window_placement", verified, applied = applied.is_some(),
            readback_turns = turns, target_is_pointer = desktop.pointer.as_deref() == Some(plan.display_key.as_str()),
            display_matches = actual.as_ref().is_some_and(|actual| actual.display_key == plan.display_key));
        // 失败仍保留托盘可恢复的真实浮窗，但不宣称定位成功或保存中间坐标。
        if request.reveal { host.show(); }
        if !live(cx) { return; }
        cx.update(|cx| {
            if verified {
                owner.update(cx, |owner, _| {
                    if owner.settings_ready || owner.geometry_restored { owner.display_state = actual; }
                });
            }
            finish(&owner, &request, handle, verified, request.reveal, cx);
        });
    });
    owner.update(cx, |owner, _| owner.display_task = Some(task));
}

