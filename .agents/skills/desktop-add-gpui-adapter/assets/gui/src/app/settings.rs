//! 设置页展示固定偏好与明确选择的桌面能力，事件归属实际控件。

use super::Shell;
use crate::preferences::{LanguageChoice, ThemeChoice};
use gpui_kit::{AnyElement, Context, Div, FontWeight, IntoElement, component::{ActiveTheme, button::{Button, ButtonVariants}}, div, prelude::*, px};

impl Shell {
    /// 明确按钮与即时反馈形成原生设置基线，所有文案使用当前词典。
    pub(super) fn settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        div().w_full().flex().flex_col().gap_6()
            .child(self.card("settings.application", "settings.local_preferences", cx)
                .child(div().w_full().min_w_0().whitespace_normal().text_lg().font_weight(FontWeight::SEMIBOLD).child(format!("{} {}", self.name(), self.version()))))
            .child(self.card("settings.appearance", "settings.appearance_detail", cx)
                .child(div().flex().gap_3().flex_wrap().children([
                    (ThemeChoice::Light, "settings.light"),
                    (ThemeChoice::Dark, "settings.dark"),
                    (ThemeChoice::System, "settings.system"),
                ].into_iter().enumerate().map(|(index, (theme, key))| {
                    let label = self.t(key);
                    Button::new(("theme-choice", index)).outline().label(label.clone())
                        .accessibility_label(label).min_w(px(124.)).h(px(40.))
                        .when(self.preferences.theme == theme, |button| button.primary())
                        .on_click(cx.listener(move |this, _, window, cx| this.select_theme(theme, window, cx)))
                }))))
            .child(self.card("settings.language", "settings.language_detail", cx)
                .child(div().flex().gap_3().flex_wrap().children([
                    (LanguageChoice::Chinese, "settings.chinese"),
                    (LanguageChoice::English, "settings.english"),
                    (LanguageChoice::System, "settings.system"),
                ].into_iter().enumerate().map(|(index, (language, key))| {
                    let label = self.t(key);
                    Button::new(("language-choice", index)).outline().label(label.clone())
                        .accessibility_label(label).min_w(px(124.)).h(px(40.))
                        .when(self.preferences.language == language, |button| button.primary())
                        .on_click(cx.listener(move |this, _, _, cx| this.select_language(language, cx)))
                }))))
@@NOTIFICATION_SETTING@@
@@AUTOSTART_SETTING@@
            .into_any_element()
    }

    /// 表面、边框和辅助文字都使用组件库完整亮暗语义色。
    pub(super) fn card(&self, title: &str, detail: &str, cx: &Context<Self>) -> Div {
        div().w_full().flex().flex_col().gap_4().p_6().rounded_lg()
            .border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(div().w_full().min_w_0().whitespace_normal().text_lg().font_weight(FontWeight::SEMIBOLD).child(self.t(title)))
            .child(div().w_full().min_w_0().whitespace_normal().text_sm().text_color(cx.theme().muted_foreground).child(self.t(detail)))
    }
}
