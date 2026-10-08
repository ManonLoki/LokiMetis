//! 固定深色的独立桌宠设置窗，所有控件发往应用级owner。

use super::{Controller, host};
use gpui_kit::{
    Context, Entity, FocusHandle, FontWeight, IntoElement, MouseButton, Render, Role, Subscription,
    Window, base::TestSupportExt as _, component::slider::SliderState, div, prelude::*, px, rgb,
    rgba,
};
use loki_metris_core::{PetLayout, PetPreferenceChange};

/// 控件实体只属于此设置窗；关闭不影响controller中的偏好提交。
pub(super) struct Settings {
    owner: Entity<Controller>,
    slider_focus: FocusHandle,
    slider: Entity<SliderState>,
    range: (u16, u16),
    _owner: Subscription,
    _slider: Subscription,
}
impl Settings {
    /// 首次创建读取已确认偏好，输入状态跨同窗重绘保持。
    pub fn new(owner: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("LokiMetris Pet Settings");
        let prefs = owner.read(cx).settings.pet_window;
        let handle = owner.read(cx).overlay;
        let range = super::range_for(handle, prefs.layout, cx);
        let slider = cx.new(|_| {
            SliderState::new()
                .min(f32::from(range.min))
                .max(f32::from(range.max))
                .step(1.)
                .default_value(f32::from(range.constrain(prefs.pet_size)))
        });
        let owner_observe = cx.observe(&owner, |_, _, cx| cx.notify());
        let slider_owner = owner.clone();
        let slider_observe = cx.observe(&slider, move |_, slider, cx| {
            let value = slider.read(cx).value().end().round() as u16;
            slider_owner.update(cx, |owner, cx| {
                if value != owner.settings.pet_window.pet_size {
                    owner.change(PetPreferenceChange::Size(value), cx);
                }
            });
        });
        let close_owner = owner.clone();
        window.on_window_should_close(cx, move |_, cx| {
            close_owner.update(cx, |owner, _| owner.settings_window = None);
            true
        });
        Self {
            owner,
            slider_focus: cx.focus_handle(),
            slider,
            range: (range.min, range.max),
            _owner: owner_observe,
            _slider: slider_observe,
        }
    }
    /// 操作期间控件继续发字段意图，界面不复制另一份偏好。
    fn change(&mut self, value: PetPreferenceChange, cx: &mut Context<Self>) {
        self.owner.update(cx, |owner, cx| owner.change(value, cx));
    }
}
impl Render for Settings {
    /// 独立设置使用参考固定深色，不受主界面浅色主题影响。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs = self.owner.read(cx).settings.pet_window;
        let handle = self.owner.read(cx).overlay;
        let range = super::range_for(handle, prefs.layout, cx);
        let value = range.constrain(prefs.pet_size);
        if self.range != (range.min, range.max) {
            self.slider.update(cx, |state, _| {
                *state = SliderState::new()
                    .min(f32::from(range.min))
                    .max(f32::from(range.max))
                    .step(1.)
                    .default_value(f32::from(value))
            });
            self.range = (range.min, range.max);
        } else if self.slider.read(cx).value().end().round() as u16 != value {
            self.slider.update(cx, |state, cx| {
                state.set_value(f32::from(value), window, cx)
            });
        }
        let t = |key| self.owner.read(cx).t(key);
        let title = t("monitor.pet.settingsTitle");
        let hint = t("monitor.pet.settingsHint");
        let close = t("monitor.pet.settingsClose");
        let display = t("monitor.pet.displayCount");
        let layout = t("monitor.pet.layout");
        let size_label = t("monitor.pet.size");
        let top = t("monitor.pet.alwaysOnTop");
        let lock = t("monitor.pet.lockPositionSize");
        let main = t("monitor.pet.returnMain");
        let hide = t("monitor.pet.hideToTray");
        let mut segments = div()
            .id("pet-layouts")
            .test_support()
            .grid()
            .grid_cols(2)
            .gap(px(3.))
            .role(Role::Group)
            .aria_label(layout);
        for (layout, label) in [
            (PetLayout::Single, "1×1"),
            (PetLayout::Row, "1×2"),
            (PetLayout::Column, "2×1"),
            (PetLayout::Row3, "1×3"),
            (PetLayout::Column3, "3×1"),
            (PetLayout::Grid, "2×2"),
        ] {
            segments = segments.child(
                gpui_kit::base::Button::new(format!("pet-layout-{layout:?}"))
                    .h(px(28.))
                    .rounded(px(7.))
                    .text_size(px(11.))
                    .bg(if prefs.layout == layout {
                        rgba(0x345ec9ff)
                    } else {
                        rgba(0xffffff0f)
                    })
                    .selected(prefs.layout == layout)
                    .accessibility_label(label)
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.change(PetPreferenceChange::Layout(layout), cx)
                    })),
            );
        }
        let check = |id: &'static str,
                     label: String,
                     checked: bool,
                     change: PetPreferenceChange,
                     _cx: &mut Context<Self>| {
            let owner = self.owner.clone();
            gpui_kit::base::Switch::new(id)
                .checked(checked)
                .accessibility_label(label.clone())
                .w_full()
                .min_h(px(31.))
                .py(px(5.))
                .px(px(8.))
                .rounded(px(7.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .child(
                    div()
                        .w(px(18.))
                        .font_weight(FontWeight(800.))
                        .text_color(rgb(0x8eb0ff))
                        .child(if checked { "✓" } else { "" }),
                )
                .child(label)
                .on_change(move |_, _, _, cx| {
                    owner.update(cx, |owner, cx| owner.change(change, cx))
                })
        };
        let error = self
            .owner
            .read(cx)
            .last_error
            .as_ref()
            .map(|error| t(error.code));
        div()
            .id("pet-settings")
            .test_support()
            .size_full()
            .overflow_y_scroll()
            .p(px(14.))
            .text_color(rgb(0xeef2fb))
            .bg(rgb(0x11151d))
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt(px(2.))
                    .px(px(3.))
                    .pb(px(12.))
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        host::start_drag(window, cx)
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt(px(3.))
                                    .text_size(px(10.))
                                    .text_color(rgb(0x7f899c))
                                    .truncate()
                                    .child(hint),
                            ),
                    )
                    .child(
                        gpui_kit::base::Button::new("pet-settings-close")
                            .accessibility_label(close)
                            .flex_shrink_0()
                            .size(px(28.))
                            .rounded(px(8.))
                            .bg(rgb(0x202632))
                            .text_color(rgb(0xaab3c2))
                            .text_size(px(20.))
                            .child("×")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner.update(cx, |owner, cx| owner.close_settings(cx))
                            })),
                    ),
            )
            .when_some(error, |surface, error| {
                surface.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0xff9b9b))
                        .mb(px(6.))
                        .child(error),
                )
            })
            .child(
                div()
                    .w_full()
                    .p(px(10.))
                    .rounded(px(13.))
                    .border_1()
                    .border_color(rgba(0xffffff24))
                    .bg(rgba(0x131720f0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pt(px(5.))
                            .px(px(5.))
                            .pb(px(4.))
                            .text_size(px(10.))
                            .text_color(rgb(0x9099aa))
                            .child(display),
                    )
                    .child(segments)
                    .child(
                        div()
                            .pt(px(5.))
                            .px(px(5.))
                            .pb(px(4.))
                            .text_size(px(10.))
                            .text_color(rgb(0x9099aa))
                            .child(size_label.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .px(px(5.))
                            .pt(px(2.))
                            .pb(px(7.))
                            .child(div().flex_1().min_w_0().child(super::size_control::render(
                                &self.slider,
                                &self.slider_focus,
                                size_label.clone(),
                                window,
                                cx,
                            )))
                            .child(
                                div()
                                    .w(px(43.))
                                    .text_right()
                                    .text_size(px(12.))
                                    .text_color(rgb(0xc7cfdd))
                                    .child(format!("{value}px")),
                            ),
                    )
                    .child(check(
                        "pet-top",
                        top,
                        prefs.always_on_top,
                        PetPreferenceChange::AlwaysOnTop(!prefs.always_on_top),
                        cx,
                    ))
                    .child(check(
                        "pet-lock",
                        lock,
                        prefs.locked,
                        PetPreferenceChange::Locked(!prefs.locked),
                        cx,
                    ))
                    .child(div().h(px(1.)).my(px(6.)).mx(px(2.)).bg(rgba(0xffffff1a)))
                    .child(
                        gpui_kit::base::Button::new("pet-return-main")
                            .accessibility_label(main.clone())
                            .w_full()
                            .min_h(px(31.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .text_size(px(12.))
                            .justify_start()
                            .child(main)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner.update(cx, |owner, cx| owner.show_main(cx))
                            })),
                    )
                    .child(
                        gpui_kit::base::Button::new("pet-hide")
                            .accessibility_label(hide.clone())
                            .w_full()
                            .min_h(px(31.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .text_size(px(12.))
                            .justify_start()
                            .child(hide)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.owner.update(cx, |owner, cx| owner.hide(cx))
                            })),
                    ),
            )
    }
}
