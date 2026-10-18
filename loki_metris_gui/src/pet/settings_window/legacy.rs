//! Linux 与虚拟测试共用设置窗生命周期；原生属性必须确认后才保留窗口。

use super::super::{Controller, host, settings};
use gpui_kit::{AnyWindowHandle, App, AppContext, Bounds, Entity, WindowBounds, WindowOptions, base::Root, point, px, size};
use loki_metris_core::PetWindowPreferences;

/// 同回合创建不重借用 controller，原生失败由既有错误面呈现并销毁未完成的新窗。
pub(super) fn open(owner: Entity<Controller>, cx: &mut App) {
    let area = owner.read(cx).overlay.and_then(|handle| {
        handle.update(cx, |_, window, _| host::work_area(window)).ok()
    }).unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    let origin = point(area.origin.x + (area.size.width - px(320.)) / 2.,
        area.origin.y + (area.size.height - px(470.)) / 2.);
    if let Some(handle) = owner.read(cx).settings_window
    {
        match handle.update(cx, |_, window, cx| -> anyhow::Result<()> {
            host::apply(window, PetWindowPreferences::default(), true)?;
            host::move_to(window, f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y)))?;
            host::set_visible(window, true)?;
            window.bounds_changed(cx);
            window.activate_window();
            Ok(())
        }) {
            Ok(Ok(())) => {
                confirm(&owner, handle, cx);
                return;
            }
            Ok(Err(error)) => { failed(&owner, error, cx); return; }
            Err(_) => owner.update(cx, |owner, _| owner.settings_window = None),
        }
    }
    let controller = owner.clone();
    let result = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(origin, size(px(320.), px(470.))))),
        window_min_size: Some(size(px(280.), px(440.))), titlebar: None,
        is_resizable: false, is_minimizable: false, show: false,
        app_id: Some("com.loki-metris.app".into()), ..Default::default()
    }, |window, cx| {
        let view = cx.new(|cx| settings::Settings::new(controller, window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    match result {
        Ok(handle) => {
            let applied = handle.update(cx, |_, window, cx| -> anyhow::Result<()> {
                host::apply(window, PetWindowPreferences::default(), true)?;
                host::move_to(window, f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y)))?;
                host::set_visible(window, true)?;
                window.bounds_changed(cx);
                window.activate_window();
                Ok(())
            }).and_then(|result| result);
            match applied {
                Ok(()) => owner.update(cx, |owner, cx| {
                    owner.settings_window = Some(handle.into());
                    cx.notify();
                }),
                Err(error) => {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                    failed(&owner, error, cx);
                }
            }
            if owner.read(cx).settings_window == Some(handle.into()) { confirm(&owner, handle.into(), cx); }
        }
        Err(error) => failed(&owner, error, cx),
    }
}

/// 设置窗拥有独立异步确认任务，关闭撤销租约，失败不会留存未确认的入口。
fn confirm(owner: &Entity<Controller>, handle: AnyWindowHandle, cx: &mut App) {
    let generation = owner.read(cx).settings_window_generation;
    let preference_generation = owner.read(cx).settings_generation;
    let weak = owner.downgrade();
    let task = cx.spawn(async move |cx| {
        let result = super::super::native_settle::wait(&weak, handle,
            super::super::native_settle::Lease::Settings(generation), cx).await;
        if matches!(result, Ok(false)) { return; }
        cx.update(|cx| {
            let Some(owner) = weak.upgrade() else { return; };
            let current = owner.read(cx).settings_window == Some(handle)
                && owner.read(cx).settings_window_generation == generation;
            if !current { return; }
            let failed = result.is_err();
            owner.update(cx, |owner, cx| {
                owner.settings_window_task.take();
                match result {
                    Ok(_) => {
                        if owner.settings_generation == preference_generation
                            && owner.last_error.as_ref().is_some_and(|error| error.code == "error.monitor.petApplyFailed") {
                            owner.last_error = None;
                        }
                    }
                    Err(error) => { owner.last_error = Some(error); owner.settings_window = None; }
                }
                cx.notify();
            });
            if failed { let _ = handle.update(cx, |_, window, _| window.remove_window()); }
        });
    });
    owner.update(cx, |owner, _| owner.settings_window_task = Some(task));
}

/// 原生创建或定位失败由原有设置错误面呈现，不保存或保留未确认的设置窗句柄。
fn failed(owner: &Entity<Controller>, error: anyhow::Error, cx: &mut App) {
    tracing::warn!(%error, "pet settings native placement failed");
    owner.update(cx, |owner, cx| {
        owner.last_error = Some(loki_metris_core::HookError::new("error.monitor.petApplyFailed"));
        cx.notify();
    });
}
