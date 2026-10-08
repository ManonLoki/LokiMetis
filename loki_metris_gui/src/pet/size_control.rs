//! 桌宠尺寸滑块组合基础轨道与完整的键盘、辅助赋值契约。

use gpui_kit::{
    AccessibleAction, Entity, FocusHandle, IntoElement, MouseButton, Role, Window,
    accesskit::{ActionData, Orientation},
    base::{SliderIndicator, SliderThumb, SliderTrack, TestSupportExt as _},
    component::slider::SliderState,
    div,
    prelude::*,
    px, relative, rgb,
};

/// 平台可传数字或字符串；非有限数值不能进入尺寸转换。
fn numeric_value(data: Option<&ActionData>) -> Option<f32> {
    let value = match data? {
        ActionData::NumericValue(value) => *value,
        ActionData::Value(value) => value.parse::<f64>().ok()?,
        _ => return None,
    };
    value
        .is_finite()
        .then_some(value as f32)
        .filter(|v| v.is_finite())
}

/// 继续使用工具包的拖动和轨道计算，只补齐其缺少的SetValue入口。
pub(super) fn render(
    state: &Entity<SliderState>,
    focus: &FocusHandle,
    label: String,
    window: &mut Window,
    cx: &gpui_kit::App,
) -> impl IntoElement {
    let current = state.read(cx);
    let fraction = (current.value().end() - current.min_value())
        / (current.max_value() - current.min_value()).max(1.);
    let input = state.clone();
    let keyboard = state.clone();
    let up = state.clone();
    let down = state.clone();
    let focus_on_down = focus.clone();
    div()
        .id("pet-size-control")
        .test_support()
        .w_full()
        .h(px(24.))
        .flex()
        .items_center()
        .track_focus(focus)
        .role(Role::Slider)
        .aria_label(label)
        .aria_numeric_value(f64::from(current.value().end()))
        .aria_min_numeric_value(f64::from(current.min_value()))
        .aria_max_numeric_value(f64::from(current.max_value()))
        .aria_numeric_value_step(1.)
        .aria_orientation(Orientation::Horizontal)
        .capture_any_mouse_down(move |_, window, cx| focus_on_down.focus(window, cx))
        .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
            if let Some(value) = numeric_value(data) {
                input.update(cx, |state, cx| state.set_value(value.round(), window, cx));
            }
        })
        .on_a11y_action(AccessibleAction::Increment, move |_, window, cx| {
            up.update(cx, |state, cx| {
                state.set_value(state.value().end() + 1., window, cx)
            });
        })
        .on_a11y_action(AccessibleAction::Decrement, move |_, window, cx| {
            down.update(cx, |state, cx| {
                state.set_value(state.value().end() - 1., window, cx)
            });
        })
        .on_key_down(move |event, window, cx| {
            keyboard.update(cx, |state, cx| {
                let value = match event.keystroke.key.as_str() {
                    "left" | "down" => state.value().end() - 1.,
                    "right" | "up" => state.value().end() + 1.,
                    "home" => state.min_value(),
                    "end" => state.max_value(),
                    _ => return,
                };
                cx.stop_propagation();
                state.set_value(value, window, cx);
            });
        })
        .on_mouse_up(
            MouseButton::Left,
            window.listener_for(state, |state, _, _, cx| state.handle_release(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            window.listener_for(state, |state, _, _, cx| state.handle_release(cx)),
        )
        .child(
            SliderTrack::new(state)
                .w_full()
                .h(px(24.))
                .flex()
                .items_center()
                .child(
                    SliderIndicator::new(state)
                        .relative()
                        .w_full()
                        .h(px(4.))
                        .rounded_full()
                        .bg(rgb(0x2d3548))
                        .child(
                            div()
                                .absolute()
                                .h_full()
                                .left_0()
                                .right(relative(1. - fraction))
                                .rounded_full()
                                .bg(rgb(0x5b83e8)),
                        )
                        .child(
                            SliderThumb::new(state)
                                .absolute()
                                .top(px(-6.))
                                .left(relative(fraction))
                                .ml(px(-8.))
                                .size(px(16.))
                                .rounded_full()
                                .bg(rgb(0x5b83e8)),
                        ),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS数字与Windows字符串都接收，损坏或非有限值保持当前偏好。
    #[test]
    fn platform_value_payloads_reject_non_finite_input() {
        assert_eq!(
            numeric_value(Some(&ActionData::NumericValue(76.))),
            Some(76.)
        );
        assert_eq!(
            numeric_value(Some(&ActionData::Value("77".into()))),
            Some(77.)
        );
        for value in ["NaN", "inf", "invalid", "1e300"] {
            assert_eq!(numeric_value(Some(&ActionData::Value(value.into()))), None);
        }
        assert_eq!(numeric_value(None), None);
    }
}
