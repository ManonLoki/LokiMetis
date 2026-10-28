//! 看板各页共用的指标展示：小摘要卡、基点比率与 core 标签码翻译。

use super::{Shell, format};
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px,
    component::{ActiveTheme, button::{Button, ButtonVariants}, Icon}};
use loki_metris_core::DisplayLabelCode;

impl Shell {
    /// 小摘要沿用轻量材质；只有总 Token 提供复制精确值。
    pub(super) fn mini_metric(&self,key:&'static str,value:String,exact:Option<u64>,cx:&mut Context<Self>) -> AnyElement {
        let dark=cx.theme().mode.is_dark();
        crate::app::surface::inset_card(SharedString::from(format!("mini-card-{key}")), dark).p_5()
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.t(key)))
            .child(div().id(SharedString::from(format!("mini-{key}"))).role(gpui_kit::Role::Label).aria_label(self.t(key)).aria_value(value.clone())
                .text_size(px(32.)).font_weight(FontWeight::EXTRA_BOLD).child(value))
            .when_some(exact,|card,value|card.child(div().flex().items_center().gap_1()
                .child(div().text_xs().font_weight(FontWeight::NORMAL).text_color(cx.theme().muted_foreground).font_family("Geist Mono").child(format!("{}: {} Token",self.t("dashboard.exact"),format::exact(value))))
                .child(Button::new("statistics-copy").ghost().size(px(22.)).icon(Icon::new(if self.dashboard.copied_metric==Some(key) {gpui_kit::assets::IconName::Check}else{gpui_kit::assets::IconName::Copy}).size(px(14.)))
                    .accessibility_label(self.t(if self.dashboard.copied_metric==Some(key) {"dashboard.copied"}else{"dashboard.copy"}))
                    .on_click(cx.listener(move |this,_,_,cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(format::exact(value)));
                        this.dashboard.copied_metric=Some(key);
                        this.dashboard.copy_task=Some(cx.spawn(async |entity,cx| {
                            cx.background_executor().timer(super::COPY_FEEDBACK).await;
                            let _=entity.update(cx,|this,cx| {this.dashboard.copied_metric=None;cx.notify();});
                        }));cx.notify();
                    })))))
            .into_any_element()
    }

    /// 基点比率不依赖浮点聚合，空比例明确不适用。
    pub(super) fn basis_points(&self,value:Option<u16>) -> String {
        value.map(|v|{let tenth=(u32::from(v)+5)/10;if tenth%10==0 {format!("{}%",tenth/10)}else{format!("{}.{:01}%",tenth/10,tenth%10)}}).unwrap_or_else(||self.t("workbuddy.not_applicable"))
    }

    /// 标签码只负责翻译；Literal/安全项目/线程标签保留 core 已验证文本。
    pub(super) fn display_label(&self,label:&str,code:DisplayLabelCode) -> String {
        let key=match code {
            DisplayLabelCode::UnknownModel=>"unknown_model",DisplayLabelCode::UnknownReasoningEffort=>"unknown_reasoning",
            DisplayLabelCode::ReasoningNone=>"reasoning_none",DisplayLabelCode::ReasoningMinimal=>"reasoning_minimal",
            DisplayLabelCode::ReasoningLow=>"reasoning_low",DisplayLabelCode::ReasoningMedium=>"reasoning_medium",
            DisplayLabelCode::ReasoningHigh=>"reasoning_high",DisplayLabelCode::ReasoningXHigh=>"reasoning_xhigh",
            DisplayLabelCode::UncategorizedProject=>"uncategorized",DisplayLabelCode::UnknownThread=>"unknown_thread",
            DisplayLabelCode::Project=>return format!("{} {label}",self.t("statistics.project")),
            DisplayLabelCode::Thread=>return format!("{} {label}",self.t("statistics.thread")),
            DisplayLabelCode::UnnamedRoot=>"unnamed_root",DisplayLabelCode::Remainder=>"remainder",_=>return label.to_owned(),
        };self.t(&format!("statistics.{key}"))
    }
}
