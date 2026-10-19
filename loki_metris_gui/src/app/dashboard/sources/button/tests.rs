//! 通过真实GPUI输入、绘制quad及原生Button语义检查动态反馈，不镜像CSS实现。

use super::*;
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, InputEvent, KeyDownEvent, KeyUpEvent,
    Pixels, Window, component::Disableable,
    Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Point, Render, Rgba, Role,
    TestAppContext, WindowBounds, WindowOptions, base::TestSupportExt as _, div, point, prelude::*,
    px, size, test::TestWindowExt as _,
};
use std::time::Duration;

/// 同一按钮可切换禁用、加载和主题；相邻按钮拥有独立动作计数。
struct Probe {
    /// 深色主题。
    dark: bool,
    /// 按钮禁用。
    disabled: bool,
    /// 按钮加载中。
    loading: bool,
    /// 两个按钮各自的点击次数。
    clicks: [usize; 2],
}

impl Render for Probe {
    /// 页面留白没有动作；回调由两个真实Button分别拥有。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("source-button-probe")
            .test_support()
            .tab_group()
            .size_full()
            .p(px(20.))
            .flex()
            .gap(px(20.))
            .children((0..2usize).map(|index| {
                SourceButton::new(
                    ("source-probe", index),
                    SourceButtonVariant::FilledBrand,
                    self.dark,
                )
                .w(px(136.))
                .h(px(36.))
                .px(px(18.))
                .rounded(px(12.))
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .disabled(index == 0 && self.disabled)
                .loading(index == 0 && self.loading)
                .label(format!("Action {index}"))
                .accessibility_label(format!("Source action {index}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicks[index] += 1;
                    cx.notify();
                }))
            }))
            .child(
                div()
                    .id("source-button-outside")
                    .test_support()
                    .absolute()
                    .right(px(10.))
                    .bottom(px(10.))
                    .size(px(20.)),
            )
    }
}

/// 隔离的测试窗口不连接真实目录、扫描器或宿主。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(420.), px(200.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| Probe {
                    dark: false,
                    disabled: false,
                    loading: false,
                    clicks: [0, 0],
                })
            },
        )
        .unwrap()
    })
}

/// 从实际Scene读取按钮背景矩形，布局测试记录有意保持未缩放占位。
fn visual_bounds(window: &Window, index: usize) -> Bounds<Pixels> {
    let layout = window.find(("source-probe", index)).bounds();
    let scale = window.scale_factor();
    let actual = window.painted_quads().into_iter().find(|quad| {
        quad.background.as_solid().is_some_and(|color| color.a > 0.)
            && quad.bounds.size.width.0 > 100. * scale
            && quad.bounds.size.height.0 > 20. * scale
            && (quad.bounds.center().x.0 / scale - f32::from(layout.center().x)).abs() < 2.
    }).expect("读取真实按钮背景图元");
    Bounds::new(actual.bounds.origin.map(|v| px(v.0 / scale)),
        actual.bounds.size.map(|v| px(v.0 / scale)))
}

/// 按压几何来自Scene而非控件布局，中心下移1px、宽高缩至98.5%。
fn assert_pressed(window: &Window, original: Bounds<Pixels>) {
    assert_eq!(window.find(("source-probe", 0usize)).bounds(), original,
        "按压变换不得改布局占位");
    let actual = visual_bounds(window, 0);
    for (actual, expected) in [
        (f32::from(actual.center().x), f32::from(original.center().x)),
        (f32::from(actual.center().y), f32::from(original.center().y) + 1.),
        (f32::from(actual.size.width), f32::from(original.size.width) * 0.985),
        (f32::from(actual.size.height), f32::from(original.size.height) * 0.985),
    ] {
        assert!((actual - expected).abs() < 0.0001, "实际图元{actual} != {expected}");
    }
}

/// 只读取按钮真实border-box上的填充，不能用文字或其它按钮的quad冒充反馈。
fn background(window: &Window, index: usize) -> Rgba {
    let bounds = visual_bounds(window, index).scale(window.scale_factor());
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| quad.bounds == bounds)
        .filter_map(|quad| quad.background.as_solid().map(Rgba::from)).rfind(|color| color.a > 0.)
        .expect("真实按钮必须绘制可见背景")
}

/// 颜色经过GPUI原生HSL往返允许浮点舍入，不能把可见色差当成容差。
fn assert_color(actual: Rgba, expected: Rgba, message: &str) {
    assert!(
        (actual.r - expected.r).abs() < 1e-6
            && (actual.g - expected.g).abs() < 1e-6
            && (actual.b - expected.b).abs() < 1e-6
            && (actual.a - expected.a).abs() < 1e-6,
        "{message}: {actual:?} != {expected:?}"
    );
}

/// 指针按下和松开分开传递，验证按下不会提前执行领域动作。
fn mouse(window: &mut Window, cx: &mut App, position: Point<Pixels>, down: bool) {
    if down {
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
    } else {
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    }
    window.render_frame(cx);
}

/// 键盘同样保留完整按下/松开时序，不以直接调用回调替代原生激活。
fn key(handle: AnyWindowHandle, key: &str, down: bool, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(key).unwrap();
    cx.update_window(handle, |_, window, cx| {
        if down {
            window.dispatch_event(
                KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
        } else {
            window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
        }
        window.render_frame(cx);
    })
    .unwrap();
}

/// 悬停首帧连续、200ms收敛；快速反向及亮暗重定向不能跳色或污染邻居。
#[gpui_kit::test]
fn source_button_hover_is_continuous_reversible_and_owned(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let initial = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find(("source-probe", 0usize)).role(),
                Some(Role::Button)
            );
            assert_eq!(
                window.find(("source-probe", 0usize)).label(),
                Some("Source action 0")
            );
            let initial = background(window, 0);
            window.hover(("source-probe", 0usize), cx);
            window.render_frame(cx);
            assert_eq!(
                background(window, 0),
                initial,
                "hover第一帧必须连续，不能瞬间换色"
            );
            initial
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(80));
    let middle = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let middle = background(window, 0);
            assert_ne!(middle, initial);
            assert_eq!(background(window, 1), initial);
            window.hover("source-button-outside", cx);
            window.render_frame(cx);
            assert_eq!(background(window, 0), middle, "反向第一帧不能跳变");
            middle
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(background(window, 0), initial);
        assert_ne!(initial, middle);
        window.hover(("source-probe", 0usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let settled = background(window, 0);
        let expected: Rgba = gpui_kit::rgb(0xa52f25);
        assert_color(settled, expected, "历史亮色filled hover取brand7");
        probe.update(cx, |this, cx| {
            this.dark = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(background(window, 0), settled, "切换主题首帧也应连续");
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_color(
            background(window, 0),
            gpui_kit::rgb(0xc43b2d),
            "历史暗色filled hover取brand6",
        );
    })
    .unwrap();
}

/// 鼠标按住整体缩放而不改布局，松开一次激活；禁用/加载与留白保持无动作。
#[gpui_kit::test]
fn source_button_pointer_press_disabled_and_loading_preserve_native_actions(
    cx: &mut TestAppContext,
) {
    let (handle, probe) = open(cx);
    let (bounds, position) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find(("source-probe", 0usize)).bounds();
            mouse(window, cx, bounds.center(), true);
            assert_eq!(probe.read(cx).clicks, [0, 0]);
            (bounds, bounds.center())
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_pressed(window, bounds);
        assert_eq!(
            window.find(("source-probe", 1usize)).bounds().origin.y,
            bounds.origin.y
        );
        mouse(window, cx, position, false);
        assert_eq!(probe.read(cx).clicks, [1, 0]);
        window.click_at("source-button-probe", point(px(3.), px(3.)), cx);
        assert_eq!(probe.read(cx).clicks, [1, 0]);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    for loading in [false, true] {
        probe.update(cx, |this, cx| {
            this.disabled = !loading;
            this.loading = loading;
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            mouse(window, cx, position, true);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("source-probe", 0usize)).bounds(), bounds);
            mouse(window, cx, position, false);
            assert_eq!(probe.read(cx).clicks, [1, usize::from(loading)]);
            window.click(("source-probe", 1usize), cx);
        })
        .unwrap();
    }
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [1, 2]);
}

/// 原生遍历建立初焦点后，Enter/Space按下不执行、松开一次执行且禁用后不再激活。
#[gpui_kit::test]
fn source_button_keyboard_keeps_focus_and_single_release_activation(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
    })
    .unwrap();
    let original = cx
        .update_window(handle, |_, window, _cx| {
            assert_eq!(window.find(("source-probe", 0usize)).focused(), Some(true));
            window.find(("source-probe", 0usize)).bounds()
        })
        .unwrap();
    for (name, count) in [("enter", 1), ("space", 2)] {
        key(handle, name, true, cx);
        assert_eq!(probe.read_with(cx, |this, _| this.clicks), [count - 1, 0]);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_pressed(window, original);
        })
        .unwrap();
        key(handle, name, false, cx);
        assert_eq!(probe.read_with(cx, |this, _| this.clicks), [count, 0]);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("source-probe", 0usize)).bounds(), original);
            assert_eq!(visual_bounds(window, 0), original);
        })
        .unwrap();
    }
    probe.update(cx, |this, cx| {
        this.loading = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    key(handle, "space", true, cx);
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("source-probe", 0usize)).bounds(), original);
        assert_eq!(visual_bounds(window, 0), original);
    })
    .unwrap();
    key(handle, "space", false, cx);
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 0]);
    probe.update(cx, |this, cx| {
        this.loading = false;
        this.disabled = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    key(handle, "enter", true, cx);
    key(handle, "enter", false, cx);
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 0]);
}

/// 按住后移出再松开不执行；中途禁用后恢复也不能继承旧按住状态。
#[gpui_kit::test]
fn source_button_cancelled_pointer_does_not_stick_or_activate(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let original = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find(("source-probe", 0usize)).bounds();
            mouse(window, cx, bounds.center(), true);
            bounds
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_pressed(window, original);
        window.hover("source-button-outside", cx);
        let outside = window.find("source-button-outside").bounds().center();
        mouse(window, cx, outside, false);
        assert_eq!(probe.read(cx).clicks, [0, 0]);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("source-probe", 0usize)).bounds(), original);
        assert_eq!(visual_bounds(window, 0), original);
        mouse(window, cx, original.center(), true);
        probe.update(cx, |this, cx| {
            this.disabled = true;
            cx.notify();
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("source-probe", 0usize)).bounds(), original);
        assert_eq!(visual_bounds(window, 0), original);
        mouse(window, cx, original.center(), false);
        assert_eq!(probe.read(cx).clicks, [0, 0]);
        probe.update(cx, |this, cx| {
            this.disabled = false;
            cx.notify();
        });
        window.render_frame(cx);
        window.click(("source-probe", 0usize), cx);
        assert_eq!(probe.read(cx).clicks, [1, 0]);
    })
    .unwrap();
}

/// 按住Space期间Tab改焦点会取消旧反馈与旧激活，随后新按钮仍可正常使用Enter。
#[gpui_kit::test]
fn source_button_keyboard_focus_change_cancels_old_press(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
    })
    .unwrap();
    let original = cx
        .update_window(handle, |_, window, _| {
            window.find(("source-probe", 0usize)).bounds()
        })
        .unwrap();
    key(handle, "space", true, cx);
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_pressed(window, original);
    })
    .unwrap();
    key(handle, "tab", true, cx);
    key(handle, "tab", false, cx);
    key(handle, "space", false, cx);
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("source-probe", 0usize)).bounds(), original);
        assert_eq!(visual_bounds(window, 0), original);
        assert_eq!(window.find(("source-probe", 1usize)).focused(), Some(true));
        assert_eq!(probe.read(cx).clicks, [0, 0]);
    })
    .unwrap();
    key(handle, "enter", true, cx);
    key(handle, "enter", false, cx);
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [0, 1]);
}
