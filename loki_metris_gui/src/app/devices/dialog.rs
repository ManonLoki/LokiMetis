//! 配对和移除模态拥有稳定输入与焦点，不把网络状态或凭据塞入控件。

use super::Shell;
use crate::app::surface::{SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement, MouseButton, Role, Subscription, Window,
    base::{Dialog, DialogBackdrop, DialogPopup, TestSupportExt as _},
    component::{ActiveTheme, Disableable, input::OtpState}, div, prelude::*, px};

/// 临时草稿离开模态即丢弃；输入采用字符串保留四位码前导零。
pub(super) struct DeviceDialog {
    id: String,
    name: String,
    remove: bool,
    /// 打开模态时冻结的真实连接，确认与取消都绑定其票据。
    pub(super) request: Option<loki_metris_service::usage_service::DevicePairingRequest>,
    /// 已提交后保留模态到真实终态，跨页面发起也能看到握手失败。
    pub(super) submitted: bool,
    input: Entity<OtpState>,
    focus: FocusHandle,
    previous: Option<FocusHandle>,
    _subscription: Subscription,
}

impl Shell {
    /// 客户端请求或移除按钮打开模态；输入实体只在打开时创建一次。
    pub(in crate::app) fn open_device_dialog(&mut self, id: String, name: String, remove: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.devices.snapshot.pending { return; }
        if !remove && let Some(error) = self.device_pairing_error(&id) {
            self.devices.error = Some(error); cx.notify(); return;
        }
        self.devices.error = None;
        let previous = window.focused(cx); let focus = cx.focus_handle();
        let request = if remove { None } else { self.devices.snapshot.requests.iter().find(|request| request.device.id == id).cloned() };
        let input = cx.new(|cx| OtpState::new(4, window, cx).masked(false));
        let subscription = cx.subscribe(&input, |_, _, _, cx| cx.notify());
        if remove { window.focus(&focus, cx); } else { window.focus(&input.focus_handle(cx), cx); }
        self.devices.modal = Some(DeviceDialog { id, name, remove, request, submitted: false, input, focus, previous, _subscription: subscription });
        cx.notify();
    }

    /// Escape、取消和遮罩关闭恢复原焦点，并丢弃尚未提交的配对码。
    pub(super) fn close_device_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.devices.modal.take() {
            if let Some(request) = dialog.request {
                if let Some(service) = &self.dashboard.service {
                    service.reject_device_request(&dialog.id, request.ticket);
                    if dialog.submitted && self.devices.snapshot.pending { service.cancel_device_activity(); }
                }
                self.devices.snapshot.requests.retain(|current| current.device.id != dialog.id || current.ticket != request.ticket);
            }
            if let Some(focus) = dialog.previous { window.focus(&focus, cx); }
        }
        cx.notify();
    }

    /// 四位码有效才提交意图；配对模态保留到真实终态，移除沿用原确认流程。
    fn confirm_device_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.devices.modal.as_ref() else { return; };
        if self.devices.snapshot.pending || dialog.submitted { return; }
        if !dialog.remove && let Some(error) = self.device_pairing_error(&dialog.id) {
            self.devices.error = Some(error); cx.notify(); return;
        }
        let code = dialog.input.read(cx).value().to_string();
        if !dialog.remove && !loki_metris_core::devices::valid_pairing_code(&code) { return; }
        let result = match &self.dashboard.service {
            Some(service) if dialog.remove => service.remove_device(dialog.id.clone()),
            Some(service) => match &dialog.request {
                Some(request) => service.pair_device_request(&dialog.id, request.ticket, code),
                None => Err("device_request_expired"),
            },
            None => Err("device_unavailable"),
        };
        match result {
            Ok(()) => {
                self.devices.error = None;
                if dialog.remove { self.close_device_dialog(window, cx); }
                else if let Some(dialog) = &mut self.devices.modal { dialog.submitted = true; }
                self.refresh_devices(cx); cx.notify();
            },
            Err(error) => { self.devices.error = Some(error); cx.notify(); },
        }
    }

    /// 模态使用现有原生焦点陷阱，按钮自己处理Enter，背景辅助树由壳层隔离。
    pub(in crate::app) fn device_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.devices.modal.as_ref()?;
        let title = self.t(if modal.remove { "devices.removeTitle" } else { "devices.pairTitle" });
        let description = self.t(if modal.remove { "devices.removeDescription" } else { "devices.pairDescription" }).replace("{{name}}", &modal.name);
        let dark = cx.theme().mode.is_dark();
        let target_error = if modal.remove || modal.submitted { None } else { self.device_pairing_error(&modal.id) };
        let disabled = self.devices.snapshot.pending || modal.submitted || target_error.is_some()
            || (!modal.remove && !loki_metris_core::devices::valid_pairing_code(&modal.input.read(cx).value()));
        let mut content = div().id("device-dialog").role(Role::Dialog).aria_label(title.clone()).aria_description(description.clone())
            .test_support().p_5().rounded(px(16.)).bg(cx.theme().secondary).border_1().border_color(cx.theme().border).flex().flex_col().gap_4()
            .child(div().id("device-dialog-title").role(Role::Heading).aria_level(1).aria_label(title.clone()).text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(title))
            .child(description);
        if let Some(request) = &modal.request {
            let address = request.device.endpoints.first().map(|address| address.ip().to_string()).unwrap_or_default();
            content = content.child(div().id("device-request-details").test_support().flex().flex_col().gap_2()
                .child(div().id("device-request-name").test_support().role(Role::Label).aria_label(modal.name.clone()).truncate().child(modal.name.clone()))
                .child(div().id("device-request-id").test_support().role(Role::Label).aria_label(modal.id.clone()).text_size(px(12.)).child(modal.id.clone()))
                .child(div().id("device-request-ip").test_support().role(Role::Label).aria_label(address.clone()).child(address))
                .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t("devices.requestUnverified"))));
        }
        if !modal.remove { content = content.child(self.device_code_input(window, cx)); }
        if modal.submitted && self.devices.snapshot.pending {
            content = content.child(div().id("device-dialog-progress").test_support().role(Role::Status)
                .aria_label(self.t("devices.working")).child(self.t("devices.working")));
        }
        if let Some(error) = target_error.or(self.devices.error).or(if modal.submitted { self.devices.snapshot.error } else { None }) {
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

    /// 四格共用一个原生焦点并显示实际数字，逐位输入/退格和整码粘贴不改变core校验。
    fn device_code_input(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let modal = self.devices.modal.as_ref().expect("配对模态存在");
        let input = modal.input.read(cx);
        let focus = input.focus_handle(cx).tab_stop(true);
        let length = input.value().len();
        let focused = focus.is_focused(window);
        let mut field = div().id("device-pair-code").test_support().role(Role::TextInput)
            .aria_label(self.t("devices.codeLabel")).aria_value(input.value().to_string()).track_focus(&focus)
            .flex().gap_2().justify_center()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| focus.focus(window, cx))
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                let Some(modal) = &this.devices.modal else { return; };
                if modal.submitted || this.devices.snapshot.pending { return; }
                let key = &event.keystroke;
                let mut code = modal.input.read(cx).value().to_string();
                if key.modifiers.secondary(cx) && key.key == "v" {
                    window.prevent_default(); cx.stop_propagation();
                    let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else { return; };
                    if !loki_metris_core::devices::valid_pairing_code(&pasted) { return; }
                    code = pasted;
                } else {
                    if key.modifiers.control || key.modifiers.platform || key.modifiers.alt { return; }
                    if key.key == "backspace" { code.pop(); }
                    else {
                        let Some(digit) = key.key_char.as_deref().unwrap_or(&key.key).chars().next()
                            .filter(char::is_ascii_digit) else { return; };
                        if code.len() >= 4 { return; }
                        code.push(digit);
                    }
                    window.prevent_default(); cx.stop_propagation();
                }
                modal.input.update(cx, |input, cx| input.set_value(code, window, cx));
                this.devices.error = None; cx.notify();
            }));
        for index in 0..4 {
            let active = focused && index == length.min(3);
            field = field.child(div().id(("device-pair-code-cell", index)).test_support()
                .size(px(48.)).flex_shrink_0().flex().items_center().justify_center()
                .border_1().border_color(if active { cx.theme().ring } else { cx.theme().input })
                .rounded(cx.theme().radius).bg(cx.theme().secondary).text_size(px(24.))
                .when(index < length, |cell| cell.child(input.value().chars().nth(index).unwrap().to_string()))
                .when(index >= length && active, |cell| cell.child(div().h(px(20.)).w(px(2.)).bg(cx.theme().caret))));
        }
        field.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, KeyDownEvent, KeyUpEvent, Keystroke, TestAppContext, test::TestWindowExt};
    use gpui_kit::InputEvent as _;

    /// 四格有真实几何和唯一文本输入焦点，键入/退格保留前导零且不会自动提交。
    #[gpui_kit::test]
    fn otp_cells_edit_numeric_code_and_preserve_native_accessible_focus(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        assert!(cx.simulate_accessibility_activation(handle).is_some());
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { super::super::tests::prepare_request(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
            window.render_frame(cx); window.render_frame(cx);
            let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
            let field = window.find("device-pair-code").bounds();
            let mut right = field.left();
            for index in 0usize..4 {
                let cell = window.within("device-pair-code").find(("device-pair-code-cell", index)).bounds();
                assert_eq!(cell.size, gpui_kit::size(px(48.), px(48.)));
                assert!(cell.left() >= right && cell.right() <= field.right()); right = cell.right();
            }
            window.input("003", cx); window.press("backspace", cx); window.input("37", cx);
            assert_eq!(input.read(cx).value().as_ref(), "0037");
            window.input("8x", cx); assert_eq!(input.read(cx).value().as_ref(), "0037");
            assert!(shell.read(cx).devices.error.is_none(), "填满不能自动提交");
            let tree: serde_json::Value = serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
            let fields: Vec<_> = tree["nodes"].as_object().unwrap().iter().filter(|(_, node)| node["aria"]["role"] == "TextInput").collect();
            assert_eq!(fields.len(), 1);
            assert_eq!(tree["gpui_focus"].as_str(), Some(fields[0].0.as_str()));
            assert_eq!(fields[0].1["aria"]["label"], "4-digit pairing code");
            assert_eq!(fields[0].1["aria"]["value"], "0037", "明文输入向辅助技术披露当前数字");
            for _ in 0..3 { window.press("tab", cx); }
            assert_eq!(window.focused(cx), Some(input.focus_handle(cx)));
            for _ in 0..3 { window.press("shift-tab", cx); }
            assert_eq!(window.focused(cx), Some(input.focus_handle(cx)));
        }).unwrap();
    }

    /// 整码粘贴替换草稿并清错，非法粘贴保持原码，少于四位时确认按钮不可提交。
    #[gpui_kit::test]
    fn otp_paste_is_bounded_and_confirmation_requires_four_digits(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { super::super::tests::prepare_request(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
            window.render_frame(cx);
            let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
            window.input("003", cx); window.click("device-dialog-confirm", cx);
            assert!(shell.read(cx).devices.error.is_none());
            let paste = if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" };
            window.click("device-pair-code", cx);
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("0007".into())); window.press(paste, cx);
            assert_eq!(input.read(cx).value().as_ref(), "0007");
            window.click("device-dialog-confirm", cx); window.render_frame(cx);
            assert_eq!(shell.read(cx).devices.error, Some("device_unavailable"));
            window.click("device-pair-code", cx);
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("00078".into())); window.press(paste, cx);
            assert_eq!(input.read(cx).value().as_ref(), "0007");
            assert_eq!(shell.read(cx).devices.error, Some("device_unavailable"));
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("0037".into())); window.press(paste, cx);
            assert_eq!(input.read(cx).value().as_ref(), "0037");
            assert!(shell.read(cx).devices.error.is_none());
        }).unwrap();
    }

    /// 稳定输入保留前导零及实体身份，同步失败必须显示在模态自身的辅助树。
    #[gpui_kit::test]
    fn code_draft_is_stable_and_submit_error_is_visible_inside_dialog(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { super::super::tests::prepare_request(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
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

    /// 全局弹窗等待真实握手终态，设备页以外的错码仍可见且不会向新请求重复提交。
    #[gpui_kit::test]
    fn global_pairing_dialog_keeps_pending_and_failed_outcome_visible(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.page = crate::app::Page::Settings;
                super::super::tests::prepare_request(this); cx.notify();
            });
            window.render_frame(cx); window.input("0007", cx);
            shell.update(cx, |this, cx| {
                this.devices.modal.as_mut().unwrap().submitted = true;
                this.devices.snapshot.requests.clear();
                this.devices.snapshot.pending = true; this.devices.snapshot.pairing = true; cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find("device-dialog-progress").is_some());
            window.input("8", cx);
            assert_eq!(shell.read(cx).devices.modal.as_ref().unwrap().input.read(cx).value().as_ref(), "0007");
            shell.update(cx, |this, cx| {
                this.devices.snapshot.pending = false; this.devices.snapshot.pairing = false;
                this.devices.snapshot.error = Some("device_pairing_failed"); cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(window.find("device-dialog-error").role(), Some(Role::Alert));
            assert!(window.find("device-dialog-error").label().unwrap().contains("Pairing verification failed"));
            window.click("device-dialog-confirm", cx);
            assert!(shell.read(cx).devices.error.is_none(), "失败请求不能再提交到另一条连接");
            shell.update(cx, |this, cx| { this.devices.snapshot.error = None; this.devices.snapshot.completed = Some("paired"); cx.notify(); });
            window.render_frame(cx);
            assert!(shell.read(cx).devices.modal.is_none(), "只有真实成功终态才能自动关闭");
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
                shell.update(cx, |this, cx| { super::super::tests::prepare_request(this); this.open_device_dialog("cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(), "Receiver".into(), false, window, cx); });
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

    /// 同ID的新连接取代已过期请求时，旧草稿不能进入新连接的配对模态。
    #[gpui_kit::test]
    fn replacement_request_discards_previous_code_draft(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { super::super::tests::prepare_request(this); cx.notify(); });
            window.render_frame(cx);
            let input = shell.read(cx).devices.modal.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("0037", window, cx));
            shell.update(cx, |this, cx| {
                this.devices.snapshot.requests[0].ticket = 2;
                this.devices.snapshot.requests[0].device.name = "Replacement phone".into(); cx.notify();
            });
            window.render_frame(cx);
            let modal = shell.read(cx).devices.modal.as_ref().unwrap();
            assert_eq!(modal.request.as_ref().unwrap().ticket, 2);
            assert_ne!(modal.input.entity_id(), input.entity_id());
            assert_eq!(modal.input.read(cx).value().as_ref(), "");
            assert_eq!(window.find("device-request-name").label(), Some("Replacement phone"));
            window.click("device-dialog-confirm", cx);
            assert!(shell.read(cx).devices.error.is_none(), "旧配对码不能令新请求直接提交");
        }).unwrap();
    }
}
