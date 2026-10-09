//! 目录弹窗持有稳定草稿和焦点，关闭草稿不取消已经提交的后台写入。

use super::{Shell,SourceButton,SourceButtonVariant};
use crate::usage_service::SourceOperation;
use gpui_kit::{AnyElement,Context,Entity,FocusHandle,Focusable,Subscription,Window,div,prelude::*,px,FontWeight,
    base::{Dialog,DialogPopup,DialogBackdrop},component::{ActiveTheme,Disableable,input::{Input,InputState,InputEvent},button::{Button,ButtonVariants}}};
use loki_metris_core::SourceRootSummary;

/// 弹窗只持有别名草稿和目标身份，不把原始目录暴露给输入控件。
pub(in crate::app) struct SourceDialog {
    root:SourceRootSummary,
    remove:bool,
    input:Entity<InputState>,
    focus:FocusHandle,
    previous_focus:Option<FocusHandle>,
    _subscription:Subscription,
}

impl Shell {
    /// 输入实体只在打开时创建一次，保留中文输入法与焦点状态。
    pub(super) fn open_source_dialog(&mut self,root:SourceRootSummary,remove:bool,window:&mut Window,cx:&mut Context<Self>) {
        let previous_focus=window.focused(cx);let focus=cx.focus_handle();
        let label=self.t("sources.dialog.renameLabel");let value=root.alias.clone();
        let input=cx.new(|cx| {let mut input=InputState::new(window,cx).placeholder(label);input.set_value(value,window,cx);input});
        let subscription=cx.subscribe(&input,|_,_,event,cx|{if matches!(event,InputEvent::Change) {cx.notify();}});
        if remove {window.focus(&focus,cx);}else{window.focus(&input.focus_handle(cx),cx);}
        self.dashboard.sources.modal=Some(SourceDialog{root,remove,input,focus,previous_focus,_subscription:subscription});cx.notify();
    }

    /// 取消、遮罩和 Escape 统一恢复最初触发控件焦点。
    fn close_source_dialog(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        if let Some(dialog)=self.dashboard.sources.modal.take() {if let Some(focus)=dialog.previous_focus {window.focus(&focus,cx);}}
        cx.notify();
    }

    /// 原版确认立即关闭弹窗；写锁竞争失败由页面反馈承接。
    fn confirm_source_dialog(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        let Some(dialog)=&self.dashboard.sources.modal else{return};
        if self.dashboard.sources.refresh_failed {return}
        let alias=dialog.input.read(cx).value().trim().to_owned();
        if !dialog.remove && (alias.is_empty()||alias==dialog.root.alias) {return}
        let operation=if dialog.remove {SourceOperation::Remove{root_id:dialog.root.id.clone()}}
            else{SourceOperation::Rename{root_id:dialog.root.id.clone(),alias}};
        self.close_source_dialog(window,cx);self.change_source(operation,cx);
    }

    /// 无动画的居中模态使用 occlude popup 防止内部点击落到背景关闭处理。
    pub(in crate::app) fn source_dialog(&self,cx:&mut Context<Self>)->Option<AnyElement> {
        let modal=self.dashboard.sources.modal.as_ref()?;
        let title=self.t(if modal.remove {"sources.dialog.removeTitle"}else{"sources.dialog.renameTitle"});
        let label=self.t(if modal.remove {"sources.dialog.removeConfirm"}else{"sources.dialog.save"});
        let value=modal.input.read(cx).value();
        let disabled=self.dashboard.sources.refresh_failed||(!modal.remove&&(value.trim().is_empty()||value.trim()==modal.root.alias));
        let mut popup=DialogPopup::new().w(px(440.)).max_w_full().p_4().rounded(px(16.)).border_1().border_color(cx.theme().border)
            .bg(cx.theme().secondary).flex().flex_col().gap_4()
            .child(div().flex().items_center().justify_between().child(div().font_weight(FontWeight::SEMIBOLD).text_size(px(16.)).child(title))
                .child(Button::new("sources-dialog-close").ghost().label("×").accessibility_label(self.t("sources.dialog.cancel"))
                    .on_click(cx.listener(|this,_,window,cx|this.close_source_dialog(window,cx)))));
        if modal.remove {popup=popup.child(self.t("sources.dialog.removeBody").replace("{{alias}}",&modal.root.alias).replace("{{client}}",self.sources_client_name()));}
        else {popup=popup.child(div().flex().flex_col().gap_1().child(self.t("sources.dialog.renameLabel"))
            .child(Input::new(&modal.input).disabled(self.dashboard.sources.refresh_failed)));}
        popup=popup.child(div().flex().justify_end().gap_2()
            .child(SourceButton::new("sources-dialog-cancel",SourceButtonVariant::SubtleBrand,cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).label(self.t("sources.dialog.cancel"))
                .on_click(cx.listener(|this,_,window,cx|this.close_source_dialog(window,cx))))
            .child(SourceButton::new("sources-dialog-confirm",if modal.remove{SourceButtonVariant::FilledRed}else{SourceButtonVariant::FilledBrand},cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).label(label).disabled(disabled)
                .on_click(cx.listener(|this,_,window,cx|this.confirm_source_dialog(window,cx)))));
        let weak=cx.entity().downgrade();let confirm=weak.clone();
        Some(Dialog::new(cx).focus_handle(modal.focus.clone()).close_on_escape(true).close_on_backdrop_press(true)
            .backdrop(DialogBackdrop::new().absolute().inset_0().bg(gpui_kit::black().opacity(0.55))).popup(popup)
            .on_open_change(move|open,_,window,cx| {if !open {let _=weak.update(cx,|this,cx|this.close_source_dialog(window,cx));}})
            .on_ok(move|_,window,cx| {let _=confirm.update(cx,|this,cx|this.confirm_source_dialog(window,cx));false})
            .into_any_element())
    }
}
