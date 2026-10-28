//! 实际原生按钮内容、输入所有权与加载时钟回归，不用适配器私有状态替代绘制结果。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, InputEvent, KeyDownEvent, KeyUpEvent,
    Keystroke, Render, Role, TestAppContext, WindowBounds, WindowOptions, div, point, px, size,
    test::TestWindowExt,
};
use std::time::Duration;

/// 两种实际尺寸同时出现，相邻动作和反馈状态互不串扰。
struct Probe {
    /// 按钮加载中。
    loading: bool,
    /// 按钮禁用。
    disabled: bool,
    /// 两个按钮各自的点击次数。
    clicks: [usize; 2],
}
impl Render for Probe {
    /// 回调只属于真实 Button；父级只提供布局和像素基线。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(gpui_kit::rgb(0x000000))
            .p(px(20.))
            .flex()
            .items_start()
            .gap(px(20.))
            .tab_group()
            .children((0..2usize).map(|index| {
                let button = SurfaceButton::new(
                    ("historical-probe", index),
                    SurfaceButtonVariant::FilledBrand,
                    false,
                )
                .historical_size(if index == 0 {
                    HistoricalButtonSize::Small
                } else {
                    HistoricalButtonSize::XSmall
                })
                .loading(self.loading)
                .disabled(self.disabled)
                .label(format!("Action {index}"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.clicks[index] += 1;
                    cx.notify();
                }));
                if index == 0 {
                    button.left_icon(gpui_kit::component::Icon::default().data(include_bytes!(
                        "../../../../assets/icons/phosphor-check.svg"
                    )))
                } else {
                    button
                }
            }))
    }
}

/// 使用原生 Root 的焦点路径；不触碰真实偏好、服务或用户目录。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(480.), px(180.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| Probe {
                    loading: false,
                    disabled: false,
                    clicks: [0, 0],
                })
            },
        )
        .unwrap()
    })
}

/// 每次取真实子树；布局断言不读取控件builder或动画内部字段。
fn child(window: &mut Window, index: usize, id: &str) -> gpui_kit::test::ElementSnapshot {
    let button = window.find(("historical-probe", index));
    gpui_kit::base::test_support::find(window, button.path(), &ElementId::from(id.to_owned()))
        .unwrap()
}
/// 独立上下边吸附只允许小于一个设备像素的误差。
fn near(actual: Pixels, expected: Pixels) {
    assert!(
        (actual - expected).abs() <= px(0.51),
        "{actual:?} vs {expected:?}"
    );
}
/// 真实键盘事件经过当前原生焦点，不直接调用on_click或action回调。
fn key(handle: AnyWindowHandle, key: &'static str, down: bool, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, app| {
        let keystroke = Keystroke::parse(key).unwrap();
        let input = if down {
            KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input()
        } else {
            KeyUpEvent { keystroke }.to_platform_input()
        };
        window.dispatch_event(input, app);
        window.render_frame(app);
    })
    .unwrap();
}

/// 左图标、十像素间距和非对称内边距必须真正参与布局；两种文字各有自己的行盒。
#[gpui_kit::test]
fn historical_content_keeps_icon_gap_padding_and_line_boxes(cx: &mut TestAppContext) {
    let (handle, _probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find(("historical-probe", 0usize)).bounds();
        let icon = child(window, 0, "historical-button-icon").bounds();
        let label = child(window, 0, "historical-button-label").bounds();
        near(button.size.height, px(36.));
        near(icon.size.width, px(17.));
        near(icon.size.height, px(17.));
        near(icon.origin.x - button.origin.x, px(13.));
        near(label.origin.x - icon.right(), px(10.));
        near(button.right() - label.right(), px(19.));
        near(label.size.height, px(14.));
        let xs = window.find(("historical-probe", 1usize)).bounds();
        let xs_label = child(window, 1, "historical-button-label").bounds();
        near(xs.size.height, px(30.));
        near(xs_label.size.height, px(12.));
        near(xs_label.origin.x - xs.origin.x, px(15.));
        near(xs.right() - xs_label.right(), px(15.));
    })
    .unwrap();
}

/// 真实CSS时长到达后 inner完全移出clip，Loader位于整个按钮中心下一像素；宽度保持。
#[gpui_kit::test]
fn historical_loading_slides_hides_centers_and_reverses_continuously(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let original = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let original = window.find(("historical-probe", 0usize)).bounds();
            probe.update(cx, |this, cx| {
                this.loading = true;
                cx.notify();
            });
            window.render_frame(cx);
            original
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(50));
    let partial = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let inner = child(window, 0, "historical-button-inner").bounds();
            assert!(inner.origin.y > original.origin.y + px(1.));
            assert!(inner.origin.y < original.bottom());
            inner
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        probe.update(cx, |this, cx| {
            this.loading = false;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(
            child(window, 0, "historical-button-inner").bounds(),
            partial
        );
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(("historical-probe", 0usize)).bounds(), original);
        assert!(child(window, 0, "historical-button-label").visible());
        probe.update(cx, |this, cx| {
            this.loading = true;
            cx.notify();
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (index, diameter) in [(0usize, 20.), (1, 30. / 1.8)] {
            let button = window.find(("historical-probe", index));
            assert_eq!(button.role(), Some(Role::Button));
            let loader = child(window, index, "historical-button-loader").bounds();
            near(loader.size.width, px(diameter));
            near(loader.size.height, px(diameter));
            near(loader.center().x, button.bounds().center().x);
            near(loader.center().y, button.bounds().center().y + px(1.));
            assert!(!child(window, index, "historical-button-label").visible());
            window.click(("historical-probe", index), cx);
        }
        assert_eq!(probe.read(cx).clicks, [0, 0]);
        assert_eq!(window.find(("historical-probe", 0usize)).bounds(), original);
    })
    .unwrap();
}

/// 原生焦点和Enter/Space各激活一次；loading与disabled均阻止重复且不代理给父级。
#[gpui_kit::test]
fn historical_loading_preserves_native_input_ownership(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
    })
    .unwrap();
    key(handle, "enter", true, cx);
    key(handle, "enter", false, cx);
    key(handle, "space", true, cx);
    key(handle, "space", false, cx);
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 0]);
    probe.update(cx, |this, cx| {
        this.loading = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("historical-probe", 0usize), cx);
    })
    .unwrap();
    key(handle, "enter", true, cx);
    key(handle, "enter", false, cx);
    key(handle, "space", true, cx);
    key(handle, "space", false, cx);
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 0]);
    probe.update(cx, |this, cx| {
        this.loading = false;
        this.disabled = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("historical-probe", 0usize), cx);
    })
    .unwrap();
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 0]);
    probe.update(cx, |this, cx| {
        this.disabled = false;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        window.click(("historical-probe", 1usize), cx);
    })
    .unwrap();
    assert_eq!(probe.read_with(cx, |this, _| this.clicks), [2, 1]);
}
