//! 全局设置按产品卡片布局展示，只有实际宿主能力拥有可操作控件。

#[cfg(test)]
mod a11y_tests;
mod agents;
mod preferences;
mod release_notes;
mod release_notes_content;
mod repository;
mod segments;
mod support;
mod switches;
use super::Shell;
use crate::preferences::{LanguageChoice, PreferencePatch, ThemeChoice};
pub(in crate::app) use agents::AgentSettingsState;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, IntoElement, Role, SharedString, Text,
    base::TestSupportExt as _,
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
    },
    div,
    prelude::*,
    px,
};
pub(in crate::app) use preferences::PreferenceSaveState;
pub(in crate::app) use release_notes::ReleaseNotesModal;
pub(in crate::app) use repository::RepositoryState;

impl Shell {
    /// 应用信息、唯一 Agent 入口和两列偏好卡沿用参考页面顺序与间距。
    pub(super) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap_5()
            .line_height(gpui_kit::relative(1.55))
            .child(self.settings_application(cx))
            .child(self.agent_settings_panel(cx))
            .child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(if self.viewport_width >= 1200. { 2 } else { 1 })
                    .gap_5()
                    .child(self.settings_language(cx))
                    .child(self.settings_theme(cx)),
            )
            .child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(if self.viewport_width >= 1200. { 2 } else { 1 })
                    .gap_5()
                    .child(self.settings_notifications(cx))
                    .child(self.settings_autostart(cx)),
            )
            .child(self.settings_sponsor(cx))
            .into_any_element()
    }

    /// 名称与版本直接来自当前包，更新日志沿用当前构建的真实记录。
    fn settings_application(&self, cx: &mut Context<Self>) -> AnyElement {
        self.settings_surface("settings-application",32.,cx).gap_4()
            .child(div().w_full().flex().flex_wrap().items_start().justify_between().gap_4()
                .child(div().id("settings-application-copy").test_support().flex_1().min_w(px(280.)).max_w(px(620.)).flex().flex_col().gap_4()
                    .child(self.settings_title("settings.application",cx))
                    .child(div().flex().flex_col().gap(px(2.))
                        .child(div().text_size(px(18.)).font_weight(FontWeight(650.)).child(Text::new("settings-application-name".into(),"LokiMetris".into())))
                        .child(div().text_sm().text_color(cx.theme().muted_foreground).child(Text::new("settings-localized-name".into(),format!("{}: 诡秘神谕",self.t("settings.localized_name")).into()))))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(Text::new("settings-application-description".into(),self.t("settings.application_description").into()))))
                .child(div().flex().flex_col().items_end().gap_4()
                    .child(div().h(px(26.)).px(px(12.)).rounded_full().flex().items_center().border_1().border_color(cx.theme().primary.opacity(0.26)).bg(cx.theme().primary.opacity(0.09))
                        .text_color(if cx.theme().mode.is_dark(){cx.theme().foreground}else{cx.theme().primary}).text_size(px(13.)).font_weight(FontWeight::BOLD)
                        .child(Text::new("settings-application-version".into(),format!("{} {}",self.t("settings.version"),self.version()).into())))
                    .child(Button::new("settings-release-notes").h(px(36.)).rounded(px(12.)).outline().bg(cx.theme().primary.opacity(0.09)).border_color(cx.theme().primary.opacity(0.26)).accessibility_label(self.t("settings.release_notes_action"))
                        .child(div().flex().items_center().gap_3().font_weight(FontWeight::SEMIBOLD)
                            .child(gpui_kit::component::Icon::default().data(include_bytes!("../../../assets/icons/clock-counter-clockwise.svg")).size(px(18.)))
                            .child(self.t("settings.release_notes_action")))
                        .on_click(cx.listener(|this,_,window,cx|this.open_release_notes(window,cx))))
                    .child(Button::new("settings-github").ghost().h(px(36.)).accessibility_label(self.t("settings.github_repository_action"))
                        .child(div().flex().items_center().gap_3().font_weight(FontWeight::SEMIBOLD)
                            .child(gpui_kit::component::Icon::default().data(include_bytes!("../../../assets/icons/github-logo.svg")).size(px(18.)))
                            .child(self.t("settings.github_repository_action")))
                        .tooltip(self.t("settings.github_repository_description")).disabled(self.repository.pending)
                        .on_click(cx.listener(|this,_,_,cx|this.open_repository(cx))))))
            .when(self.repository.failed,|card|card.child(self.preference_failure("settings.github_repository_error",cx)))
            .into_any_element()
    }

    /// 语言仅提供中英两项；系统探测只作为初始偏好解析。
    fn settings_language(&self, cx: &mut Context<Self>) -> AnyElement {
        self.settings_surface("settings-language",32., cx)
            .gap_4()
            .child(self.settings_title("settings.language", cx))
            .child(self.preference_segments(
                "language-choice",
                "settings.language",
                [
                    (
                        PreferencePatch::Language(LanguageChoice::Chinese),
                        "settings.chinese",
                    ),
                    (
                        PreferencePatch::Language(LanguageChoice::English),
                        "settings.english",
                    ),
                ],
                usize::from(self.locale != "zh-CN"),
                &self.preference_save.language_focus,
                cx,
            ))
            .when(
                matches!(
                    self.preference_save.failed,
                    Some(PreferencePatch::Language(_))
                ),
                |card| card.child(self.preference_failure("settings.language_error", cx)),
            )
            .into_any_element()
    }

    /// 三态主题按已确认值显示选中，保存中与失败时均保留原主题。
    fn settings_theme(&self, cx: &mut Context<Self>) -> AnyElement {
        self.settings_surface("settings-theme",32., cx)
            .gap_4()
            .child(self.settings_title("settings.theme_title", cx))
            .child(self.preference_segments(
                "theme-choice",
                "settings.theme_title",
                [
                    (PreferencePatch::Theme(ThemeChoice::Light), "settings.light"),
                    (PreferencePatch::Theme(ThemeChoice::Dark), "settings.dark"),
                    (
                        PreferencePatch::Theme(ThemeChoice::System),
                        "settings.system",
                    ),
                ],
                match self.preferences.theme {
                    ThemeChoice::Light => 0,
                    ThemeChoice::Dark => 1,
                    ThemeChoice::System => 2,
                },
                &self.preference_save.theme_focus,
                cx,
            ))
            .when(
                matches!(self.preference_save.failed, Some(PreferencePatch::Theme(_))),
                |card| card.child(self.preference_failure("settings.theme_error", cx)),
            )
            .into_any_element()
    }

    /// 通知开关只有真实授权且偏好落盘成功才改变；重复点击在等待期间禁用。
    fn settings_notifications(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = crate::native::notification_state(cx);
        let changed =
            cx.listener(|_, enabled: &bool, _, cx| crate::native::set_notification(*enabled, cx));
        self.settings_plain_surface(20., cx)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(Text::new(
                                "settings-notification-label".into(),
                                self.t("settings.system_notification").into(),
                            )),
                    )
                    .child(
                        switches::settings_switch(
                            "notification-switch",
                            self.t("settings.system_notification"),
                            state.desired.unwrap_or(state.enabled),
                            state.busy,
                            cx,
                        )
                        .on_change(move |enabled, _, window, cx| changed(&enabled, window, cx)),
                    ),
            )
            .when(state.busy, |card| {
                card.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(Text::new(
                            "settings-notification-busy".into(),
                            self.t("settings.capability_pending").into(),
                        )),
                )
            })
            .when(state.error.is_some(), |card| {
                card.child(self.capability_feedback(
                    "settings.capability_error_title",
                    "settings.system_notification_error",
                    None,
                    cx,
                ))
            })
            .into_any_element()
    }

    /// 自启状态以 OS 重读为权威；仅失败或未知时显示恢复动作。
    fn settings_autostart(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = crate::native::autostart_state(cx);
        let changed = cx.listener(|_, enabled: &bool, _, cx| {
            crate::native::set_autostart(*enabled, cx);
            cx.notify();
        });
        self.settings_plain_surface(20., cx)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(Text::new(
                                "settings-autostart-label".into(),
                                self.t("settings.autostart").into(),
                            )),
                    )
                    .child(
                        switches::settings_switch(
                            "autostart-switch",
                            self.t("settings.autostart"),
                            state.desired.or(state.enabled).unwrap_or(false),
                            state.busy || state.enabled.is_none(),
                            cx,
                        )
                        .on_change(move |enabled, _, window, cx| changed(&enabled, window, cx)),
                    ),
            )
            .when(state.busy, |card| {
                card.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(Text::new(
                            "settings-autostart-busy".into(),
                            self.t(if state.enabled.is_none() {
                                "settings.capability_loading"
                            } else {
                                "settings.capability_pending"
                            })
                            .into(),
                        )),
                )
            })
            .when(state.error && state.enabled.is_some(), |card| {
                card.child(self.capability_feedback(
                    "settings.capability_error_title",
                    "settings.autostart_error",
                    None,
                    cx,
                ))
            })
            .when(state.enabled.is_none(), |card| {
                card.child(self.capability_feedback(
                    "settings.capability_unknown_title",
                    "settings.autostart_unknown",
                    Some(state.busy),
                    cx,
                ))
            })
            .into_any_element()
    }

    /// 品牌卡片共享原生聚光与阴影材质，稳定ID隔离各卡片的指针和过渡状态。
    fn settings_surface(&self, id: &'static str, padding: f32, cx: &Context<Self>) -> super::surface::SpotlightCard {
        super::surface::SpotlightCard::new(id, cx.theme().mode.is_dark()).p(px(padding))
    }

    /// 原生能力及媒体figure使用参考的中性Paper，保持独立于品牌聚光容器。
    fn settings_plain_surface(&self, padding: f32, cx: &Context<Self>) -> Div {
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .p(px(padding))
            .rounded(px(20.))
            .border_1()
            .border_color(gpui_kit::rgb(if cx.theme().mode.is_dark() {
                0x424242
            } else {
                0xdee2e6
            }))
            .bg(gpui_kit::rgb(if cx.theme().mode.is_dark() {
                0x242424
            } else {
                0xffffff
            }))
    }

    /// 公共标题保留参考字形，并作为二级标题供辅助技术按章节浏览。
    fn settings_title(&self, key: &str, _cx: &Context<Self>) -> AnyElement {
        let text = self.t(key);
        div()
            .id(SharedString::from(format!("settings-heading-{key}")))
            .role(Role::Heading)
            .aria_level(2)
            .aria_label(text.clone())
            .test_support()
            .text_size(px(19.2))
            .line_height(gpui_kit::relative(1.3))
            .font_weight(FontWeight(650.))
            .child(text)
            .into_any_element()
    }

    /// 保存失败明确告知原值仍生效，下一次点击可再次提交。
    fn preference_failure(&self, key: &str, cx: &Context<Self>) -> AnyElement {
        div()
            .id(gpui_kit::SharedString::from(key.to_owned()))
            .role(Role::Alert)
            .aria_label(self.t(key))
            .w_full()
            .rounded(px(12.))
            .p_4()
            .bg(cx.theme().danger.opacity(0.1))
            .text_color(cx.theme().danger)
            .child(self.t(key))
            .into_any_element()
    }

    /// 宿主能力错误沿用参考的灰色提示；未知自启可重读，等待期间不能重复派发。
    fn capability_feedback(
        &self,
        title: &str,
        message: &str,
        retry_disabled: Option<bool>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let gray = gpui_kit::rgb(0x868e96);
        let foreground = gpui_kit::rgb(if cx.theme().mode.is_dark() {
            0xf8f9fa
        } else {
            0x212529
        });
        div().id(gpui_kit::SharedString::from(message.to_owned())).role(Role::Alert).aria_label(self.t(title)).aria_description(self.t(message))
            .w_full().flex().items_start().rounded(px(20.)).p_4().border_1().border_color(gray.opacity(0.26)).bg(gray.opacity(0.09))
            .child(div().size(px(20.)).flex_shrink_0().mt(px(1.)).mr_4().flex().items_center().justify_center().text_color(foreground)
                .child(gpui_kit::component::Icon::default().data(include_bytes!("../../../assets/icons/warning-circle.svg")).size(px(18.))))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(10.)).text_sm()
                .child(div().font_weight(FontWeight::BOLD).text_color(foreground).child(self.t(title)))
                .child(div().flex().flex_col().items_start().gap_3().text_color(if cx.theme().mode.is_dark(){gpui_kit::white()}else{gpui_kit::black()})
                    .child(self.t(message))
                    .when_some(retry_disabled,|body,disabled|body.child(Button::new("autostart-reload").outline().h(px(30.)).px(px(10.)).rounded(px(12.))
                        .bg(gray.opacity(0.09)).border_color(gray.opacity(0.26)).text_color(foreground).accessibility_label(self.t("settings.capability_retry"))
                        .child(div().flex().items_center().gap(px(10.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD)
                            .child(gpui_kit::component::Icon::default().data(include_bytes!("../../../assets/icons/arrows-clockwise.svg")).size(px(16.)))
                            .child(self.t("settings.capability_retry")))
                        .disabled(disabled).on_click(cx.listener(|_,_,_,cx|{crate::native::reload_autostart(cx);cx.notify();}))))))
            .into_any_element()
    }
}
