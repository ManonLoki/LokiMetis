//! 皮肤徽标映射安装版Mantine默认md尺寸、light／dot／outline色阶。
use gpui_kit::{
    AnyElement, ElementId, FontWeight, IntoElement, base::TestSupportExt as _, div, prelude::*, px,
    rgb,
};

/// 标签类别只决定展示，不包含资源选择或应用动作。
#[derive(Clone, Copy)]
pub(super) enum SkinBadge {
    Theme,
    Legacy,
    Source { user: bool },
    Mode,
}

/// 所有徽标保留20px边框盒、18px行高、10px水平内距和胶囊圆角。
pub(super) fn skin_badge(
    id: impl Into<ElementId>,
    label: String,
    kind: SkinBadge,
    dark: bool,
) -> AnyElement {
    let mut badge = div()
        .id(id)
        .test_support()
        .h(px(20.))
        .px(px(10.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .whitespace_nowrap()
        .overflow_hidden()
        .rounded_full()
        .border_1()
        .text_size(px(11.))
        .line_height(px(18.))
        .font_weight(FontWeight::BOLD);
    match kind {
        SkinBadge::Theme | SkinBadge::Legacy => {
            let (base, text) = match kind {
                SkinBadge::Theme => (0xc43b2d, if dark { 0xfff2ee } else { 0x6b1e18 }),
                _ => (0x868e96, if dark { 0xf8f9fa } else { 0x212529 }),
            };
            badge = badge
                .bg(rgb(base).opacity(0.09))
                .border_color(rgb(base).opacity(0.26))
                .text_color(rgb(text));
        }
        SkinBadge::Source { user } => {
            let background = rgb(if dark { 0x3b3b3b } else { 0xffffff });
            let dot = 20. / 3.4;
            badge = badge
                .bg(background)
                .border_color(rgb(if dark { 0x3b3b3b } else { 0xced4da }))
                .text_color(rgb(if dark { 0xffffff } else { 0x000000 }))
                .gap(px(dot))
                .child(
                    div()
                        .size(px(dot))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(rgb(if user { 0x12b886 } else { 0x228be6 })),
                );
        }
        SkinBadge::Mode => {
            let color = rgb(if dark { 0xfde1d8 } else { 0xc43b2d });
            badge = badge.border_color(color).text_color(color);
        }
    }
    // Mantine还有0.25px字距；当前文字接口尚无对应合同，不插入伪造空格。
    badge.child(label.to_uppercase()).into_any_element()
}
