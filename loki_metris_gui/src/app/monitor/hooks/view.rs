//! Hooks卡片对应参考输入尺寸、只读路径与逐工具操作状态。

use super::{Shell, state::Feedback};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, IntoElement, Role,
    base::TestSupportExt as _,
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    div,
    prelude::*,
    px, rgb,
};
use loki_metris_core::HookWriteOutcome;

impl Shell {
    /// 配置路径全部来自宿主；界面从不以存在marker推断安装成功。
    pub(in crate::app::monitor) fn monitor_hooks(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.monitor.hooks;
        let mut card = div()
            .id("monitor-hooks")
            .test_support()
            .w_full()
            .flex_shrink_0()
            .p(px(12.))
            .rounded(px(20.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_color(if cx.theme().mode.is_dark() {
                rgb(0xc1c2c5).into()
            } else {
                cx.theme().foreground
            })
            .child(
                div()
                    .id("monitor-hooks-title")
                    .role(Role::Heading)
                    .aria_label(self.t("monitor.settings.title"))
                    .text_size(px(19.2))
                    .line_height(gpui_kit::relative(1.3))
                    .font_weight(FontWeight(650.))
                    .child(self.t("monitor.settings.title")),
            );
        if let Some(code) = self.agent_settings.error {
            return card
                .child(self.hook_feedback_element(&self.t(code), true, cx))
                .into_any_element();
        }
        if let Some(error) = &state.error {
            return card
                .child(self.hook_feedback_element(&self.monitor_error_text(error), true, cx))
                .child(
                    Button::new("monitor-hooks-retry")
                        .label(self.t("ui.retry"))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.load_monitor_hook_locations(cx)),
                        ),
                )
                .into_any_element();
        }
        if state.loading && !state.loaded || self.agent_settings.loading {
            return card
                .child(self.t("monitor.picker.loading"))
                .into_any_element();
        }
        let Some(tool) = self.monitor_hook_tool() else {
            return card
                .child(self.hook_feedback_element(
                    &self.t("monitor.settings.chooseAgentFirst"),
                    false,
                    cx,
                ))
                .into_any_element();
        };
        let mut tabs = div()
            .id("monitor-hook-agents")
            .test_support()
            .role(Role::TabList)
            .flex()
            .items_center()
            .gap(px(4.))
            .overflow_x_scroll();
        for candidate in self.monitor_hook_tools() {
            tabs = tabs.child(
                gpui_kit::base::Button::new(format!(
                    "monitor-hook-agent-{}",
                    loki_metris_core::ai_tool_name(candidate)
                ))
                .role(Role::Tab)
                .aria_selected(candidate == tool)
                .selected(candidate == tool)
                .flex_shrink_0()
                .px(px(12.))
                .h(px(31.68))
                .rounded(px(10.4))
                .text_size(px(14.08))
                .line_height(gpui_kit::relative(1.))
                .font_weight(FontWeight::BOLD)
                .accessibility_label(loki_metris_core::ai_tool_name(candidate))
                .child(loki_metris_core::ai_tool_name(candidate))
                .when(candidate == tool, |button| {
                    button
                        .bg(cx.theme().accent)
                        .text_color(cx.theme().accent_foreground)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.monitor.hooks.selected = Some(candidate);
                    this.monitor.hooks.clear_feedback();
                    cx.notify();
                })),
            );
        }
        card = card.child(tabs);
        let Some(editor) = state
            .editors
            .iter()
            .find(|editor| editor.saved.tool == tool)
        else {
            return card.into_any_element();
        };
        let Some(inputs) = &editor.inputs else {
            return card.into_any_element();
        };
        let mut panel = div()
            .id("monitor-hooks-panel")
            .test_support()
            .pt(px(10.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(self.hook_input(
                "monitor-hook-directory",
                "monitor.settings.directory",
                &inputs[0],
                false,
                cx,
            ))
            .child(self.hook_input(
                "monitor-hook-file",
                "monitor.settings.configFile",
                &inputs[1],
                true,
                cx,
            ));
        if inputs.len() > 2 {
            let mut extra = div().flex().flex_col().gap_1().child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(17.5))
                    .child(self.t("monitor.settings.additionalConfigFiles")),
            );
            for (index, input) in inputs.iter().skip(2).enumerate() {
                extra = extra.child(self.hook_input_control(
                    format!("monitor-hook-extra-{index}"),
                    &self.t("monitor.settings.additionalConfigFiles"),
                    input,
                    true,
                    cx,
                ));
            }
            panel = panel.child(extra);
        }
        let dirty = editor.dirty();
        let left = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.))
            .child(
                Button::new("monitor-hook-choose")
                    .outline()
                    .h(px(30.))
                    .px(px(14.))
                    .rounded(px(12.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .label(self.t("monitor.settings.chooseDirectory"))
                    .loading(editor.picking)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose_monitor_hook_directory(tool, cx)
                    })),
            )
            .child(
                Button::new("monitor-hook-save")
                    .h(px(30.))
                    .px(px(14.))
                    .rounded(px(12.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .label(self.t("monitor.settings.saveDirectory"))
                    .loading(editor.saving)
                    .disabled(!dirty || editor.draft.trim().is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.save_monitor_hook_directory(tool, false, cx)
                    })),
            )
            .child(
                Button::new("monitor-hook-reset")
                    .h(px(30.))
                    .px(px(14.))
                    .rounded(px(12.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .label(self.t("monitor.settings.restoreDirectory"))
                    .loading(editor.saving)
                    .disabled(!editor.saved.is_custom)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.save_monitor_hook_directory(tool, true, cx)
                    })),
            );
        panel = panel.child(
            div()
                .id("monitor-hook-actions")
                .test_support()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(left)
                .child(
                    Button::new("monitor-hook-write")
                        .primary()
                        .h(px(30.))
                        .px(px(14.))
                        .rounded(px(12.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .label(self.t("monitor.settings.write"))
                        .loading(editor.writing)
                        .disabled(dirty || state.loading)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.write_monitor_hooks(tool, cx)),
                        ),
                ),
        );
        if let Some((feedback_tool, feedback)) = &state.feedback {
            if *feedback_tool == tool {
                let (label, error) = match feedback {
                    Feedback::Failed(error) => (self.monitor_error_text(error), true),
                    Feedback::Written(result) => {
                        let key = match result.outcome {
                            HookWriteOutcome::Unchanged => "unchanged",
                            HookWriteOutcome::Active => "active",
                            HookWriteOutcome::RestartRequired => "restartRequired",
                            HookWriteOutcome::CodexReviewRequired => "codexReviewRequired",
                            HookWriteOutcome::WorkBuddyReviewRequired => "workBuddyReviewRequired",
                            HookWriteOutcome::CodeBuddyReviewRequired => "codeBuddyReviewRequired",
                            HookWriteOutcome::HermesEnableRequired => "hermesEnableRequired",
                            HookWriteOutcome::OpenClawEnableRequired => "openClawEnableRequired",
                        };
                        (
                            self.t("monitor.settings.written")
                                .replace("%{file}", &result.filename)
                                .replace("%{outcome}", &self.t(&format!("monitor.outcome.{key}"))),
                            false,
                        )
                    }
                };
                panel = panel.child(self.hook_feedback_element(&label, error, cx));
            }
        }
        card.child(panel).into_any_element()
    }

    /// 输入标签与控件保持独立，只读配置仍可聚焦、全选和复制。
    fn hook_input(
        &self,
        id: &str,
        key: &str,
        input: &Entity<InputState>,
        readonly: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let label = self.t(key);
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(px(17.5))
                    .child(label.clone()),
            )
            .child(self.hook_input_control(id.to_owned(), &label, input, readonly, cx))
            .into_any_element()
    }
    /// 固定30px输入高度与原版小号路径字段一致，不使用禁用态代替只读。
    fn hook_input_control(
        &self,
        id: String,
        label: &str,
        input: &Entity<InputState>,
        readonly: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let dark = cx.theme().mode.is_dark();
        div()
            .id(id)
            .test_support()
            .w_full()
            .h(px(30.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(if dark { 0x424242 } else { 0xced4da }))
            .bg(rgb(if dark { 0x2e2e2e } else { 0xffffff }))
            .child(
                Input::new(input)
                    .aria_label(label.to_owned())
                    .readonly(readonly)
                    .appearance(false)
                    .h_full()
                    .px(px(10.))
                    .text_size(px(12.)),
            )
            .into_any_element()
    }
    /// 成功为蓝色提示，错误为红色；没有启用项使用同一信息表面。
    fn hook_feedback_element(&self, label: &str, error: bool, cx: &Context<Self>) -> AnyElement {
        let color = rgb(if error { 0xfa5252 } else { 0x228be6 });
        div()
            .id("monitor-hook-feedback")
            .test_support()
            .role(if error { Role::Alert } else { Role::Status })
            .aria_label(label.to_owned())
            .w_full()
            .p_4()
            .rounded(px(12.))
            .bg(color.opacity(0.1))
            .text_size(px(14.))
            .text_color(if cx.theme().mode.is_dark() {
                cx.theme().foreground
            } else {
                color.into()
            })
            .child(label.to_owned())
            .into_any_element()
    }
}
