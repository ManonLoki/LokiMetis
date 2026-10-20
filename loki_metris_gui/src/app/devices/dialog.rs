//! 配对和移除模态拥有稳定输入与焦点，不把网络状态或凭据塞入控件。

use super::Shell;
use crate::app::surface::{SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, Role, Subscription, Window,
    base::{Dialog, DialogBackdrop, DialogPopup, TestSupportExt as _},
    component::{ActiveTheme, Disableable, input::{Input, InputEvent, InputState}}, div, prelude::*, px};

/// 临时草稿离开模态即丢弃；输入采用字符串保留四位码前导零。
pub(super) struct DeviceDialog {
    id: String,
    name: String,
    remove: bool,
    input: Entity<InputState>,
    focus: FocusHandle,
    previous: Option<FocusHandle>,
    _subscription: Subscription,
}

impl Shell {
    /// 仅按钮能打开模态；普通行/卡片无动作，输入实体只在打开时创建一次。
    pub(in crate::app) fn open_device_dialog(&mut self, id: String, name: String, remove: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.devices.snapshot.pending { return; }
        if !remove && let Some(error) = self.device_pairing_error(&id) {
            self.devices.error = Some(error); cx.notify(); return;
        }
        self.devices.error = None;
        let previous = window.focused(cx); let focus = cx.focus_handle();
        let placeholder = self.t("devices.codePlaceholder");
        let input = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(placeholder));
        let subscription = cx.subscribe(&input, |this, _, event, cx| { if matches!(event, InputEvent::Change) { this.devices.error = None; cx.notify(); } });
        if remove { window.focus(&focus, cx); } else { window.focus(&input.focus_handle(cx), cx); }
        self.devices.modal = Some(DeviceDialog { id, name, remove, input, focus, previous, _subscription: subscription });
        cx.notify();
    }

    /// Escape、取消和遮罩关闭恢复原焦点，并丢弃尚未提交的配对码。
    fn close_device_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.devices.modal.take() && let Some(focus) = dialog.previous { window.focus(&focus, cx); }
        cx.notify();
    }

    /// 只在四位码有效且服务接受意图后关闭模态；真实结果稍后由页面快照披露。
    fn confirm_device_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.devices.modal.as_ref() else { return; };
        if self.devices.snapshot.pending { return; }
        if !dialog.remove && let Some(error) = self.device_pairing_error(&dialog.id) {
            self.devices.error = Some(error); cx.notify(); return;
        }
        let code = dialog.input.read(cx).value().to_string();
        if !dialog.remove && !loki_metris_core::devices::valid_pairing_code(&code) { return; }
        let result = match &self.dashboard.service {
            Some(service) if dialog.remove => service.remove_device(dialog.id.clone()),
            Some(service) => service.pair_device(&dialog.id, code),
            None => Err("device_unavailable"),
        };
        match result {
            Ok(()) => { self.devices.error = None; self.close_device_dialog(window, cx); self.refresh_devices(cx); },
            Err(error) => { self.devices.error = Some(error); cx.notify(); },
        }
    }

    /// 模态使用现有原生焦点陷阱，按钮自己处理Enter，背景辅助树由壳层隔离。
    pub(in crate::app) fn device_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.devices.modal.as_ref()?;
        let title = self.t(if modal.remove { "devices.removeTitle" } else { "devices.pairTitle" });
        let description = self.t(if modal.remove { "devices.removeDescription" } else { "devices.pairDescription" }).replace("{{name}}", &modal.name);
        let dark = cx.theme().mode.is_dark();
        let target_error = if modal.remove { None } else { self.device_pairing_error(&modal.id) };
        let disabled = self.devices.snapshot.pending || target_error.is_some()
            || (!modal.remove && !loki_metris_core::devices::valid_pairing_code(&modal.input.read(cx).value()));
        let mut content = div().id("device-dialog").role(Role::Dialog).aria_label(title.clone()).aria_description(description.clone())
            .test_support().p_5().rounded(px(16.)).bg(cx.theme().secondary).border_1().border_color(cx.theme().border).flex().flex_col().gap_4()
            .child(div().id("device-dialog-title").role(Role::Heading).aria_level(1).aria_label(title.clone()).text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(description);
        if !modal.remove { content = content.child(Input::new(&modal.input).id("device-pair-code").aria_label(self.t("devices.codeLabel"))); }
        if let Some(error) = target_error.or(self.devices.error) {
            let message = self.device_error_text(error);
            content = content.child(div().id("device-dialog-error").test_support().role(Role::Alert).aria_label(message.clone()).text_color(cx.theme().danger).child(message));
        }
        content = content.child(div().flex().justify_end().gap_2()
            .child(SurfaceButton::new("device-dialog-cancel", SurfaceButtonVariant::SubtleBrand, dark).label(self.t("devices.cancel"))
                .on_click(cx.listener(|this, _, window, cx| this.close_device_dialog(window, cx))))
            .child(SurfaceButton::new("device-dialog-confirm", if modal.remove { SurfaceButtonVariant::FilledRed } else { SurfaceButtonVariant::FilledBrand }, dark)
                .label(self.t(if modal.remove { "devices.remove" } else { "devices.pair" })).disabled(disabled)
                .on_click(cx.listener(|this, _, window, cx| this.confirm_device_dialog(window, cx)))));
        let weak = cx.weak_entity(); let confirm = weak.clone(); let focus = modal.focus.clone();
        let input_focus = modal.input.focus_handle(cx);
        Some(Dialog::new(cx).focus_handle(modal.focus.clone()).close_on_escape(true).close_on_backdrop_press(true)
            .backdrop(DialogBackdrop::new().absolute().inset_0().bg(gpui_kit::black().opacity(0.55)))
            .popup(DialogPopup::new().w(px(440.)).max_w_full().child(content))
            .on_open_change(move |open, _, window, cx| { if !open { let _ = weak.update(cx, |this, cx| this.close_device_dialog(window, cx)); } })
            .on_ok(move |_, window, cx| {
                if !focus.is_focused(window) && !input_focus.is_focused(window) { cx.propagate(); return false; }
                let _ = confirm.update(cx, |this, cx| this.confirm_device_dialog(window, cx)); false
            }).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, KeyDownEvent, KeyUpEvent, Keystroke, TestAppContext, test::TestWindowExt};
    use gpui_kit::InputEvent as _;

    /// 稳定输入保留前导零及实体身份，同步失败必须显示在模态自身的辅助树。
    #[gpui_kit::test]
    fn code_draft_is_stable_and_submit_error_is_visible_inside_dialog(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { super::super::tests::prepare_target(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
            let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("0037", window, cx));
            window.render_frame(cx); window.render_frame(cx);
            assert_eq!(shell.read(cx).devices.modal.as_ref().unwrap().input.entity_id(), input.entity_id());
            assert_eq!(input.read(cx).value().as_ref(), "0037");
            window.click("device-dialog-confirm", cx); window.render_frame(cx);
            assert_eq!(window.find("device-dialog-error").role(), Some(Role::Alert));
            assert_eq!(window.find("device-dialog-error").label(), Some("The device service is unavailable. Try again later."));
            assert!(shell.read(cx).devices.modal.is_some());
        }).unwrap();
    }

    /// 输入后的Tab进入取消按钮，Enter/Space只能关闭并恢复触发焦点。
    #[gpui_kit::test]
    fn keyboard_cancel_does_not_submit_pairing(cx: &mut TestAppContext) {
        for key in ["enter", "space"] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx); window.click("nav-devices", cx);
                let previous = window.focused(cx);
                shell.update(cx, |this, cx| { super::super::tests::prepare_target(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
                let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
                input.update(cx, |input, cx| input.set_value("0037", window, cx)); window.render_frame(cx);
                for key in ["tab", key] {
                    let keystroke = Keystroke::parse(key).unwrap();
                    window.dispatch_event(KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false }.to_platform_input(), cx);
                    window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx); window.render_frame(cx);
                }
                assert!(shell.read(cx).devices.modal.is_none());
                assert_eq!(window.focused(cx), previous);
                assert!(shell.read(cx).devices.snapshot.saved.is_empty());
            }).unwrap();
        }
    }

    /// 广告关闭配对或候选过期后禁用确认并保留草稿，恢复能力可重新操作。
    #[gpui_kit::test]
    fn modal_rechecks_pairing_availability_without_losing_code(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                super::super::tests::prepare_target(this);
                this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx);
            });
            let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("0037", window, cx));
            for absent in [false, true] {
                shell.update(cx, |this, cx| {
                    super::super::tests::prepare_target(this);
                    if absent { this.devices.snapshot.discovered.clear(); }
                    else { this.devices.snapshot.discovered[0].pairing_available = false; }
                    cx.notify();
                });
                window.render_frame(cx);
                assert_eq!(window.find("device-dialog-error").role(), Some(Role::Alert));
                assert_eq!(input.read(cx).value().as_ref(), "0037");
                window.click("device-dialog-confirm", cx); window.render_frame(cx);
                assert!(shell.read(cx).devices.error.is_none(), "禁用确认不应触发提交错误");
                assert!(shell.read(cx).devices.modal.is_some());
            }
            shell.update(cx, |this, cx| { super::super::tests::prepare_target(this); cx.notify(); });
            window.render_frame(cx); assert!(window.try_find("device-dialog-error").is_none());
            window.click("device-dialog-confirm", cx); window.render_frame(cx);
            assert_eq!(shell.read(cx).devices.error, Some("device_unavailable"));
            window.click("device-dialog-cancel", cx); window.render_frame(cx);
            assert!(shell.read(cx).devices.modal.is_none());
        }).unwrap();
    }
}
