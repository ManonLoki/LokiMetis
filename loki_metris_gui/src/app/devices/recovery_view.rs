//! 恢复弹窗明确展示所选包和认证身份，强恢复码只在导出结果中由用户主动复制。

use super::{Shell, recovery::RecoveryKind};
use crate::app::surface::{SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Role,
    base::{Dialog, DialogBackdrop, DialogPopup, TestSupportExt as _},
    component::{ActiveTheme, Disableable, input::Input}, div, prelude::*, px};

impl Shell {
    /// 使用已有原生模态与焦点陷阱，操作按钮独立拥有动作且忙碌时不重复提交。
    pub(super) fn device_recovery_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.devices.recovery.as_ref()?; let dark = cx.theme().mode.is_dark();
        let exported = matches!(modal.kind, RecoveryKind::Exported(_));
        let title = self.t(if exported { "devices.recovery.exported" } else { "devices.recovery.import" });
        let mut content = div().id("device-recovery-dialog").test_support().role(Role::Dialog).aria_label(title.clone())
            .bg(cx.theme().background).border_1().border_color(cx.theme().border).rounded(px(16.)).p_6().flex().flex_col().gap_4()
            .child(div().id("device-recovery-title").test_support().role(Role::Heading).aria_level(1).font_weight(FontWeight::SEMIBOLD).text_size(px(18.)).child(title));
        match &modal.kind {
            RecoveryKind::Exported(code) => {
                content = content.child(div().child(self.t("devices.recovery.codeHint")))
                    .child(div().id("device-recovery-code").test_support().text_size(px(12.)).child(code.clone()))
                    .child(SurfaceButton::new("device-recovery-copy", SurfaceButtonVariant::SubtleBrand, dark)
                        .label(self.t("devices.recovery.copyCode")).on_click(cx.listener(|this, _, _, cx| {
                            if let Some(modal) = &this.devices.recovery && let RecoveryKind::Exported(code) = &modal.kind {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(code.clone()));
                            }
                        })));
            },
            RecoveryKind::Import(path) => {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                content = content.child(div().id("device-recovery-file").test_support().truncate().child(name))
                    .child(div().child(self.t("devices.recovery.enterCode")));
                if let Some(input) = &modal.input { content = content.child(Input::new(input).id("device-recovery-input")
                    .aria_label(self.t("devices.recovery.enterCode")).disabled(self.devices.recovery_busy)); }
            },
            RecoveryKind::Preview(preview) => {
                content = content.child(div().child(self.t("devices.recovery.previewHint")))
                    .child(div().id("device-recovery-preview-id").test_support().child(preview.summary.client_id.clone()))
                    .child(div().id("device-recovery-preview-count").test_support().child(format!("{}: {}", self.t("devices.saved"), preview.summary.devices.len())))
                    .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t("devices.recovery.peerRevocationHint")));
            },
        }
        if let Some(error) = self.devices.error {
            content = content.child(div().id("device-recovery-error").test_support().role(Role::Alert).aria_label(self.device_error_text(error))
                .text_color(cx.theme().danger).child(self.device_error_text(error)));
        }
        if self.devices.recovery_busy { content = content.child(div().id("device-recovery-progress").test_support().role(Role::Status).child(self.t("devices.working"))); }
        let mut actions = div().flex().justify_end().gap_2().child(SurfaceButton::new("device-recovery-close", SurfaceButtonVariant::SubtleBrand, dark)
            .label(self.t(if exported { "devices.recovery.done" } else { "devices.cancel" })).disabled(self.devices.recovery_busy)
            .on_click(cx.listener(|this, _, window, cx| this.close_device_recovery(window, cx))));
        if !exported { actions = actions.child(SurfaceButton::new("device-recovery-confirm", SurfaceButtonVariant::FilledBrand, dark)
            .label(self.t(if matches!(modal.kind, RecoveryKind::Preview(_)) { "devices.recovery.confirm" } else { "devices.recovery.preview" }))
            .disabled(self.devices.recovery_busy).on_click(cx.listener(|this, _, window, cx| this.confirm_device_recovery(window, cx)))); }
        content = content.child(actions);
        let weak = cx.weak_entity();
        Some(Dialog::new(cx).focus_handle(modal.focus.clone()).close_on_escape(!self.devices.recovery_busy).close_on_backdrop_press(!self.devices.recovery_busy)
            .backdrop(DialogBackdrop::new().absolute().inset_0().bg(gpui_kit::black().opacity(0.55)))
            .popup(DialogPopup::new().w(px(560.)).max_w_full().child(content))
            .on_open_change(move |open, _, window, cx| { if !open { let _ = weak.update(cx, |this, cx| this.close_device_recovery(window, cx)); } })
            .into_any_element())
    }
}
