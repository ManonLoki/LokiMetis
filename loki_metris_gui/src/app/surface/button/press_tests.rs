//! 原生输入、真实Scene和辅助树共同验证按压仿射及取消，不用布局记录冒充绘制结果。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    MouseDownEvent, MouseUpEvent, Render, TestAppContext, WindowOptions, WindowVisibility,
    div, test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc};

/// 两个同级按钮共享真实动作模型，owner只用于检查隐藏后动画是否已经静止。
pub(super) struct Probe {
    /// 使用小尺寸。
    pub(super) small: bool,
    /// 两个按钮各自的点击次数。
    pub(super) clicks: [usize; 2],
    /// 回传的按钮动效 owner。
    pub(super) motion: Rc<RefCell<Option<gpui_kit::WeakEntity<ButtonMotion>>>>,
}
impl Render for Probe {
    /// 普通padding是祖先布局；兄弟按钮没有任何按压代理。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(20.)).flex().gap(px(20.)).tab_group()
            .children((0..2usize).map(|index| {
                let motion = self.motion.clone();
                let mut button = SurfaceButton::new(("press", index), SurfaceButtonVariant::FilledBrand, false)
                    .historical_size(if self.small { HistoricalButtonSize::Small } else { HistoricalButtonSize::XSmall })
                    .w(px(160.)).label("Scale")
                    .left_icon(Icon::default().data(&b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'><path fill='currentColor' d='M2 3h20v18H2z'/></svg>"[..]))
                    .on_click(cx.listener(move |this, _, _, _| this.clicks[index] += 1));
                if index == 0 {
                    button.motion_observer = Some(Box::new(move |owner| *motion.borrow_mut() = Some(owner.downgrade())));
                }
                button
            }))
    }
}

/// 每次事件都经过真实GPUI分发，释放不能直接调用Click回调。
pub(super) fn mouse(window: &mut Window, cx: &mut App, position: Point<Pixels>, down: bool) {
    if down {
        window.dispatch_event(MouseDownEvent {
            button: MouseButton::Left, position, modifiers: Modifiers::default(),
            click_count: 1, first_mouse: false,
        }.to_platform_input(), cx);
    } else {
        window.dispatch_event(MouseUpEvent {
            button: MouseButton::Left, position, modifiers: Modifiers::default(), click_count: 1,
        }.to_platform_input(), cx);
    }
    window.render_frame(cx);
}

/// 实际背景图元是可见border-box，返回值不来自Div布局测试记录。
pub(super) fn painted(window: &Window, index: usize) -> Bounds<Pixels> {
    let layout = window.find(("press", index)).bounds();
    let device = window.scale_factor();
    let quad = window.painted_quads().into_iter().find(|quad| {
        quad.background.as_solid().is_some_and(|value| value.a > 0.)
            && quad.bounds.size.width.0 > 100. * device
            && (quad.bounds.center().x.0 / device - f32::from(layout.center().x)).abs() < 2.
    }).expect("真实按钮背景");
    Bounds::new(quad.bounds.origin.map(|value| px(value.0 / device)),
        quad.bounds.size.map(|value| px(value.0 / device)))
}

/// 以真实稳定原生ID定位唯一辅助节点，包括其真实设备像素bounds。
fn a11y(window: &Window) -> serde_json::Value {
    let tree: serde_json::Value = serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
    let id = format!("{:?}", ElementId::from(("press", 0usize)));
    let nodes: Vec<_> = tree["nodes"].as_object().unwrap().values()
        .filter(|node| node["element_id"] == id)
        .collect();
    assert_eq!(nodes.len(), 1, "按钮必须继续使用唯一原生辅助节点: {tree}");
    nodes[0].clone()
}

/// 测试窗口不连接数据服务或用户配置，尺寸只决定可见控件范围。
fn open(cx: &mut TestAppContext, small: bool) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    let result = cx.update(|app| gpui_kit::open_window(WindowOptions::default(), app, |_, app| {
        app.new(|_| Probe { small, clicks: [0; 2], motion: Rc::default() })
    }).unwrap());
    assert!(cx.simulate_accessibility_activation(result.0).is_some());
    result
}

/// 两种历史高度均整体缩放，AX沿同一几何；原布局边缘内、新命中区外释放必须取消。
#[gpui_kit::test]
fn historical_press_scales_native_bounds_and_cancels_release_in_removed_edge(cx: &mut TestAppContext) {
    for small in [false, true] {
        let (handle, probe) = open(cx, small);
        let (original, sibling, original_ax) = cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            let original = painted(window, 0);
            let sibling = painted(window, 1);
            let original_ax = a11y(window);
            mouse(window, app, original.center(), true);
            (original, sibling, original_ax)
        }).unwrap();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            let pressed = painted(window, 0);
            assert_eq!(window.find(("press", 0usize)).bounds(), original, "原布局占位不变");
            assert_eq!(painted(window, 1), sibling, "兄弟元素不变");
            assert!((f32::from(pressed.size.width) - 160. * 0.985).abs() < 0.0001);
            assert!((f32::from(pressed.size.height) - f32::from(original.size.height) * 0.985).abs() < 0.0001);
            let node = a11y(window);
            assert_eq!(node["accesskit_id"], original_ax["accesskit_id"]);
            let device = f64::from(window.scale_factor());
            for (field, expected) in [("x0", pressed.left()), ("y0", pressed.top()), ("x1", pressed.right()), ("y1", pressed.bottom())] {
                assert!((node["bounds"][field].as_f64().unwrap() - f64::from(f32::from(expected)) * device).abs() < 0.0001);
            }
            mouse(window, app, point(original.left() + px(0.1), original.center().y), false);
            assert_eq!(probe.read(app).clicks, [0, 0], "缩掉的边缘不能沿旧矩形激活");
        }).unwrap();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            assert_eq!(painted(window, 0), original);
            mouse(window, app, original.center(), true);
            mouse(window, app, original.center(), false);
            assert_eq!(probe.read(app).clicks, [1, 0]);
        }).unwrap();
    }
}

/// 隐藏到显示之间不绘制；鼠标及Space的旧pending均不得复活，新的输入仍可激活。
#[gpui_kit::test]
fn historical_press_hidden_without_prepaint_cancels_native_pending_action(cx: &mut TestAppContext) {
    for keyboard in [false, true] {
        let (handle, probe) = open(cx, true);
        let original = cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            let bounds = painted(window, 0);
            if keyboard {
                window.focus_next(app);
                window.render_frame(app);
                window.dispatch_event(KeyDownEvent { keystroke: Keystroke::parse("space").unwrap(), is_held: false, prefer_character_input: false }.to_platform_input(), app);
                window.render_frame(app);
            } else { mouse(window, app, bounds.center(), true); }
            bounds
        }).unwrap();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
        cx.run_until_parked();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
        cx.run_until_parked();
        cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            assert_eq!(painted(window, 0), original);
            let owner = probe.read(app).motion.borrow().as_ref().unwrap().upgrade().unwrap();
            assert_eq!(owner.read(app).transform.sample(app.background_executor().now()), ([0., 1.], false));
            if keyboard {
                window.dispatch_event(KeyUpEvent { keystroke: Keystroke::parse("space").unwrap() }.to_platform_input(), app);
                window.render_frame(app);
                assert_eq!(probe.read(app).clicks, [0, 0]);
                window.press("space", app);
            } else {
                mouse(window, app, original.center(), false);
                assert_eq!(probe.read(app).clicks, [0, 0]);
                mouse(window, app, original.center(), true);
                mouse(window, app, original.center(), false);
            }
            assert_eq!(probe.read(app).clicks, [1, 0]);
        }).unwrap();
    }
}

/// 新的另一种输入不能解除旧模态取消；只有新序列自己的释放允许触发一次。
#[gpui_kit::test]
fn historical_press_cross_modality_does_not_revive_hidden_pending_action(cx: &mut TestAppContext) {
    for old_keyboard in [false, true] {
        let (handle, probe) = open(cx, true);
        let center = cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            window.focus_next(app);
            window.render_frame(app);
            let center = painted(window, 0).center();
            if old_keyboard { key(window, app, true); }
            else { mouse(window, app, center, true); }
            center
        }).unwrap();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
        cx.run_until_parked();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
        cx.run_until_parked();
        cx.update_window(handle, |_, window, app| {
            window.render_frame(app);
            if old_keyboard {
                mouse(window, app, center, true);
                key(window, app, false);
            } else {
                key(window, app, true);
                mouse(window, app, center, false);
            }
            assert_eq!(probe.read(app).clicks, [0, 0], "新模态不得重新允许旧release");
            if old_keyboard { mouse(window, app, center, false); }
            else { key(window, app, false); }
            assert_eq!(probe.read(app).clicks, [1, 0], "新序列仍只触发一次");
        }).unwrap();
    }
}

/// 真实Space事件辅助，复用同一原生分发及绘制流程。
fn key(window: &mut Window, app: &mut App, down: bool) {
    if down {
        window.dispatch_event(KeyDownEvent {
            keystroke: Keystroke::parse("space").unwrap(), is_held: false,
            prefer_character_input: false,
        }.to_platform_input(), app);
    } else {
        window.dispatch_event(KeyUpEvent { keystroke: Keystroke::parse("space").unwrap() }.to_platform_input(), app);
    }
    window.render_frame(app);
}

/// 隐藏前持住Space产生的OS重复键不是新输入，保持静止且释放不触发。
#[gpui_kit::test]
fn historical_press_held_repeat_after_hidden_remains_cancelled(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx, true);
    let original = cx.update_window(handle, |_, window, app| {
        window.render_frame(app);
        window.focus_next(app);
        window.render_frame(app);
        let original = painted(window, 0);
        key(window, app, true);
        original
    }).unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.run_until_parked();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| {
        window.render_frame(app);
        window.dispatch_event(KeyDownEvent {
            keystroke: Keystroke::parse("space").unwrap(), is_held: true,
            prefer_character_input: false,
        }.to_platform_input(), app);
        window.render_frame(app);
        assert_eq!(painted(window, 0), original);
        key(window, app, false);
        assert_eq!(probe.read(app).clicks, [0, 0]);
        window.press("space", app);
        assert_eq!(probe.read(app).clicks, [1, 0]);
    }).unwrap();
}
