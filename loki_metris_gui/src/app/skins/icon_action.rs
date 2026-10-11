//! ActionIcon的瞬时键盘下压反馈；原生Button独占激活与取消语义。

use crate::app::surface::native_control::{FocusRing, NativeControl};
use gpui_kit::{
    App, Context, FocusHandle, IntoElement, RenderOnce, Subscription, Window, base::Button,
    component::ActiveTheme, prelude::*, px, rgb,
};

/// 只保存瞬时绘制状态；一次性订阅在失焦、隐藏和停用时清除持键反馈。
struct PressState {
    focus: FocusHandle,
    held: bool,
    _subscriptions: Vec<Subscription>,
}
impl PressState {
    /// 稳定owner持有真实焦点，订阅使用Context内部弱引用，不保活已卸载控件。
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let lost = cx.on_focus_out(&focus, window, |this, _, window, _| this.set(false, window));
        let activation = cx.observe_window_activation(window, |this, window, _| {
            if !window.is_window_active() {
                this.set(false, window);
            }
        });
        let visibility = cx.observe_window_visibility(window, |this, visibility, window, _| {
            if !visibility.is_visible() {
                this.set(false, window);
            }
        });
        Self {
            focus,
            held: false,
            _subscriptions: vec![lost, activation, visibility],
        }
    }
    /// 只有状态实际改变才刷新；不请求动画帧或调用业务动作。
    fn set(&mut self, held: bool, window: &mut Window) {
        if self.held != held {
            self.held = held;
            window.refresh();
        }
    }
}

/// 已配置按钮的最后一层绘制适配，业务回调仍绑定同一个原生按钮。
#[derive(IntoElement)]
pub(in crate::app) struct IconAction {
    button: Button,
    key: &'static str,
    disabled: bool,
}
/// 调用方完成尺寸、图标与动作配置后，再附加局部持键反馈。
pub(in crate::app) fn icon_action(button: Button, key: &'static str, disabled: bool) -> IconAction {
    IconAction {
        button,
        key,
        disabled,
    }
}
impl RenderOnce for IconAction {
    /// 观察真实事件但不阻断传播；Space/Enter的keyup仍由原生Button激活一次。
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let owner = window.use_keyed_state(self.key, cx, PressState::new);
        let (focus, held) = owner.update(cx, |state, _| {
            if self.disabled || !window.is_visible() || !state.focus.is_focused(window) {
                state.set(false, window);
            }
            (state.focus.clone(), state.held)
        });
        self.button = self.button.track_focus(&focus);
        if !self.disabled {
            let weak = owner.downgrade();
            self.button
                .interactivity()
                .capture_key_down(move |event, window, cx| {
                    let activation = matches!(event.keystroke.key.as_str(), "enter" | "space")
                        && !event.keystroke.modifiers.modified()
                        && !window.default_prevented();
                    let _ = weak.update(cx, |state, _| {
                        state.set(activation && state.focus.is_focused(window), window);
                    });
                });
            let weak = owner.downgrade();
            self.button
                .interactivity()
                .capture_key_up(move |_, window, cx| {
                    let _ = weak.update(cx, |state, _| state.set(false, window));
                });
        }
        let ring = rgb(if cx.theme().mode.is_dark() {
            0xd95a44
        } else {
            0xc43b2d
        });
        let button = self
            .button
            .when(held, |button| button.top(px(1.)))
            .disabled(self.disabled)
            .render(window, cx)
            .into_element();
        NativeControl::new(button, self.disabled).focus_ring(FocusRing {
            focus,
            radius: px(12.),
            gap: px(2.),
            width: px(2.),
            color: ring.into(),
        })
    }
}
