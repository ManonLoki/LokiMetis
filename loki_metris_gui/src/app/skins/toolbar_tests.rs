//! 真实首次绘制、悬停与禁用输入回归，防止样式覆盖和重复hover注册复发。

use super::*;
use crate::app::surface::{HistoricalButtonSize, SurfaceButton};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Render, Role, TestAppContext,
    WindowBounds, WindowOptions,
    component::{Icon, Theme, ThemeMode},
    div, point, size,
    test::TestWindowExt,
};

/// 独立工具栏夹具，只记录真实按钮激活，不启动后台服务。
pub(super) struct Probe {
    pub(super) disabled: bool,
    pub(super) clicks: [usize; 2],
}
impl Render for Probe {
    /// 同时绘制宿主标签、刷新和导入，以实际元素验证状态和内容布局。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = window
            .use_keyed_state("host-focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        div()
            .id("toolbar-probe")
            .tab_group()
            .size_full()
            .flex()
            .items_start()
            .gap(px(12.))
            .p(px(20.))
            .child(host_tab(
                Button::new("host")
                    .role(Role::Tab)
                    .track_focus(&focus)
                    .child("WorkBuddy")
                    .on_click(cx.listener(|this, _, _, _| this.clicks[0] += 1)),
                "host-motion".into(),
                focus,
                false,
                self.disabled,
            ))
            .child(icon_action(
                default_icon_button("refresh", self.disabled, cx)
                    .size(px(36.))
                    .rounded(px(12.))
                    .accessibility_label("Refresh")
                    .child(Icon::default())
                    .on_click(cx.listener(|this, _, _, _| this.clicks[1] += 1)),
                "refresh-press",
                self.disabled,
            ))
            .child(
                SurfaceButton::new(
                    "import",
                    SurfaceButtonVariant::Default,
                    cx.theme().mode.is_dark(),
                )
                .historical_size(HistoricalButtonSize::Small)
                .left_icon(Icon::default())
                .left_icon_size(px(16.))
                .label("导入")
                .w(px(86.)),
            )
    }
}

/// 创建固定尺寸的内存测试窗口，不接触宿主偏好。
pub(super) fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(500.), px(160.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| Probe {
                    disabled: false,
                    clicks: [0, 0],
                })
            },
        )
        .unwrap()
    })
}

/// 从实际绘制图元读取控件底色，不能用builder或私有时间线代替绘制。
fn background(window: &Window, id: &str) -> Rgba {
    let bounds = window
        .find(id.to_owned())
        .bounds()
        .scale(window.scale_factor());
    window
        .painted_quads()
        .into_iter()
        .find(|quad| quad.bounds == bounds)
        .and_then(|quad| quad.background.as_solid())
        .expect("控件必须绘制底色")
        .into()
}
/// 容许RGBA与HSLA转换的有限误差，仍要求各通道到达历史色值。
fn near_color(actual: Rgba, expected: Rgba) {
    for (a, b) in [actual.r, actual.g, actual.b, actual.a]
        .into_iter()
        .zip([expected.r, expected.g, expected.b, expected.a])
    {
        assert!((a - b).abs() < 0.0001, "{actual:?} != {expected:?}");
    }
}

/// enabled首次绘制必须无重复hover断言；悬停仍保留历史默认按钮底色与边框。
#[gpui_kit::test]
fn native_toolbar_first_render_and_refresh_hover_keep_historical_colors(cx: &mut TestAppContext) {
    let (handle, _probe) = open(cx);
    for dark in [false, true] {
        cx.update(|cx| {
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                None,
                cx,
            )
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.hover("host", cx);
            window.render_frame(cx);
            near_color(
                background(window, "refresh"),
                colors(SurfaceButtonVariant::Default, dark, false, false).0,
            );
            window.hover("refresh", cx);
            window.render_frame(cx);
            near_color(
                background(window, "refresh"),
                colors(SurfaceButtonVariant::Default, dark, false, true).0,
            );
            let bounds = window.find("refresh").bounds().scale(window.scale_factor());
            let quad = window
                .painted_quads()
                .into_iter()
                .find(|quad| quad.bounds == bounds && quad.border_widths.top.0 > 0.)
                .unwrap();
            near_color(
                quad.border_color.into(),
                colors(SurfaceButtonVariant::Default, dark, false, true).1,
            );
            assert_eq!(window.find("refresh").bounds().size, size(px(36.), px(36.)));
            assert_eq!(window.find("host").role(), Some(Role::Tab));
        })
        .unwrap();
    }
}

/// 真实壳层的左右键循环并选择；Home/End仅移焦，原生Enter/Space才提交选择。
#[gpui_kit::test]
fn host_tabs_keyboard_preserves_scoped_focus_and_selection(cx: &mut TestAppContext) {
    use loki_metris_core::SkinHostKind::{Codex, WorkBuddy};
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1492., 900.);
    shell.update(cx, |shell, cx| {
        shell.page = crate::app::Page::Skins;
        shell.skins.host = Some(Codex);
        shell.skins.generation = 17;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("skin-host-Codex", cx);
        assert_eq!(
            shell.read(cx).skins.generation,
            17,
            "相同查询键不得失效在途状态"
        );
        window.press("tab", cx);
        assert_ne!(window.find("skin-host-WorkBuddy").focused(), Some(true));
        window.press("shift-tab", cx);
        assert_eq!(window.find("skin-host-Codex").focused(), Some(true));
        window.press("end", cx);
        assert_eq!(shell.read(cx).skins.host, Some(Codex));
        assert_eq!(window.find("skin-host-WorkBuddy").focused(), Some(true));
        window.press("enter", cx);
        assert_eq!(shell.read(cx).skins.host, Some(WorkBuddy));
        window.press("home", cx);
        assert_eq!(shell.read(cx).skins.host, Some(WorkBuddy));
        window.press("space", cx);
        assert_eq!(shell.read(cx).skins.host, Some(Codex));
        for (key, selected, focused) in [
            ("left", WorkBuddy, "skin-host-WorkBuddy"),
            ("right", Codex, "skin-host-Codex"),
            ("right", WorkBuddy, "skin-host-WorkBuddy"),
            ("left", Codex, "skin-host-Codex"),
        ] {
            window.press(key, cx);
            assert_eq!(shell.read(cx).skins.host, Some(selected));
            assert_eq!(window.find(focused).focused(), Some(true));
            assert_eq!(window.find(focused).selected(), Some(true));
        }
        shell.update(cx, |shell, cx| {
            shell.skins.busy = true;
            cx.notify();
        });
        window.render_frame(cx);
        window.press("right", cx);
        window.click("skin-host-WorkBuddy", cx);
        assert_eq!(shell.read(cx).skins.host, Some(Codex));
    })
    .unwrap();
}

/// 标签160ms内过渡并准确收敛；不靠无期限requestAnimationFrame保持静态画面。
#[gpui_kit::test]
fn host_hover_finishes_and_disabled_toolbar_blocks_native_activation(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update(|cx| Theme::change(ThemeMode::Light, None, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("host", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(80));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let partial = background(window, "host");
        assert!(partial.a > 0. && partial.a < 1.);
        // CSS:hover在鼠标静止时不因键盘输入消失。
        window.press("a", cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(80));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        near_color(background(window, "host"), rgb(0xf0e9de).into());
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(
            window.simulate_next_frame(cx),
            0,
            "静态端点不得继续预约下一帧"
        );
        probe.update(cx, |probe, cx| {
            probe.disabled = true;
            cx.notify();
        });
        window.render_frame(cx);
        near_color(background(window, "refresh"), rgb(0xe9ecef).into());
        window.click("host", cx);
        window.click("refresh", cx);
        window.press("tab", cx);
        window.press("enter", cx);
        window.press("space", cx);
    })
    .unwrap();
    assert_eq!(probe.read_with(cx, |probe, _| probe.clicks), [0, 0]);
}

/// 导入16px图标是局部合同；既有来源页默认17px由共享内容回归独立保护。
#[gpui_kit::test]
fn import_icon_keeps_sixteen_pixels_and_fits_fixed_button(cx: &mut TestAppContext) {
    let (handle, _probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find("import");
        let icon = gpui_kit::base::test_support::find(
            window,
            button.path(),
            &"historical-button-icon".into(),
        )
        .unwrap();
        let label = gpui_kit::base::test_support::find(
            window,
            button.path(),
            &"historical-button-label".into(),
        )
        .unwrap();
        assert_eq!(icon.bounds().size, size(px(16.), px(16.)));
        assert!(label.bounds().right() <= button.bounds().right() - px(18.));
        assert_eq!(button.bounds().size, size(px(86.), px(36.)));
    })
    .unwrap();
}
