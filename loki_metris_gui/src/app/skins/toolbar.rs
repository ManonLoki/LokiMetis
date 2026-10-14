//! 工具栏复用原生输入语义，颜色与瞬时下压按历史 Tabs / ActionIcon 分别处理。

use crate::app::surface::native_control::{FocusRing, NativeControl};
use crate::app::surface::{
    SurfaceButtonVariant, motion::CssMotion, surface_button_colors as colors,
};
use gpui_kit::{
    App, FocusHandle, HoverListenerMode, IntoElement, RenderOnce, Rgba, Window, base::Button,
    component::ActiveTheme, prelude::*, px, rgb, transparent_black,
};
use std::time::Duration;

#[path = "icon_action.rs"]
mod icon_action;
pub(super) use icon_action::icon_action;

/// 与历史主题变量保持同一颜色及透明度，不能用普通页面灰底替换。
pub(super) fn background(dark: bool) -> Rgba {
    rgb(if dark { 0x18191b } else { 0xfffcf7 }).opacity(if dark { 0.94 } else { 0.92 })
}

/// ActionIcon没有Button的200ms缩放过渡；原生Button仍独占点击、焦点与禁用。
pub(super) fn default_icon_button(id: &'static str, disabled: bool, cx: &App) -> Button {
    let dark = cx.theme().mode.is_dark();
    let (background, border, foreground) =
        colors(SurfaceButtonVariant::Default, dark, disabled, false);
    let hovered = colors(SurfaceButtonVariant::Default, dark, false, true).0;
    Button::new(id)
        .relative()
        .flex_shrink_0()
        .border_1()
        .border_color(border)
        .bg(background)
        .text_color(foreground)
        .cursor_pointer()
        .when(!disabled, |button| {
            button
                .hover(move |style| style.bg(hovered))
                .active(|style| style.top(px(1.)))
        })
        .when(disabled, |button| button.cursor_not_allowed())
        .disabled(disabled)
}

/// 只有颜色改变时申请下一帧；静态标签不保留轮询或常驻动画。
struct TabMotion {
    hovered: bool,
    background: CssMotion<4>,
    foreground: CssMotion<4>,
}

#[derive(IntoElement)]
/// 单个宿主标签的绘制适配器，业务动作和原生焦点由内层Button保留。
pub(super) struct HostTab {
    button: Button,
    key: String,
    selected: bool,
    disabled: bool,
    focus: FocusHandle,
}

/// 由调用处提供唯一按钮ID和业务回调，适配器只拥有短暂绘制状态。
pub(super) fn host_tab(
    button: Button,
    key: String,
    focus: FocusHandle,
    selected: bool,
    disabled: bool,
) -> HostTab {
    HostTab {
        button,
        key,
        selected,
        disabled,
        focus,
    }
}

/// 插值预乘alpha，避免透明背景过渡出现黑边。
fn channels(color: Rgba) -> [f64; 4] {
    [
        f64::from(color.r * color.a),
        f64::from(color.g * color.a),
        f64::from(color.b * color.a),
        f64::from(color.a),
    ]
}
/// 从预乘通道恢复绘制色；透明端点不进行除零。
fn color(channels: [f64; 4]) -> Rgba {
    let a = channels[3] as f32;
    if a == 0. {
        return transparent_black().into();
    }
    Rgba {
        r: channels[0] as f32 / a,
        g: channels[1] as f32 / a,
        b: channels[2] as f32 / a,
        a,
    }
}

impl RenderOnce for HostTab {
    /// 挂载直接采用端点，快速反向沿当前位置连续恢复。
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dark = cx.theme().mode.is_dark();
        let owner = window.use_keyed_state(self.key, cx, |_, cx| {
            let now = cx.background_executor().now();
            let duration = Duration::from_millis(160);
            TabMotion {
                hovered: false,
                background: CssMotion::with_duration(now, duration),
                foreground: CssMotion::with_duration(now, duration),
            }
        });
        let (background, foreground, running) = owner.update(cx, |state, cx| {
            let (background, foreground) = if self.selected {
                (
                    rgb(if dark { 0x3c241d } else { 0xfbe2d6 }),
                    rgb(if dark { 0xffb18e } else { 0x94291f }),
                )
            } else if state.hovered {
                (
                    rgb(if dark { 0x2a2622 } else { 0xf0e9de }),
                    rgb(if dark { 0xf1ede8 } else { 0x2f2922 }),
                )
            } else {
                (
                    transparent_black().into(),
                    rgb(if dark { 0xbdb7af } else { 0x655d54 }),
                )
            };
            let now = cx.background_executor().now();
            state.background.set_target(channels(background), now);
            state.foreground.set_target(channels(foreground), now);
            let (background, a) = state.background.sample(now);
            let (foreground, b) = state.foreground.sample(now);
            (color(background), color(foreground), a || b)
        });
        let weak = owner.downgrade();
        self.button
            .interactivity()
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent);
        self.button
            .interactivity()
            .on_hover(move |hovered, window, cx| {
                let changed = weak
                    .update(cx, |state, _| {
                        let changed = state.hovered != *hovered;
                        state.hovered = *hovered;
                        changed
                    })
                    .unwrap_or(false);
                if changed {
                    window.refresh();
                }
            });
        if running && window.is_visible() {
            window.request_animation_frame();
        }
        let ring =
            rgb(if dark { 0xff9b6f } else { 0xc43b2d }).opacity(if dark { 0.35 } else { 0.24 });
        let button = self
            .button
            .bg(background)
            .text_color(foreground)
            .font_weight(gpui_kit::FontWeight::BOLD)
            .text_size(px(14.08))
            .line_height(px(14.08))
            .py(px(8.8))
            .px(px(12.))
            .rounded(px(10.4))
            .flex_shrink_0()
            .cursor_pointer()
            .when(self.disabled, |button| {
                button.opacity(0.5).cursor_not_allowed()
            })
            .disabled(self.disabled)
            .render(window, cx)
            .into_element();
        NativeControl::new(button, self.disabled).focus_ring(FocusRing {
            focus: self.focus,
            radius: px(10.4),
            gap: px(0.),
            width: px(3.),
            color: ring,
        })
    }
}

#[cfg(test)]
#[path = "toolbar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "toolbar_input_tests.rs"]
mod input_tests;
