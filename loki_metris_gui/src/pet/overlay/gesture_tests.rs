//! 使用实际Overlay与Controller观察主窗激活，不以生产计数旗标模拟ShowMain。

use crate::pet::tests::{Fixture, fixture, settle};
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, InputEvent, MouseButton, MouseDownEvent, MouseExitEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Subscription, TestAppContext, VisualTestContext,
    WindowVisibility, point, px, test::TestWindowExt,
};
use loki_metris_core::PetPreferenceChange;

/// 真实主窗的激活通知是ShowMain结果；测试资源与内存实体均由当前fixture拥有。
fn observed_fixture(cx: &mut TestAppContext) -> (Fixture, Entity<usize>, Subscription) {
    let f = fixture(cx);
    let main = f.owner.read_with(cx, |owner, _| owner.main);
    let count = cx.new(|_| 0_usize);
    let observe = cx.update_window(main, |_, window, cx| {
        count.update(cx, |_, cx| cx.observe_window_activation(window, |count, window, _| {
            if window.is_window_active() { *count += 1; }
        }))
    }).unwrap();
    cx.update_window(f.overlay, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
    }).unwrap();
    cx.run_until_parked();
    count.update(cx, |count, _| *count = 0);
    (f, count, observe)
}

/// 逐个完整事件后推进原生defer队列，才能检出按下过早显示主窗的真实结果。
fn dispatch(handle: AnyWindowHandle, event: impl InputEvent, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(event.to_platform_input(), cx);
        window.render_frame(cx);
    }).unwrap();
    cx.run_until_parked();
}

/// 位置来自生产slot的实际布局，避免将分页栏误当桌宠空白。
fn body_position(f: &Fixture, cx: &mut TestAppContext) -> Point<Pixels> {
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        window.find(("pet-position", 0_usize)).bounds().center()
    }).unwrap()
}

/// 平台clickCount原样传入，不在测试里重新累计双击时间窗口。
fn down(f: &Fixture, position: Point<Pixels>, count: usize, cx: &mut TestAppContext) {
    dispatch(f.overlay, MouseDownEvent {
        button: MouseButton::Left, position, click_count: count, ..Default::default()
    }, cx);
}

/// 真实释放事件决定一次鼠标点击是否完成。
fn up(f: &Fixture, position: Point<Pixels>, count: usize, cx: &mut TestAppContext) {
    dispatch(f.overlay, MouseUpEvent {
        button: MouseButton::Left, position, click_count: count, ..Default::default()
    }, cx);
}

/// 按住时位移仍使用逻辑坐标，中途离开后返回也不能恢复已取消的候选。
fn motion(f: &Fixture, position: Point<Pixels>, cx: &mut TestAppContext) {
    dispatch(f.overlay, MouseMoveEvent {
        position, pressed_button: Some(MouseButton::Left), ..Default::default()
    }, cx);
}

/// 第二次按下不激活主窗，完成释放才激活一次；重复释放与第三击不重放。
#[gpui_kit::test]
fn completed_double_click_activates_only_after_second_up_once(cx: &mut TestAppContext) {
    let (f, count, _observe) = observed_fixture(cx);
    let position = body_position(&f, cx);
    down(&f, position, 1, cx);
    up(&f, position, 1, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 0);
    down(&f, position, 2, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 0, "二击Down不能ShowMain");
    up(&f, position, 2, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 1);
    cx.update_window(f.overlay, |_, window, _| window.activate_window()).unwrap();
    cx.run_until_parked();
    up(&f, position, 2, cx);
    down(&f, position, 3, cx);
    up(&f, position, 3, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 1, "候选必须只消费一次");
}

/// 实际移动、离开、失焦、隐藏、替换按下和不匹配释放均取消当前二击。
#[gpui_kit::test]
fn moved_or_cancelled_second_press_does_not_activate_main(cx: &mut TestAppContext) {
    for cancellation in ["move", "out-and-back", "exit", "blur", "deactivate", "hidden", "passthrough", "replace", "count", "button"] {
        let (f, count, _observe) = observed_fixture(cx);
        let position = body_position(&f, cx);
        down(&f, position, 2, cx);
        let mut release = position;
        match cancellation {
            "move" | "out-and-back" => {
                release.x += px(40.);
                motion(&f, release, cx);
                if cancellation == "out-and-back" { release = position; motion(&f, release, cx); }
            }
            "exit" => {
                dispatch(f.overlay, MouseExitEvent { position: point(px(-1.), px(-1.)), ..Default::default() }, cx);
                motion(&f, position, cx);
            }
            "blur" => { cx.update_window(f.overlay, |_, window, cx| window.blur(cx)).unwrap(); cx.run_until_parked(); }
            "deactivate" => { VisualTestContext::from_window(f.overlay, cx).deactivate_window(); cx.run_until_parked(); }
            "hidden" => {
                cx.simulate_window_visibility_change(f.overlay, WindowVisibility::Hidden);
                cx.run_until_parked();
                cx.simulate_window_visibility_change(f.overlay, WindowVisibility::Visible);
                cx.run_until_parked();
            }
            "replace" => down(&f, position, 1, cx),
            "passthrough" => {
                f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(true), cx));
                settle(cx, &f.owner);
                f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(false), cx));
                settle(cx, &f.owner);
            }
            _ => {}
        }
        if cancellation == "button" {
            dispatch(f.overlay, MouseUpEvent { button: MouseButton::Right, position, click_count: 2, ..Default::default() }, cx);
        } else { up(&f, release, if cancellation == "count" { 1 } else { 2 }, cx); }
        assert_eq!(count.read_with(cx, |count, _| *count), 0, "{cancellation}不能ShowMain");
    }
}

/// OS不保证送出MouseExit；即使测试注入迟到hover/双击，穿透仍清除分页和手势。
#[gpui_kit::test]
fn mouse_passthrough_ignores_stale_mouse_gestures_without_resetting_page(cx: &mut TestAppContext) {
    let (f, count, _observe) = observed_fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| { window.hover("pet-overlay", cx); window.render_frame(cx); assert!(window.try_find("pet-pager").is_some()); }).unwrap();
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(true), cx));
    settle(cx, &f.owner);
    let position = body_position(&f, cx);
    motion(&f, position, cx);
    down(&f, position, 2, cx);
    up(&f, position, 2, cx);
    cx.update_window(f.overlay, |_, window, cx| { window.render_frame(cx); assert!(window.try_find("pet-pager").is_none()); }).unwrap();
    assert_eq!(count.read_with(cx, |count, _| *count), 0);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 0);
}

/// 即使位移仅1px，跨实际slot或分页栏边界也不能完成原目标的双击。
#[gpui_kit::test]
fn double_click_requires_same_slot_and_control_target(cx: &mut TestAppContext) {
    for crossing in ["slot", "body-to-pager", "pager-to-body"] {
        let (f, count, _observe) = observed_fixture(cx);
        let (before, after) = cx.update_window(f.overlay, |_, window, cx| {
            window.hover("pet-overlay", cx);
            window.render_frame(cx);
            if crossing == "slot" {
                let tile = window.find(("pet-position", 0_usize)).bounds();
                (point(tile.right() - px(0.5), tile.center().y), point(tile.right() + px(0.5), tile.center().y))
            } else {
                let pager = window.find("pet-pager").bounds();
                (point(px(10.), pager.origin.y - px(0.5)), point(px(10.), pager.origin.y + px(0.5)))
            }
        }).unwrap();
        let (press, release) = if crossing == "pager-to-body" { (after, before) } else { (before, after) };
        down(&f, press, 2, cx);
        up(&f, release, 2, cx);
        assert_eq!(count.read_with(cx, |count, _| *count), 0, "{crossing}跨目标不能ShowMain");
    }
}

/// 锁定仅阻止原生单击拖动，单击不ShowMain而完整鼠标双击仍激活主窗。
#[gpui_kit::test]
fn locked_overlay_still_requires_completed_double_click(cx: &mut TestAppContext) {
    let (f, count, _observe) = observed_fixture(cx);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Locked(true), cx));
    settle(cx, &f.owner);
    let position = body_position(&f, cx);
    down(&f, position, 1, cx);
    up(&f, position, 1, cx);
    down(&f, position, 2, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 0);
    up(&f, position, 2, cx);
    assert_eq!(count.read_with(cx, |count, _| *count), 1);
    assert!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.locked));
}

/// 分页栏双击不显示主窗；按钮只翻一页，右键仍打开实际独立设置窗。
#[gpui_kit::test]
fn pager_clicks_keep_their_own_actions(cx: &mut TestAppContext) {
    let (f, count, _observe) = observed_fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| {
        window.hover("pet-overlay", cx);
        window.double_click("pet-pager", cx);
        window.click("pet-next", cx);
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 1);
    assert_eq!(count.read_with(cx, |count, _| *count), 0);
    cx.update_window(f.overlay, |_, window, cx| {
        window.right_click("pet-pager", cx);
    }).unwrap();
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_some()));
    assert_eq!(count.read_with(cx, |count, _| *count), 0);
}

/// 位移阈值在1x和2x均为2逻辑像素；略超过后返回也不能恢复完成动作。
#[gpui_kit::test]
fn double_click_motion_threshold_is_logical_and_cancellation_is_sticky(cx: &mut TestAppContext) {
    for scale in [1., 2.] {
        for (distance, expected) in [(0., 1), (2., 1), (2.01, 0)] {
            let (f, count, _observe) = observed_fixture(cx);
            cx.simulate_window_scale_factor_change(f.overlay, scale);
            let position = body_position(&f, cx);
            down(&f, position, 2, cx);
            motion(&f, point(position.x + px(distance), position.y), cx);
            motion(&f, position, cx);
            up(&f, position, 2, cx);
            assert_eq!(count.read_with(cx, |count, _| *count), expected, "scale={scale} distance={distance}");
        }
    }
}
