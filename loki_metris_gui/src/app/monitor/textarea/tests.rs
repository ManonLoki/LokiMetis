//! 真实 Shell 与原生 Textarea 的几何、材质和键盘路径，不读取适配器内部状态。

use crate::app::{Page, Shell, monitor::state::Route};
use crate::preferences::ThemeChoice;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, Hsla, Pixels, Role, TestAppContext,
    Window,
    component::{ActiveTheme, WindowExt, input::TextareaState},
    px, rgb,
    test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, public_monitor_ai_tools};
use std::time::Duration;

/// 用真实档案与页面建立四个原生状态，持久化仅进入测试内存。
fn setup(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 1400.);
    shell.update(cx, |shell, cx| {
        shell.page = Page::Monitor;
        shell.monitor.route = Route::Management;
        shell.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        shell.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    (handle, shell, writer)
}

/// 原生输入实体来自生产 catalog，而非为样式测试另建控件。
fn inputs(shell: &Entity<Shell>, cx: &gpui_kit::App) -> [Entity<TextareaState>; 4] {
    shell.read(cx).monitor.catalog.editors[0]
        .inputs
        .clone()
        .unwrap()
}

/// 允许布局的独立设备像素吸附，不能容许旧组件的八像素差距。
fn near(window: &Window, actual: Pixels, expected: f32) {
    assert!(
        (actual - px(expected)).abs() < px(1. / window.scale_factor()),
        "actual {actual:?}, expected {expected}"
    );
}

/// 填充和边框可由 Kit 分成两个图元；都必须来自同一个真实输入边界。
fn material(window: &Window, bounds: Bounds<Pixels>) -> (gpui_kit::Quad, gpui_kit::Quad) {
    let quads = window.painted_quads();
    let background = quads
        .iter()
        .find(|quad| {
            quad.bounds == bounds.scale(window.scale_factor())
                && quad.background.as_solid().is_some_and(|color| color.a > 0.)
        })
        .expect("真实输入必须绘制背景")
        .clone();
    let border = quads
        .into_iter()
        .find(|quad| {
            quad.bounds == bounds.scale(window.scale_factor())
                && quad.border_color.a > 0.
                && quad.border_widths.top == px(1.).scale(window.scale_factor())
        })
        .expect("真实输入必须绘制一像素边框");
    (background, border)
}

/// 两主题必须使用历史实底、描边和十二像素圆角；不污染全局输入令牌。
#[gpui_kit::test]
fn shell_textarea_has_historical_palette_radius_and_height(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    for (theme, background, border) in [
        (ThemeChoice::Light, 0xffffff, 0xced4da),
        (ThemeChoice::Dark, 0x2e2e2e, 0x424242),
    ] {
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.preferences.theme = theme;
                shell.apply_theme(window, cx);
                cx.notify();
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let old_input_token = cx.theme().input;
            for key in ["idle", "running", "asking", "error"] {
                let bounds = window.find(format!("monitor-content-input-{key}")).bounds();
                let (background_quad, painted) = material(window, bounds);
                assert_eq!(
                    background_quad.background.as_solid(),
                    Some(Hsla::from(rgb(background)))
                );
                assert_eq!(painted.border_color, Hsla::from(rgb(border)));
                assert_eq!(
                    painted.corner_radii.top_left,
                    px(12.).scale(window.scale_factor())
                );
                assert_eq!(
                    painted.corner_radii.bottom_right,
                    px(12.).scale(window.scale_factor())
                );
                near(window, bounds.size.height, 56.4);
            }
            assert_eq!(
                cx.theme().input,
                old_input_token,
                "局部材质不能覆写 Theme 输入色"
            );
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(100));
    }
}

/// 原生文本范围真实包含十二像素横向内距、五点五纵距和二十一点七行高。
#[gpui_kit::test]
fn shell_textarea_content_padding_and_line_height_are_native(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let input = inputs(&shell, cx)[0].clone();
        input.update(cx, |input, cx| input.set_value("Alpha\nBeta", window, cx));
        window.render_frame(cx);
        let bounds = window.find("monitor-content-input-idle").bounds();
        let state = input.read(cx);
        let text = state.text_bounds().expect("原生文本布局已完成");
        near(window, text.origin.x - bounds.origin.x, 13.);
        near(window, text.origin.y - bounds.origin.y, 6.5);
        near(window, bounds.right() - text.right(), 13.);
        near(window, state.line_height().unwrap(), 21.7);
        near(window, bounds.size.height, 56.4);
    })
    .unwrap();
}

/// 焦点只过渡本输入的描边，半程为标准 CSS ease，反向必须从真实当前位置继续。
#[gpui_kit::test]
fn shell_textarea_focus_eases_without_ring_or_sibling_state(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx);
    let sample = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| {
            let bounds = window.find("monitor-content-input-idle").bounds();
            material(window, bounds).1.border_color
        })
        .unwrap()
    };
    let resting = Hsla::from(rgb(0xced4da));
    assert_eq!(sample(cx), resting);
    cx.update_window(handle, |_, window, cx| {
        window.click_at(
            "monitor-content-input-idle",
            gpui_kit::point(px(20.), px(12.)),
            cx,
        )
    })
    .unwrap();
    assert_eq!(sample(cx), resting, "焦点首帧不能瞬跳或出现原生 ring");
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let middle = sample(cx);
    let easing = gpui_kit::base::Easing::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.,
    }
    .sample(0.5);
    let from = gpui_kit::Rgba::from(rgb(0xced4da));
    let to = gpui_kit::Rgba::from(rgb(0xc43b2d));
    let expected = Hsla::from(gpui_kit::Rgba {
        r: from.r + (to.r - from.r) * easing,
        g: from.g + (to.g - from.g) * easing,
        b: from.b + (to.b - from.b) * easing,
        a: 1.,
    });
    assert!(
        (middle.h - expected.h).abs() < 0.0001
            && (middle.s - expected.s).abs() < 0.0001
            && (middle.l - expected.l).abs() < 0.0001,
        "{middle:?} vs {expected:?}"
    );
    cx.update_window(handle, |_, window, cx| {
        let sibling = window.find("monitor-content-input-running").bounds();
        assert_eq!(material(window, sibling).1.border_color, resting);
        window.click_at(
            "monitor-content-input-running",
            gpui_kit::point(px(20.), px(12.)),
            cx,
        );
    })
    .unwrap();
    assert_eq!(sample(cx), middle, "反向首帧不得跳色");
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("monitor-content-input-idle").bounds();
        assert_eq!(material(window, bounds).1.border_color, resting);
        let sibling = window.find("monitor-content-input-running").bounds();
        assert_eq!(
            material(window, sibling).1.border_color,
            Hsla::from(rgb(0xc43b2d))
        );
        let ring = cx.theme().ring.opacity(0.5);
        assert!(
            !window
                .painted_quads()
                .iter()
                .any(|quad| quad.border_color == ring
                    && quad
                        .bounds
                        .intersects(&sibling.scale(window.scale_factor()))),
            "不能留下 Kit 外环"
        );
    })
    .unwrap();
}

/// 原生点击、IME 文本、换行、选择、复制粘贴与撤销仍更新同一真实草稿及辅助值。
#[gpui_kit::test]
fn shell_textarea_keeps_native_editing_selection_caret_and_registry(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let input = inputs(&shell, cx)[0].clone();
        let native_id = ("input", input.entity_id());
        assert_eq!(
            window.find(native_id).role(),
            Some(Role::MultilineTextInput)
        );
        window.click_at(
            "monitor-content-input-idle",
            gpui_kit::point(px(20.), px(12.)),
            cx,
        );
        assert_eq!(window.focused(cx), Some(input.focus_handle(cx)));
        assert!(matches!(
            window.focused_input(cx),
            Some(gpui_kit::component::input::AnyInputState::Textarea(_))
        ));
        window.input("Hello 中文", cx);
        window.press("enter", cx);
        window.input("Tail", cx);
        assert_eq!(input.read(cx).value().as_ref(), "Hello 中文\nTail");
        assert!(
            input.read(cx).cursor_layout().is_some(),
            "原生 caret 布局必须继续存在"
        );
        window.press("cmd-a", cx);
        assert_eq!(input.read(cx).selected_range(), 0.."Hello 中文\nTail".len());
        window.press("cmd-c", cx);
        window.press("backspace", cx);
        assert_eq!(input.read(cx).value().as_ref(), "");
        window.press("cmd-v", cx);
        assert_eq!(input.read(cx).value().as_ref(), "Hello 中文\nTail");
        window.press("cmd-z", cx);
        assert_eq!(input.read(cx).value().as_ref(), "");
        window.press("cmd-shift-z", cx);
        assert_eq!(input.read(cx).value().as_ref(), "Hello 中文\nTail");
        window.render_frame(cx);
        assert_eq!(window.find(native_id).value(), Some("Hello 中文\nTail"));
        assert_eq!(inputs(&shell, cx)[1].read(cx).value().as_ref(), "");
    })
    .unwrap();
    assert_eq!(
        shell.read_with(cx, |shell, _| shell.monitor.catalog.editors[0].draft.hooks
            [0]
        .content
        .clone()),
        "Hello 中文\nTail"
    );
}
