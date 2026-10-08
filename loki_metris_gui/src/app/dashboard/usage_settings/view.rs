//! 设置卡的原生数字编辑保留非法草稿，用 core 校验提交并提供同卡 Enter 行为。

use super::*;
use gpui_kit::{AnyElement,FontWeight,IntoElement,TextAlign,div,prelude::*,px,rgb,base::{self,TestSupportExt as _},
    component::{ActiveTheme,Icon,IconName,input::Input,button::{Button,ButtonVariants},Disableable}};

impl Shell {
    /// 设置读取与扫描状态无关，整页读取失败只提供明确重试。
    pub(in crate::app) fn usage_settings_page(&self,cx:&mut Context<Self>)->AnyElement {
        let state=&self.dashboard.usage_settings;
        if state.failed {return div().w_full().p_4().rounded_lg().border_1().border_color(cx.theme().danger)
            .flex().items_center().justify_between().gap_3().child(self.t("dashboard.error"))
            .child(Button::new("usage-settings-retry").outline().label(self.t("dashboard.retry"))
                .on_click(cx.listener(|this,_,_,cx|this.load_usage_settings(cx)))).into_any_element()}
        if state.saved.is_none() {return div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("privacy.page.loading")).into_any_element()}
        div().w_full().flex().flex_col().gap_8().child(self.usage_setting_card(0,cx)).child(self.usage_setting_card(1,cx)).into_any_element()
    }

    /// 两张卡独立禁用保存按钮，后台索引运行时仍可编辑和保存设置。
    fn usage_setting_card(&self,index:usize,cx:&mut Context<Self>)->AnyElement {
        let state=&self.dashboard.usage_settings;let Some(inputs)=&state.inputs else{return div().into_any_element()};
        let input=inputs[index].clone();let draft=input.read(cx).value();let parsed=parsed(&draft,index);
        let changed=parsed.is_some_and(|draft|state.saved.is_some_and(|saved|value(saved,index)!=draft));
        let disabled=!changed||state.pending[index]||state.loading;
        let prefix=if index==0 {"privacy.scanInterval"}else{"privacy.retentionDays"};
        let label=self.t(&format!("{prefix}.label"));
        let dark=cx.theme().mode.is_dark();let blue=rgb(0x228be6);let blue_text=if dark {rgb(0xd0ebff)}else{rgb(0x1c7ed6)};let border=if parsed.is_none(){cx.theme().danger}else{if cx.theme().mode.is_dark(){rgb(0x424242).into()}else{rgb(0xced4da).into()}};
        let number=base::NumberInput::new(&input).controls_right().w_full().h(px(36.)).rounded(px(12.)).border_1().border_color(border)
            .bg(if cx.theme().mode.is_dark(){rgb(0x2e2e2e)}else{rgb(0xffffff)})
            .input(Input::new(&input).aria_label(label.clone()).appearance(false).h_full().px(px(11.)).text_align(TextAlign::Left).text_size(px(14.)).font_weight(FontWeight::NORMAL).text_color(if dark {rgb(0xc1c2c5).into()}else{cx.theme().foreground}))
            .increment_button({let label=self.t("privacy.increase");move|button|button.accessibility_label(label).w(px(22.)).border_l_1().border_color(border).flex().items_center().justify_center()
                .child(Icon::new(IconName::ChevronUp).size(px(12.)))})
            .decrement_button({let label=self.t("privacy.decrease");move|button|button.accessibility_label(label).w(px(22.)).border_l_1().border_color(border).flex().items_center().justify_center()
                .child(Icon::new(IconName::ChevronDown).size(px(12.)))})
            .on_step(move|action,window,cx|input.update(cx,|state,cx| {
                state.focus(window,cx);
                if let Some(next)=base::step_value(&state.value(),action,1.,Some(1.),Some(if index==0{1440.}else{3650.})) {state.replace_all(next,window,cx);}
            }));
        let mut card=self.panel(cx).child(div().flex().items_center().gap_2()
            .child(div().id(("usage-settings-title",index)).test_support().role(gpui_kit::Role::Heading).aria_label(self.t(&format!("{prefix}.title"))).text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight(650.)).child(self.t(&format!("{prefix}.title"))))
            .child(div().rounded_full().px(px(10.)).h(px(20.)).flex().items_center().border_1().border_color(blue.opacity(0.26)).bg(blue.opacity(0.09)).text_color(blue_text).text_size(px(11.)).font_weight(FontWeight::BOLD).child(self.t(&format!("{prefix}.badge")))))
            .child(div().flex().flex_col().gap(px(3.)).child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(label)).child(number)
                .when(parsed.is_none(),|column|column.child(div().id(("usage-settings-invalid",index)).test_support().role(gpui_kit::Role::Alert).aria_label(self.t(&format!("{prefix}.error"))).text_size(px(12.)).text_color(cx.theme().danger).child(self.t(&format!("{prefix}.error"))))))
            .child(div().flex().justify_end().child(Button::new(("usage-settings-save",index)).primary().h(px(36.)).px(px(18.)).rounded(px(12.))
                .label(self.t(&format!("{prefix}.save"))).loading(state.pending[index]).disabled(disabled)
                .when(disabled&&!state.pending[index],|button|button.bg(rgb(if dark {0x2e2e2e}else{0xf1f3f5})).text_color(rgb(if dark {0x696969}else{0xadb5bd})))
                .on_click(cx.listener(move|this,_,_,cx|this.save_usage_setting(index,cx)))));
        if let Some(feedback)=state.feedback[index] {
            let (color,title,body)=match feedback {
                Feedback::Saved(value)=>(rgb(0x12b886),self.t(&format!("{prefix}.successTitle")),self.t(&format!("{prefix}.successBody")).replace("{{minutes}}",&value.to_string()).replace("{{days}}",&value.to_string())),
                Feedback::Failed=>(rgb(0xfa5252),self.t(&format!("{prefix}.errorTitle")),self.t("common.unknownError")),
            };
            card=card.child(div().id(("usage-settings-feedback",index)).test_support().role(gpui_kit::Role::Alert).aria_label(format!("{title} {body}")).rounded(px(20.)).border_1().border_color(color.opacity(0.26)).p_4().bg(color.opacity(0.09)).text_color(if dark {cx.theme().foreground}else{color.into()})
                .child(div().font_weight(FontWeight::BOLD).text_size(px(14.)).child(title)).child(div().mt_1().text_size(px(14.)).child(body)));
        }card.into_any_element()
    }
}
