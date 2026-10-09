//! 从真实管理页读取布局与绘制场景，验证徽标和图片字段的历史尺寸及动作所有权。

use super::super::{
    Shell,
    state::{ImageEntry, Route},
};
use crate::monitor::image_decode::MonitorImageSource;
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, Hsla, Pixels, Role, TestAppContext, Window, px, rgb,
    test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, AiTool, ImageFormat, public_monitor_ai_tools};
use std::sync::Arc;

/// 实际 GIF 进入 Codex 草稿，其余三个行为保留待选择状态，偏好只存测试内存。
fn setup(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 1400.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Management;
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor
            .catalog
            .editor_mut(AiTool::Codex)
            .unwrap()
            .draft
            .hooks[0]
            .image = "badge-fixture".into();
        let source = MonitorImageSource {
            id: "badge-fixture".into(),
            bytes: Arc::from(include_bytes!("../../../tests/fixtures/monitor-loop.gif").as_slice()),
            format: ImageFormat::Gif,
        };
        this.monitor.catalog.images = vec![ImageEntry {
            id: source.id.clone(),
            filename: "badge.gif".into(),
            format: source.format,
            source: Ok(source),
            views: Default::default(),
            residency: Default::default(),
        }];
        this.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    (handle, shell, writer)
}

/// 整数尺寸与间距不得因文字调整而漂移。
fn near(actual: Pixels, expected: f32) {
    assert!(
        (actual - px(expected)).abs() < px(0.03),
        "actual {actual:?}, expected {expected}"
    );
}

/// Taffy 独立吸附行盒两条边，非整数行高的误差必须小于一个设备像素。
fn line_box(window: &Window, actual: Pixels, expected: f32) {
    assert!(
        (actual - px(expected)).abs() < px(1. / window.scale_factor()),
        "actual {actual:?}, expected {expected} at scale {}",
        window.scale_factor()
    );
}

/// 徽标必须有实际一像素描边及指定透明度，不能用主题实色背景冒充 light 变体。
fn badge(window: &Window, id: &str, height: f32, radius: f32, base: u32) {
    let bounds = window.find(id.to_owned()).bounds();
    near(bounds.size.height, height);
    let scale = window.scale_factor();
    let quads = window.painted_quads();
    let quad = quads
        .iter()
        .find(|quad| {
            quad.bounds == bounds.scale(scale)
                && quad.background.as_solid() == Some(Hsla::from(rgb(base).opacity(0.09)))
        })
        .expect("徽标必须绘制历史 light 底色");
    let border = quads
        .iter()
        .find(|quad| {
            quad.bounds == bounds.scale(scale)
                && quad.border_color == Hsla::from(rgb(base).opacity(0.26))
        })
        .expect("徽标必须另行绘制历史 light 描边");
    assert_eq!(border.border_widths.top, px(1.).scale(scale));
    assert_eq!(quad.corner_radii.top_left, px(radius).scale(scale));
    assert_eq!(quad.corner_radii.bottom_right, px(radius).scale(scale));
}

/// 位置与计数使用 lg 胶囊，四种状态使用 md 圆角且各自保留 shade6 底色。
#[gpui_kit::test]
fn monitor_badges_follow_light_sizes_and_behavior_colors(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, _| {
        badge(window, "monitor-slot-badge", 26., 13., 0xc43b2d);
        badge(window, "monitor-count-badge", 26., 13., 0xc43b2d);
        badge(window, "monitor-state-badge-idle", 20., 8., 0x12b886);
        badge(window, "monitor-state-badge-running", 20., 8., 0xc43b2d);
        badge(window, "monitor-state-badge-asking", 20., 8., 0xfab005);
        badge(window, "monitor-state-badge-error", 20., 8., 0xfa5252);
    })
    .unwrap();
}

/// 标题和两个字段保持各自行盒，内容标签不能再把 textarea 向下推八像素。
#[gpui_kit::test]
fn monitor_titles_and_field_line_boxes_keep_historical_spacing(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, _| {
        for id in [
            "monitor-slot-title",
            "monitor-behaviors-title",
            "monitor-behavior-title-idle",
        ] {
            line_box(window, window.find(id).bounds().size.height, 24.8);
        }
        let image = window.find("monitor-image-label-idle").bounds();
        line_box(window, image.size.height, 20.3);
        let row = window.find("monitor-image-label-row-idle").bounds();
        let trigger = window.find("monitor-picker-trigger-idle").bounds();
        near(trigger.origin.y - row.bottom(), 7.);
        near(trigger.size.height, 132.);
        let content = window.find("monitor-content-label-idle").bounds();
        let optional = window.find("monitor-content-optional-idle").bounds();
        let input = window.find("monitor-content-input-idle").bounds();
        line_box(window, content.size.height, 21.7);
        line_box(window, optional.size.height, 16.8);
        assert!(
            optional.origin.y > content.origin.y,
            "较小的可选文本必须按基线排布"
        );
        near(input.origin.y - content.bottom(), 0.);
        line_box(window, input.size.height, 56.4);
    })
    .unwrap();
}

/// 清除是独立二十二像素原生按钮，点击不能展开图片选择器。
#[gpui_kit::test]
fn monitor_clear_is_small_native_action_and_owns_its_click(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let clear = window.find("monitor-picker-clear-idle");
        assert_eq!(clear.role(), Some(Role::Button));
        near(clear.bounds().size.width, 22.);
        near(clear.bounds().size.height, 22.);
        let icon = window.find("monitor-picker-clear-icon-idle").bounds();
        near(icon.size.width, 15.);
        near(icon.size.height, 15.);
        window.hover("monitor-picker-clear-idle", cx);
        assert!(
            window.painted_quads().iter().any(|quad| quad.bounds
                == clear.bounds().scale(window.scale_factor())
                && quad.background.as_solid() == Some(rgb(0xe9ecef).into())),
            "subtle 清除按钮悬停必须使用历史 gray2 背景"
        );
        window.click("monitor-picker-clear-idle", cx);
        assert!(
            shell.read(cx).monitor.catalog.editors[0].draft.hooks[0]
                .image
                .is_empty()
        );
        assert!(shell.read(cx).monitor.catalog.picker.is_none());
        badge(window, "monitor-state-badge-idle", 20., 8., 0x868e96);
    })
    .unwrap();
}

/// 更换标识只装饰图片按钮，保留十七像素图标、六像素间距和原触发区点击。
#[gpui_kit::test]
fn monitor_change_image_decoration_keeps_icon_padding_and_trigger_ownership(
    cx: &mut TestAppContext,
) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let decoration = window.find("monitor-image-change-idle");
        assert_eq!(decoration.role(), None, "装饰标识不能变成嵌套按钮");
        let bounds = decoration.bounds();
        let icon = window.find("monitor-image-change-icon-idle").bounds();
        let text = window.find("monitor-image-change-text-idle").bounds();
        near(icon.size.width, 17.);
        near(icon.size.height, 17.);
        near(icon.origin.x - bounds.origin.x, 9.);
        near(text.origin.x - icon.right(), 6.);
        near(bounds.right() - text.right(), 9.);
        line_box(window, bounds.size.height, 30.6);
        window.click("monitor-image-change-idle", cx);
        assert_eq!(
            shell.read(cx).monitor.catalog.picker,
            Some(loki_metris_core::HookBehavior::Idle)
        );
        assert_eq!(
            shell.read(cx).monitor.catalog.editors[0].draft.hooks[0].image,
            "badge-fixture"
        );
    })
    .unwrap();
}
