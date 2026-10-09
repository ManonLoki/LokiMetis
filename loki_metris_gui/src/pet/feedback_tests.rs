//! 真实窗口bounds观察→位置写入→版本refresh链的隔离回归；不触碰用户配置。

use super::{tests::fixture, *};
use gpui_kit::TestAppContext;

/// 真实后台回执需要假时钟与有界wall让步；不以task字段形状作为行为断言。
fn pump(cx: &mut TestAppContext) {
    for _ in 0..30 {
        cx.executor().advance_clock(Duration::from_millis(30));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 已保存的同一物理位置反复收到内部bounds通知和版本轮询，不能产生新写入或版本。
#[gpui_kit::test]
fn repeated_internal_bounds_and_refresh_do_not_restart_pet_save_cycle(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let (bounds, scale) = cx
        .update_window(f.overlay, |_, window, _| {
            (host::bounds(window), window.scale_factor())
        })
        .unwrap();
    let position = PetOverlayPosition {
        x: (f32::from(bounds.origin.x) * scale).round() as i32,
        y: (f32::from(bounds.origin.y) * scale).round() as i32,
    };
    let settings = f
        .service
        .save_pet_position(position)
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    f.owner.update(cx, |owner, _| {
        owner.settings = settings;
        owner.observed = (
            f.service.agent_settings_revision(),
            f.service.pet_settings_revision(),
            f.service.monitor_catalog_revision(),
            u64::MAX,
        );
    });
    for _ in 0..4 {
        f.owner.update(cx, |owner, cx| {
            owner.apply_geometry(false, cx).unwrap();
        });
        pump(cx);
        f.owner.update(cx, |owner, cx| owner.refresh(cx));
        pump(cx);
    }
    // 排空已接受的真实后台任务，不能靠磁盘尚未调度完成制造假通过。
    f.service.finish();
    assert_eq!(
        f.service.pet_settings_revision(),
        1,
        "stable bounds must converge without saving"
    );
    assert_eq!(
        crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor"))
            .unwrap()
            .pet_overlay_position,
        Some(position)
    );
}

/// 同一DPI量化像素的抖动不会延期；一次真实负坐标移动仍以原round规则持久化。
#[gpui_kit::test]
fn same_physical_pixel_jitter_keeps_deadline_and_real_motion_still_saves(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.moved(
            Bounds::new(point(px(-40.3), px(60.)), size(px(128.), px(128.))),
            2.,
            cx,
        )
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(150));
    f.owner.update(cx, |owner, cx| {
        owner.moved(
            Bounds::new(point(px(-40.4), px(60.)), size(px(128.), px(128.))),
            2.,
            cx,
        )
    });
    cx.executor().advance_clock(Duration::from_millis(60));
    cx.run_until_parked();
    // 只排空已接受写入，不推进假防抖时钟；旧实现此时尚未接受延期到350ms的写入。
    f.service.finish();
    assert_eq!(
        f.service.pet_settings_revision(),
        1,
        "same pixel notification must not restart 200ms debounce"
    );
    let saved =
        crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(
        saved.pet_overlay_position,
        Some(PetOverlayPosition { x: -81, y: 120 })
    );
}
