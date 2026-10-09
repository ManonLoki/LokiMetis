//! 行为卡装饰仅绘制原生圆角形状，不参与流布局、命中或图片生命周期。

use gpui_kit::{AnyElement, ContentMask, Corners, Hsla, IntoElement, canvas, fill, prelude::*, px};

const INNER_RADIUS: f32 = 19.;

/// 标题背景贴住一像素卡片边框内侧，底边保持直线连接内容区。
pub(super) fn header_background(color: Hsla) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.paint_quad(fill(bounds, color.opacity(0.06)).corner_radii(Corners {
                top_left: px(INNER_RADIUS),
                top_right: px(INNER_RADIUS),
                bottom_left: px(0.),
                bottom_right: px(0.),
            }));
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

/// 用整张内卡的圆角保持原圆心，再以顶部四像素矩形限制彩条高度。
pub(super) fn top_accent(color: Hsla) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut strip = bounds;
            strip.size.height = px(4.);
            window.with_content_mask(Some(ContentMask { bounds: strip }), |window| {
                window.paint_quad(fill(bounds, color).corner_radii(Corners::all(px(INNER_RADIUS))));
            });
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}
