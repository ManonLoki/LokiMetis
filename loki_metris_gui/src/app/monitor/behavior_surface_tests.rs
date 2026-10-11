//! 使用真实管理页和原生绘制场景验证行为卡，不复制装饰算法。

use super::{Shell, state::Route};
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, Hsla, TestAppContext, px, rgb, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, public_monitor_ai_tools};

/// 默认公开档案足以进入真实编辑页，偏好仍由内存测试宿主拥有。
fn show_management(
    cx: &mut TestAppContext,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Management;
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    (handle, shell, writer)
}

/// 彩条必须覆盖卡片顶端，不能作为四像素内容行把标题向下推。
#[gpui_kit::test]
fn behavior_header_starts_at_inner_card_top(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = show_management(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let inner = window.find("monitor-behavior-idle").bounds().inset(px(1.));
        let scale = window.scale_factor();
        let tint: Hsla = rgb(0x868e96).opacity(0.06).into();
        let quads = window.painted_quads();
        let header = quads
            .iter()
            .find(|quad| {
                quad.background.as_solid() == Some(tint)
                    && quad.bounds.origin.x == inner.origin.x.scale(scale)
            })
            .expect("真实标题背景必须绘制 6% 不透明度的行为色");
        assert_eq!(
            header.bounds.origin.y,
            inner.origin.y.scale(scale),
            "绝对彩条不能使标题额外下移四像素"
        );
        assert_eq!(header.corner_radii.top_left, px(19.).scale(scale));
        assert_eq!(header.corner_radii.top_right, px(19.).scale(scale));
        assert_eq!(header.corner_radii.bottom_left, px(0.).scale(scale));
        assert_eq!(header.corner_radii.bottom_right, px(0.).scale(scale));
    })
    .unwrap();
}

/// 使用整张卡片的圆角形状和四像素裁剪区域，避免把彩条自身画成小胶囊。
#[gpui_kit::test]
fn behavior_accent_clips_the_full_inner_card_shape(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = show_management(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let inner = window.find("monitor-behavior-idle").bounds().inset(px(1.));
        let scale = window.scale_factor();
        let accent: Hsla = rgb(0x868e96).into();
        let quads = window.painted_quads();
        let strip = quads
            .iter()
            .find(|quad| {
                quad.background.as_solid() == Some(accent) && quad.bounds == inner.scale(scale)
            })
            .expect("彩条必须使用完整卡片内框绘制圆角形状");
        for radius in [
            strip.corner_radii.top_left,
            strip.corner_radii.top_right,
            strip.corner_radii.bottom_left,
            strip.corner_radii.bottom_right,
        ] {
            assert_eq!(radius, px(19.).scale(scale));
        }
        assert_eq!(strip.content_mask.bounds.origin, inner.origin.scale(scale));
        assert_eq!(
            strip.content_mask.bounds.size.width,
            inner.size.width.scale(scale)
        );
        assert_eq!(strip.content_mask.bounds.size.height, px(4.).scale(scale));
    })
    .unwrap();
}

/// 卡片外框保留一像素边框，外圆角二十像素对应内侧十九像素。
#[gpui_kit::test]
fn behavior_outer_card_uses_twenty_pixel_corners(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = show_management(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("monitor-behavior-idle").bounds();
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        let border = quads
            .iter()
            .find(|quad| {
                quad.bounds == card.scale(scale) && quad.border_widths.top == px(1.).scale(scale)
            })
            .expect("真实行为卡必须保留原生一像素边框");
        for radius in [
            border.corner_radii.top_left,
            border.corner_radii.top_right,
            border.corner_radii.bottom_left,
            border.corner_radii.bottom_right,
        ] {
            assert_eq!(radius, px(20.).scale(scale));
        }
    })
    .unwrap();
}
