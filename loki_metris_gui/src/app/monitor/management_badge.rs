//! 管理页 light 徽标直接映射历史 Mantine 尺寸与五色阶，不修改全局主题。

use gpui_kit::{
    AnyElement, ElementId, FontWeight, IntoElement, base::TestSupportExt as _, div, prelude::*, px,
    rgb,
};
use loki_metris_core::HookBehavior;

/// 徽标颜色只控制展示，行为头部彩条仍保留自己的强调色。
#[derive(Clone, Copy)]
pub(super) enum BadgeColor {
    Brand,
    Teal,
    Gray,
    Yellow,
    Red,
}

impl BadgeColor {
    /// 空图状态使用行为色，任意非空引用保持历史已配置展示语义。
    pub(super) fn for_behavior(behavior: HookBehavior, configured: bool) -> Self {
        if configured {
            return Self::Teal;
        }
        match behavior {
            HookBehavior::Idle => Self::Gray,
            HookBehavior::Running => Self::Brand,
            HookBehavior::Asking => Self::Yellow,
            _ => Self::Red,
        }
    }

    /// light 填充和边框始终使用 shade6，文字分别取亮色 shade9 与暗色 shade0。
    fn shades(self) -> (u32, u32, u32) {
        match self {
            Self::Brand => (0xc43b2d, 0x6b1e18, 0xfff2ee),
            Self::Teal => (0x12b886, 0x087f5b, 0xe6fcf5),
            Self::Gray => (0x868e96, 0x212529, 0xf8f9fa),
            Self::Yellow => (0xfab005, 0xe67700, 0xfff9db),
            Self::Red => (0xfa5252, 0xc92a2a, 0xfff5f5),
        }
    }
}

/// lg 为二十六像素胶囊，md 为二十像素八像素圆角；一像素边框包含在总高内。
pub(super) fn light_badge(
    id: impl Into<ElementId>,
    label: String,
    color: BadgeColor,
    large: bool,
    dark: bool,
) -> AnyElement {
    let (base, light_text, dark_text) = color.shades();
    let (height, font, padding, radius) = if large {
        (26., 13., 12., 13.)
    } else {
        (20., 11., 10., 8.)
    };
    div()
        .id(id)
        .test_support()
        .h(px(height))
        .px(px(padding))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .whitespace_nowrap()
        .rounded(px(radius))
        .border_1()
        .border_color(rgb(base).opacity(0.26))
        .bg(rgb(base).opacity(0.09))
        .text_color(rgb(if dark { dark_text } else { light_text }))
        .text_size(px(font))
        .line_height(px(height - 2.))
        .font_weight(FontWeight::BOLD)
        .child(label.to_uppercase())
        .into_any_element()
}
