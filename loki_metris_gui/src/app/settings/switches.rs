//! 设置页开关使用原生语义及参考几何，颜色不依赖组件库的默认黑色滑块。

use gpui_kit::{
    App, ElementId, Hsla, IntoElement, RenderOnce, Window,
    base::{Easing, Switch, SwitchThumb, SwitchTrack, Transition, box_shadow, transition},
    component::ActiveTheme,
    div,
    prelude::*,
    px, rgb,
};
use std::time::Duration;

/// 视觉子树保存稳定动画通道，外层开关独立拥有输入与辅助语义。
#[derive(IntoElement)]
struct SwitchVisual {
    /// 动画通道的稳定元素 ID。
    id: ElementId,
    /// 开关处于开启状态。
    checked: bool,
    /// 开关禁用。
    disabled: bool,
    /// 轨道颜色。
    track: Hsla,
    /// 滑块颜色。
    thumb: Hsla,
}

impl RenderOnce for SwitchVisual {
    /// 首绘直接采用目标；后续滑块与轨道过渡，中央指示圆立即采用轨道目标色。
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let motion = Transition::new(Duration::from_millis(150)).easing(Easing::Ease);
        let thumb_x = transition(
            (self.id.clone(), "thumb-x"),
            px(if self.checked { 21.5 } else { 2.5 }),
            motion.clone(),
            window,
            cx,
        );
        let track = transition(
            (self.id.clone(), "track-color"),
            self.track,
            motion,
            window,
            cx,
        );
        div()
            .absolute()
            .inset_0()
            .child(
                SwitchTrack::new((self.id, "track"))
                    .checked(self.checked)
                    .disabled(self.disabled)
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .bg(track),
            )
            .child(
                SwitchThumb::new(self.checked)
                    .disabled(self.disabled)
                    .absolute()
                    .left(thumb_x)
                    .top(px(3.))
                    .size(px(14.))
                    .rounded_full()
                    .bg(self.thumb)
                    .child(
                        div()
                            .absolute()
                            .left(px(4.2))
                            .top(px(4.2))
                            .size(px(5.6))
                            .rounded_full()
                            .bg(self.track),
                    ),
            )
    }
}

/// 小号开关保留受控值、键盘和辅助语义；滑块与轨道同级，焦点不改变几何。
pub(super) fn settings_switch(
    id: &'static str,
    label: String,
    checked: bool,
    disabled: bool,
    cx: &App,
) -> Switch {
    let dark = cx.theme().mode.is_dark();
    let surface = rgb(if dark { 0x242424 } else { 0xffffff }).into();
    let track = if disabled {
        rgb(if dark { 0x2e2e2e } else { 0xe9ecef }).into()
    } else if checked {
        cx.theme().primary
    } else {
        rgb(if dark { 0x3b3b3b } else { 0xdee2e6 }).into()
    };
    let thumb = rgb(if disabled {
        if dark { 0x696969 } else { 0xf8f9fa }
    } else {
        0xffffff
    })
    .into();
    let ring = cx.theme().primary;
    let id: ElementId = id.into();
    Switch::new(id.clone())
        .checked(checked)
        .disabled(disabled)
        .accessibility_label(label)
        .relative()
        .w(px(38.))
        .h(px(20.))
        .flex_shrink_0()
        .rounded_full()
        .focus_visible(move |style| {
            style.shadow(vec![
                box_shadow(0., 0., 0., 4., ring),
                box_shadow(0., 0., 0., 2., surface),
            ])
        })
        .a11y_synthetic_children(move |builder| {
            if disabled {
                builder.parent_node().set_disabled();
            }
        })
        .child(SwitchVisual {
            id,
            checked,
            disabled,
            track,
            thumb,
        })
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - switches_tests.rs：本模块的单元测试
include!("switches_tests.rs");
