#![cfg(target_os = "macos")]
//! 真实原生Checkbox验证父圆角绘制范围对应的hover/cursor/down-up及键盘语义。
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, FocusHandle, InputEvent, IntoElement,
    KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, Role, TestAppContext, Window, WindowBounds, WindowOptions,
    base::{Checkbox, CheckboxState, TestSupportExt as _},
    div, point,
    prelude::*,
    px, size,
    test::TestWindowExt,
};

/// 父裁切复选框回归的测试根视图：记录勾选、禁用、变更次数、悬停与圆角等可观察状态。
struct Probe {
    /// 复选框勾选状态。
    checked: bool,
    /// 复选框禁用。
    disabled: bool,
    /// 勾选变化次数。
    changes: usize,
    /// 指针悬停。
    hovered: bool,
    /// 父容器圆角半径。
    radius: f32,
    /// 焦点锚点。
    anchor: FocusHandle,
}
impl Render for Probe {
    /// 在圆角父裁切容器内渲染受测复选框，并把状态变化写回探针。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let changed = cx.listener(|this, state: &CheckboxState, _, cx| {
            this.checked = *state == CheckboxState::Checked;
            this.changes += 1;
            cx.notify();
        });
        div()
            .size_full()
            .tab_group()
            .child(
                div()
                    .absolute()
                    .size_0()
                    .track_focus(&self.anchor)
                    .tab_stop(false),
            )
            .child(
                div()
                    .id("outer")
                    .test_support()
                    .relative()
                    .left(px(24.))
                    .top(px(37.))
                    .w(px(322.))
                    .h(px(160.))
                    .overflow_hidden()
                    .rounded_tl(px(self.radius))
                    .rounded_tr(px(self.radius))
                    .clip_rounded_children()
                    .child(
                        Checkbox::new("selection")
                            .checked(self.checked)
                            .disabled(self.disabled)
                            .accessibility_label("Select Pikachu")
                            .absolute()
                            .right(px(-4.))
                            .top(px(-4.))
                            .size(px(20.))
                            .rounded(px(5.))
                            .border_1()
                            .cursor_pointer()
                            .child(if self.checked { "✓" } else { "" })
                            .on_hover(cx.listener(|this, hovered, _, _| this.hovered = *hovered))
                            .on_change(move |state, _, window, cx| changed(&state, window, cx)),
                    ),
            )
    }
}

/// 初始化组件库并打开承载探针的测试窗口，返回窗口句柄与探针实体。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    let (handle, entity) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(600.), px(300.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let anchor = cx.focus_handle();
                    window.focus(&anchor, cx);
                    cx.new(|_| Probe {
                        checked: false,
                        disabled: false,
                        changes: 0,
                        hovered: false,
                        radius: 20.,
                        anchor,
                    })
                },
            )
        })
        .unwrap();
    (handle, entity)
}

/// 向测试窗口派发一次输入事件，并等待前台任务全部完成。
fn event(handle: AnyWindowHandle, event: impl InputEvent, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(event.to_platform_input(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}
/// 模拟指针移动到指定位置（无按键按下）。
fn pointer(handle: AnyWindowHandle, position: Point<Pixels>, cx: &mut TestAppContext) {
    event(
        handle,
        MouseMoveEvent {
            position,
            ..Default::default()
        },
        cx,
    );
}
/// 模拟按住左键时把指针拖到指定位置。
fn move_pressed(handle: AnyWindowHandle, position: Point<Pixels>, cx: &mut TestAppContext) {
    event(
        handle,
        MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        },
        cx,
    );
}
/// 模拟在指定位置按下左键。
fn down(handle: AnyWindowHandle, position: Point<Pixels>, cx: &mut TestAppContext) {
    event(
        handle,
        MouseDownEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        },
        cx,
    );
}
/// 模拟在指定位置松开左键。
fn up(handle: AnyWindowHandle, position: Point<Pixels>, cx: &mut TestAppContext) {
    event(
        handle,
        MouseUpEvent {
            button: MouseButton::Left,
            position,
            click_count: 1,
            ..Default::default()
        },
        cx,
    );
}
/// 模拟一次完整的按键按下与松开。
fn key(handle: AnyWindowHandle, key: &'static str, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(key).unwrap();
    event(
        handle,
        KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        },
        cx,
    );
    event(handle, KeyUpEvent { keystroke }, cx);
}
/// 渲染一帧后返回勾选框在父裁切外与裁切内的两个命中采样点，并断言其几何前提。
fn positions(handle: AnyWindowHandle, cx: &mut TestAppContext) -> (Point<Pixels>, Point<Pixels>) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let outer = window.find("outer").bounds();
        let check = window.find("selection").bounds();
        assert_eq!(check.origin.y, outer.origin.y - px(4.));
        assert_eq!(check.right(), outer.right() + px(4.));
        let outside = check.origin + point(px(15.), px(5.));
        let inside = check.origin + point(px(6.), px(12.));
        assert!(outer.contains(&outside));
        (outside, inside)
    })
    .unwrap()
}

/// 真hover、完整down/up和后续点击同时验证，不在on_change里过滤。
#[gpui_kit::test]
fn native_checkbox_rejects_parent_round_corner_and_clears_pending(cx: &mut TestAppContext) {
    for scale in [1., 2.] {
        let (window, probe) = open(cx);
        cx.simulate_window_scale_factor_change(window, scale);
        let (outside, inside) = positions(window, cx);
        pointer(window, outside, cx);
        assert!(!probe.read_with(cx, |p, _| p.hovered));
        down(window, outside, cx);
        up(window, outside, cx);
        assert_eq!(probe.read_with(cx, |p, _| p.changes), 0);
        cx.update_window(window, |_, window, _| {
            assert_ne!(window.find("selection").focused(), Some(true))
        })
        .unwrap();
        pointer(window, inside, cx);
        assert!(probe.read_with(cx, |p, _| p.hovered));
        down(window, inside, cx);
        move_pressed(window, outside, cx);
        up(window, outside, cx);
        assert_eq!(probe.read_with(cx, |p, _| p.changes), 0);
        down(window, outside, cx);
        move_pressed(window, inside, cx);
        up(window, inside, cx);
        assert_eq!(probe.read_with(cx, |p, _| p.changes), 0);
        down(window, inside, cx);
        up(window, inside, cx);
        assert_eq!(probe.read_with(cx, |p, _| p.changes), 1);
    }
}

/// 圆角只影响pointer可见区，不能取消原生Tab、AX或完整按键激活。
#[gpui_kit::test]
fn native_checkbox_keeps_keyboard_and_a11y_and_disabled_contract(cx: &mut TestAppContext) {
    let (window, probe) = open(cx);
    positions(window, cx);
    key(window, "tab", cx);
    cx.update_window(window, |_, window, _| {
        let checkbox = window.find("selection");
        assert_eq!(checkbox.role(), Some(Role::CheckBox));
        assert_eq!(checkbox.label(), Some("Select Pikachu"));
        assert_eq!(checkbox.focused(), Some(true));
    })
    .unwrap();
    key(window, "space", cx);
    key(window, "enter", cx);
    assert_eq!(probe.read_with(cx, |p, _| p.changes), 2);
    probe.update(cx, |p, cx| {
        p.disabled = true;
        cx.notify();
    });
    cx.run_until_parked();
    key(window, "space", cx);
    key(window, "enter", cx);
    let (_, inside) = positions(window, cx);
    down(window, inside, cx);
    up(window, inside, cx);
    assert_eq!(probe.read_with(cx, |p, _| p.changes), 2);
}

// GPUI内部测试还必须直接检查Frame::cursor_style及rendered_frame.scene中的
// Quad/Monochrome/Subpixel父head；现有公开TestWindow没有cursor getter，不能捏造。
// 真实CPU cache/nested/deferred及Metal像素用例按IMPLEMENTATION.md逐项并入对应crate。
