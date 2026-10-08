//! 全局设置按产品卡片布局展示，只有实际宿主能力拥有可操作控件。

mod agents;
mod preferences;
mod segments;
pub(in crate::app) use agents::AgentSettingsState;
pub(in crate::app) use preferences::PreferenceSaveState;
use super::Shell;
use crate::preferences::{LanguageChoice,ThemeChoice,PreferencePatch};
use gpui_kit::{AnyElement,Context,Div,FontWeight,IntoElement,Role,
    base::TestSupportExt as _,
    component::{ActiveTheme,Disableable,button::{Button,ButtonVariants}},div,prelude::*,px};

impl Shell {
    /// 应用信息、唯一 Agent 入口和两列偏好卡沿用参考页面顺序与间距。
    pub(super) fn settings_page(&self,cx:&mut Context<Self>)->AnyElement {
        div().w_full().flex().flex_col().gap_5()
            .child(self.settings_application(cx))
            .when(self.show_release_notes,|page|page.child(self.release_notes_panel(cx)))
            .child(self.agent_settings_panel(cx))
            .child(div().w_full().grid().grid_cols(if self.viewport_width>=1200.{2}else{1}).gap_5()
                .child(self.settings_language(cx)).child(self.settings_theme(cx)))
            .child(self.settings_autostart(cx))
            .into_any_element()
    }

    /// 名称与版本直接来自当前包，更新日志沿用当前构建的真实记录。
    fn settings_application(&self,cx:&mut Context<Self>)->AnyElement {
        self.settings_surface(32.,cx).gap_4()
            .child(div().w_full().flex().flex_wrap().items_start().justify_between().gap_4()
                .child(div().id("settings-application-copy").test_support().flex_1().min_w(px(280.)).max_w(px(620.)).flex().flex_col().gap_4()
                    .child(self.settings_title("settings.application",cx))
                    .child(div().flex().flex_col().gap(px(2.))
                        .child(div().text_size(px(18.)).font_weight(FontWeight(650.)).child("LokiMetris"))
                        .child(div().text_sm().text_color(cx.theme().muted_foreground).child(format!("{}: 诡秘神谕",self.t("settings.localized_name")))))
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("settings.application_description"))))
                .child(div().flex().flex_col().items_end().gap_4()
                    .child(div().h(px(26.)).px(px(12.)).rounded_full().flex().items_center().border_1().border_color(cx.theme().primary.opacity(0.26)).bg(cx.theme().primary.opacity(0.09))
                        .text_color(if cx.theme().mode.is_dark(){cx.theme().foreground}else{cx.theme().primary}).text_size(px(14.)).font_weight(FontWeight::BOLD)
                        .child(format!("{} {}",self.t("settings.version"),self.version())))
                    .child(Button::new("settings-release-notes").h(px(36.)).rounded(px(12.)).outline().bg(cx.theme().primary.opacity(0.09)).border_color(cx.theme().primary.opacity(0.26)).label(self.t("settings.release_notes_action"))
                        .on_click(cx.listener(|this,_,_,cx|{this.show_release_notes=!this.show_release_notes;cx.notify();})))))
            .into_any_element()
    }

    /// 语言仅提供中英两项；系统探测只作为初始偏好解析。
    fn settings_language(&self,cx:&mut Context<Self>)->AnyElement {
        self.settings_surface(32.,cx).gap_4().child(self.settings_title("settings.language",cx))
            .child(self.preference_segments("language-choice","settings.language",[(PreferencePatch::Language(LanguageChoice::Chinese),"settings.chinese"),(PreferencePatch::Language(LanguageChoice::English),"settings.english")],usize::from(self.locale!="zh-CN"),&self.preference_save.language_focus,cx))
            .when(matches!(self.preference_save.failed,Some(PreferencePatch::Language(_))),|card|card.child(self.preference_failure("settings.language_error",cx)))
            .into_any_element()
    }

    /// 三态主题按已确认值显示选中，保存中与失败时均保留原主题。
    fn settings_theme(&self,cx:&mut Context<Self>)->AnyElement {
        self.settings_surface(32.,cx).gap_4().child(self.settings_title("settings.theme_title",cx))
            .child(self.preference_segments("theme-choice","settings.theme_title",[(PreferencePatch::Theme(ThemeChoice::Light),"settings.light"),(PreferencePatch::Theme(ThemeChoice::Dark),"settings.dark"),(PreferencePatch::Theme(ThemeChoice::System),"settings.system")],match self.preferences.theme{ThemeChoice::Light=>0,ThemeChoice::Dark=>1,ThemeChoice::System=>2},&self.preference_save.theme_focus,cx))
            .when(matches!(self.preference_save.failed,Some(PreferencePatch::Theme(_))),|card|card.child(self.preference_failure("settings.theme_error",cx)))
            .into_any_element()
    }

    /// 自启状态以 OS 重读为权威；仅失败或未知时显示恢复动作。
    fn settings_autostart(&self,cx:&mut Context<Self>)->AnyElement {
        let state=crate::native::autostart_state(cx);
        self.settings_surface(20.,cx).gap_3()
            .child(div().flex().items_center().justify_between().gap_3()
                .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(self.t("settings.autostart")))
                .child(gpui_kit::component::switch::Switch::new("autostart-switch").accessibility_label(self.t("settings.autostart"))
                    .checked(state.enabled.unwrap_or(false)).map(|switch|gpui_kit::base::Disableable::disabled(switch,state.busy||state.enabled.is_none()))
                    .on_click(cx.listener(|_,enabled,_,cx|{crate::native::set_autostart(*enabled,cx);cx.notify();}))))
            .when(state.error,|card|card.child(self.preference_failure("settings.autostart_error",cx)))
            .when(state.enabled.is_none(),|card|card.child(self.t("settings.autostart_unknown")))
            .when(state.error||state.enabled.is_none(),|card|card.child(Button::new("autostart-reload").outline().label(self.t("settings.reload"))
                .disabled(state.busy).on_click(cx.listener(|_,_,_,cx|{crate::native::reload_autostart(cx);cx.notify();}))))
            .into_any_element()
    }

    /// 参考卡片共用圆角和表面，内边距按卡片类型传入。
    fn settings_surface(&self,padding:f32,cx:&Context<Self>)->Div {
        div().w_full().min_w_0().flex().flex_col().p(px(padding)).rounded(px(20.)).border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
    }

    /// 公共标题保持19.2px字号和650字重。
    fn settings_title(&self,key:&str,_cx:&Context<Self>)->AnyElement {
        div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight(650.)).child(self.t(key)).into_any_element()
    }

    /// 保存失败明确告知原值仍生效，下一次点击可再次提交。
    fn preference_failure(&self,key:&str,cx:&Context<Self>)->AnyElement {
        div().id(gpui_kit::SharedString::from(key.to_owned())).role(Role::Alert).aria_label(self.t(key)).w_full().rounded(px(12.)).p_4()
            .bg(cx.theme().danger.opacity(0.1)).text_color(cx.theme().danger).child(self.t(key)).into_any_element()
    }

    /// 既有关于卡暂保留通用表面，完成应用信息迁移后再统一入口。
    pub(super) fn card(&self,title:&str,detail:&str,cx:&Context<Self>)->Div {
        self.settings_surface(24.,cx).gap_4().child(self.settings_title(title,cx))
            .child(div().w_full().min_w_0().whitespace_normal().text_sm().text_color(cx.theme().muted_foreground).child(self.t(detail)))
    }
}
