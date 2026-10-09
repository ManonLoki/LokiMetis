//! 管理页映射真实公开档案、十二展示位、四行为和本机图片选择器。

#[cfg(test)]
#[path = "management_geometry_tests.rs"]
mod geometry_tests;

#[cfg(test)]
#[path = "management_button_tests.rs"]
mod button_tests;

#[path = "management_badge.rs"]
mod badge;

use super::Shell;
use badge::{BadgeColor, light_badge};
use crate::app::surface::{HistoricalButtonSize, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, Role, Window,
    base::TestSupportExt as _,
    component::{
        ActiveTheme, Disableable,
        input::Textarea,
    },
    div,
    prelude::*,
    px, relative, rgb,
};
use loki_metris_core::HookBehavior;

impl Shell {
    /// 草稿未加载或读取失败不渲染可保存假值。
    pub(super) fn monitor_management(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = &self.monitor.catalog;
        if let Some(error) = &state.profiles_error {
            return self.monitor_catalog_error(error, true, cx);
        }
        if let Some(error) = &state.images_error {
            return self.monitor_catalog_error(error, false, cx);
        }
        if let Some(code) = self.agent_settings.error {
            return self.monitor_catalog_error(&loki_metris_core::HookError::new(code), true, cx);
        }
        let mut page = div()
            .id("monitor-management")
            .test_support()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .when_some(self.monitor_feedback(cx), |page, feedback| {
                page.child(feedback)
            });
        if !state.profiles_loaded || self.agent_settings.loading {
            return page
                .child(div().p_8().child(self.t("monitor.picker.loading")))
                .into_any_element();
        }
        let Some(tool) = self.monitor_tool() else {
            return page
                .child(
                    div()
                        .p_4()
                        .rounded(px(12.))
                        .bg(cx.theme().accent)
                        .child(self.t("monitor.management.noClient")),
                )
                .into_any_element();
        };
        let Some(editor) = state
            .editors
            .iter()
            .find(|editor| editor.draft.tool == tool)
        else {
            return page.into_any_element();
        };
        let draft = &editor.draft;
        let mut tabs = div()
            .id("monitor-agent-tabs")
            .test_support()
            .role(Role::TabList)
            .w_full()
            .flex()
            .gap_1();
        for candidate in self.monitor_tools() {
            tabs = tabs.child(
                gpui_kit::base::Button::new(format!(
                    "monitor-agent-{}",
                    loki_metris_core::ai_tool_name(candidate)
                ))
                .selected(candidate == tool)
                .role(Role::Tab)
                .aria_selected(candidate == tool)
                .px_3()
                .rounded(px(12.))
                .h(px(32.))
                .border_b_0()
                .border_color(if candidate == tool {
                    cx.theme().primary
                } else {
                    cx.theme().border.opacity(0.)
                })
                .accessibility_label(super::monitor_tool_display_name(candidate))
                .child(super::monitor_tool_display_name(candidate))
                .when(candidate == tool, |button| {
                    button
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .on_click(
                    cx.listener(move |this, _, _, cx| this.select_monitor_tool(candidate, cx)),
                ),
            );
        }
        page = page.child(tabs);
        let mut slots = div()
            .id("monitor-slot-grid")
            .test_support()
            .role(Role::Group)
            .aria_label(self.t("monitor.slot.groupAria"))
            .w_full()
            .grid()
            .grid_cols(12)
            .gap(px(7.))
            .p(px(10.))
            .rounded(px(16.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(rgb(if cx.theme().mode.is_dark() {
                0x1e1c1a
            } else {
                0xf0ebe3
            }));
        let range = loki_metris_core::profile_slot_range();
        for slot in range.min..=range.max {
            let label = self
                .t("monitor.slot.cellAria")
                .replace("%{slot}", &slot.to_string())
                .replace("%{row}", "1")
                .replace("%{column}", &slot.to_string());
            slots = slots.child(
                div().flex().justify_center().child(
                    gpui_kit::base::Button::new(("monitor-slot", usize::from(slot)))
                        .selected(slot == draft.slot)
                        .role(Role::Button)
                        .aria_toggled(if slot == draft.slot {
                            gpui_kit::accesskit::Toggled::True
                        } else {
                            gpui_kit::accesskit::Toggled::False
                        })
                        .size(px(40.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(rgb(if cx.theme().mode.is_dark() {
                            0x191715
                        } else {
                            0xffffff
                        }))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .accessibility_label(label)
                        .child(slot.to_string())
                        .when(slot == draft.slot, |button| {
                            button.bg(rgb(0x7048e8)).text_color(rgb(0xffffff))
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_monitor_slot(tool, slot, cx)
                        })),
                ),
            );
        }
        page = page.child(
            crate::app::surface::SpotlightCard::new("monitor-slot-card", cx.theme().mode.is_dark())
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .radius(px(12.))
                .top_accent(px(3.), rgb(0xd95a44).into())
                .test_support()
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .id("monitor-slot-title")
                                .test_support()
                                .text_size(px(16.))
                                .line_height(relative(1.55))
                                .font_weight(FontWeight::BOLD)
                                .child(self.t("monitor.slot.title")),
                        )
                        .child(light_badge(
                            "monitor-slot-badge",
                            self.t("monitor.slot.position")
                                .replace("%{slot}", &draft.slot.to_string()),
                            BadgeColor::Brand,
                            true,
                            cx.theme().mode.is_dark(),
                        )),
                )
                .child(slots),
        );
        let configured = draft
            .hooks
            .iter()
            .filter(|hook| {
                state
                    .images
                    .iter()
                    .any(|image| image.id == hook.image && image.source.is_ok())
            })
            .count();
        let count_label = self
            .t("monitor.management.configuredCount")
            .replace("%{configured}", &configured.to_string())
            .replace("%{total}", &draft.hooks.len().to_string());
        page = page.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .id("monitor-behaviors-title")
                        .test_support()
                        .text_size(px(16.))
                        .line_height(relative(1.55))
                        .font_weight(FontWeight::BOLD)
                        .child(self.t("monitor.management.behaviorDisplay")),
                )
                .child(light_badge(
                    "monitor-count-badge",
                    count_label,
                    BadgeColor::Brand,
                    true,
                    cx.theme().mode.is_dark(),
                )),
        );
        if state.images_loaded && !state.images.iter().any(|image| image.source.is_ok()) {
            page = page.child(
                div()
                    .p_4()
                    .rounded(px(12.))
                    .text_color(rgb(0xfab005))
                    .bg(rgb(0xfab005).opacity(0.08))
                    .child(self.t("monitor.management.noImages")),
            );
        }
        let mut behaviors = div()
            .id("monitor-behaviors")
            .test_support()
            .w_full()
            .flex_shrink_0()
            .grid()
            .grid_cols(if self.viewport_width >= 768. { 2 } else { 1 })
            .gap_5();
        for (index, hook) in draft.hooks.iter().enumerate() {
            let behavior = hook.behavior;
            let key = behavior_key(behavior);
            let color = rgb(match behavior {
                HookBehavior::Idle => 0x868e96,
                HookBehavior::Running => 0xc43b2d,
                HookBehavior::Asking => 0xf59f00,
                _ => 0xf03e3e,
            });
            let mut card = div()
                .id(format!("monitor-behavior-{key}"))
                .test_support()
                .relative()
                .min_w_0()
                .min_h(px(356.))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .overflow_hidden()
                .rounded(px(20.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    div()
                        .id(format!("monitor-behavior-header-{key}"))
                        .test_support()
                        .relative()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(16.))
                        .px(px(18.))
                        .pt(px(18.))
                        .pb(px(14.))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(super::behavior_surface::header_background(color.into()))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(div().size(px(10.)).rounded_full().bg(color))
                                .child(
                                    div()
                                        .id(format!("monitor-behavior-title-{key}"))
                                        .test_support()
                                        .text_size(px(16.))
                                        .line_height(relative(1.55))
                                        .font_weight(FontWeight::BOLD)
                                        .child(self.t(&format!("monitor.behavior.{key}"))),
                                ),
                        )
                        .child(light_badge(
                            format!("monitor-state-badge-{key}"),
                            self.t(if hook.image.is_empty() {
                                "monitor.management.notConfigured"
                            } else {
                                "monitor.management.configured"
                            }),
                            BadgeColor::for_behavior(behavior, !hook.image.is_empty()),
                            false,
                            cx.theme().mode.is_dark(),
                        )),
                );
            let mut content = div()
                .w_full()
                .p(px(18.))
                .flex()
                .flex_col()
                .gap_4()
                .child(self.monitor_image_picker(tool, behavior, &hook.image, window, cx));
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(format!("monitor-content-label-{key}"))
                            .test_support()
                            .flex()
                            .items_baseline()
                            .text_size(px(14.))
                            .line_height(relative(1.55))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} ", self.t("monitor.management.content")))
                            .child(
                                div()
                                    .id(format!("monitor-content-optional-{key}"))
                                    .test_support()
                                    .text_size(px(12.))
                                    .line_height(relative(1.4))
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.t("monitor.management.optional")),
                            ),
                    )
                    .when_some(
                        editor.inputs.as_ref().and_then(|inputs| inputs.get(index)),
                        |field, input| {
                            field.child(
                                div()
                                    .id(format!("monitor-content-input-{key}"))
                                    .test_support()
                                    .w_full()
                                    .child(Textarea::new(input).w_full().h(px(68.)).aria_label(
                                        format!(
                                            "{} {}",
                                            self.t(&format!("monitor.behavior.{key}")),
                                            self.t("monitor.management.content")
                                        ),
                                    )),
                            )
                        },
                    ),
            );
            card = card
                .child(content)
                .child(super::behavior_surface::top_accent(color.into()));
            behaviors = behaviors.child(card);
        }
        page = page.child(behaviors);
        page = page.pb(px(80.));
        page.into_any_element()
    }

    /// 保存操作保持在窗口底部，滚动四行为或内嵌图库时仍可触达。
    pub(in crate::app) fn monitor_profile_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.page != crate::app::Page::Monitor
            || self.monitor.route != super::state::Route::Management
        {
            return None;
        }
        let state = &self.monitor.catalog;
        if state.profiles_error.is_some()
            || state.images_error.is_some()
            || self.agent_settings.error.is_some()
        {
            return None;
        }
        let tool = self.monitor_tool()?;
        let editor = state
            .editors
            .iter()
            .find(|editor| editor.draft.tool == tool)?;
        let draft = &editor.draft;
        let configured = draft
            .hooks
            .iter()
            .filter(|hook| {
                state
                    .images
                    .iter()
                    .any(|image| image.id == hook.image && image.source.is_ok())
            })
            .count();
        Some(
            div()
                .id("monitor-profile-save-bar")
                .test_support()
                .absolute()
                .left(px(if self.viewport_width < 992. {
                    100.
                } else {
                    112.
                }))
                .right(px(if self.viewport_width < 992. { 20. } else { 32. }))
                .bottom(px(12.))
                .p_3()
                .rounded(px(16.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.t("monitor.management.displayConfig")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(if configured == draft.hooks.len() {
                                    self.t("monitor.management.ready")
                                } else {
                                    self.t("monitor.management.remaining").replace(
                                        "%{count}",
                                        &(draft.hooks.len() - configured).to_string(),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(state.saved_tool == Some(tool), |row| {
                            row.child(
                                div()
                                    .id("monitor-profile-saved")
                                    .role(Role::Status)
                                    .aria_label(self.t("monitor.management.saved"))
                                    .text_color(rgb(0x20c997))
                                    .child(self.t("monitor.management.saved")),
                            )
                        })
                        .child(
                            SurfaceButton::new(
                                "monitor-profile-save",
                                SurfaceButtonVariant::FilledBrand,
                                cx.theme().mode.is_dark(),
                            )
                                .historical_size(HistoricalButtonSize::Small)
                                .left_icon(
                                    gpui_kit::component::Icon::default().data(include_bytes!(
                                        "../../../assets/icons/phosphor-check.svg"
                                    )),
                                )
                                .label(self.t("monitor.management.save"))
                                .loading(state.saving)
                                .disabled(!state.images_loaded)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_monitor_profile(cx)),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// 稳定行为键与 core 四行为一致。
pub(super) fn behavior_key(behavior: HookBehavior) -> &'static str {
    match behavior {
        HookBehavior::Idle => "idle",
        HookBehavior::Running => "running",
        HookBehavior::Asking => "asking",
        _ => "error",
    }
}
