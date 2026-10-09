//! Controller实际接受B后返回A并退出；不伪造PositionQueue回执。

use super::{tests::fixture, *};
use gpui_kit::TestAppContext;

#[gpui_kit::test]
fn accepted_b_then_return_a_shutdown_cancels_ui_receipt_but_preserves_final_a(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    let a = PetOverlayPosition { x: -10, y: 20 };
    let b = PetOverlayPosition { x: 30, y: 40 };
    let mut initial = crate::monitor::settings::MonitorSettings::default();
    initial.pet_overlay_position = Some(a);
    crate::monitor::settings::save_monitor_settings(&f.root.path().join("monitor"), &initial)
        .unwrap();
    f.owner.update(cx, |owner, _| owner.settings = initial);
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    assert_eq!(
        crate::usage_service::pet_background_task_count(&f.service),
        0
    );
    let held = crate::usage_service::block_pet_mutations(&f.service);
    let moved = |position: PetOverlayPosition| {
        Bounds::new(
            point(px(position.x as f32), px(position.y as f32)),
            size(px(128.), px(128.)),
        )
    };
    f.owner
        .update(cx, |owner, cx| owner.moved(moved(b), 1., cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(210));
    cx.run_until_parked();
    assert_eq!(
        crate::usage_service::pet_background_task_count(&f.service),
        1,
        "real B has been accepted and stays blocked at actual store lock"
    );
    f.owner
        .update(cx, |owner, cx| owner.moved(moved(a), 1., cx));
    let plan = cx.update(take_shutdown_plan);
    assert_eq!(
        plan.position,
        Some(a),
        "A must survive actual B acceptance and UI task cancellation"
    );
    held.release();
    f.service.finish_with_pet(plan);
    let saved =
        crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_overlay_position, Some(a));
    assert!(matches!(
        f.service.save_pet_position(b),
        Err("usage_service_closed")
    ));
}
