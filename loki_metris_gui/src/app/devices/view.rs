//! 已配对和发现列表采用现有品牌面板与语义按钮，整行不代理配对或移除动作。

use super::Shell;
use crate::app::surface::{StaticCard, StaticMaterial, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Role, SharedString,
    base::TestSupportExt as _, component::{ActiveTheme, Disableable}, div, prelude::*, px};

impl Shell {
    /// 页面显示真实发现状态；接收端尚未运行时清楚说明空列表的原因。
    pub(in crate::app) fn devices_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.devices.snapshot;
        let busy = state.pending;
        let dark = cx.theme().mode.is_dark();
        let mut page = div().id("devices-page").test_support().flex().flex_col().gap_5()
            .child(div().flex().items_center().justify_between().gap_4()
                .child(div().flex().flex_col().gap_2()
                    .child(div().id("devices-title").role(Role::Heading).aria_level(1).aria_label(self.t("devices.title")).font_weight(FontWeight::BOLD).text_size(px(24.)).child(self.t("devices.title")))
                    .child(div().text_color(cx.theme().muted_foreground).child(self.t("devices.subtitle"))))
                .child(SurfaceButton::new("devices-discover", SurfaceButtonVariant::FilledBrand, dark)
                    .h(px(36.)).px_4().label(self.t(if state.scanning { "devices.scanning" } else { "devices.discover" }))
                    .disabled(state.scanning || busy)
                    .on_click(cx.listener(|this, _, _, cx| this.enter_devices(cx)))))
            .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child(self.t("devices.compatibility")));
        if let Some(error) = self.devices.error.or(state.error).or(state.discovery_error) {
            page = page.child(div().id("devices-error").test_support().role(Role::Status).aria_label(self.device_error_text(error))
                .text_color(cx.theme().danger).child(self.device_error_text(error)));
        } else if let Some(completed) = state.completed {
            page = page.child(div().id("devices-feedback").test_support().role(Role::Status)
                .aria_label(self.t(if completed == "paired" { "devices.paired" } else { "devices.removed" }))
                .child(self.t(if completed == "paired" { "devices.paired" } else { "devices.removed" })));
        }
        if state.pairing {
            page = page.child(div().id("devices-pending").test_support().role(Role::Status).aria_label(self.t("devices.working")).flex().items_center().gap_3()
                .child(self.t("devices.working"))
                .child(SurfaceButton::new("devices-cancel", SurfaceButtonVariant::SubtleBrand, dark)
                    .label(self.t("devices.cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(service) = &this.dashboard.service { service.cancel_device_activity(); }
                        this.refresh_devices(cx);
                    }))));
        }
        let mut saved = StaticCard::new("devices-saved", dark, StaticMaterial::Panel).test_support().p_5().flex().flex_col().gap_4()
            .child(div().id("devices-saved-title").role(Role::Heading).aria_level(2).aria_label(format!("{} ({})", self.t("devices.saved"), state.saved.len())).font_weight(FontWeight::SEMIBOLD).text_size(px(18.))
                .child(format!("{} ({})", self.t("devices.saved"), state.saved.len())));
        if !state.loaded { saved = saved.child(self.t(if state.pending { "devices.loading" } else { "devices.notLoaded" })); }
        else if state.saved.is_empty() { saved = saved.child(div().text_color(cx.theme().muted_foreground).child(self.t("devices.noSaved"))); }
        for device in &state.saved {
            let found = state.discovered.iter().any(|candidate| candidate.id == device.id);
            let device = device.clone(); let target = device.clone();
            saved = saved.child(div().id(SharedString::from(format!("device-saved-{}", device.id))).test_support()
                .flex().items_center().justify_between().gap_4().py_2()
                .child(div().min_w_0().flex_1().flex().flex_col().gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(device.name))
                    .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(device.id.clone()))
                    .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child(self.t(if found { "devices.seen" } else { "devices.notSeen" }))))
                .child(SurfaceButton::new(SharedString::from(format!("device-remove-{}", device.id)), SurfaceButtonVariant::SubtleRed, dark)
                    .label(self.t("devices.remove")).disabled(busy)
                    .accessibility_label(format!("{} {}", self.t("devices.remove"), target.name))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_device_dialog(target.id.clone(), target.name.clone(), true, window, cx)))));
        }
        let mut discovered = StaticCard::new("devices-discovered", dark, StaticMaterial::Panel).test_support().p_5().flex().flex_col().gap_4()
            .child(div().id("devices-discovered-title").role(Role::Heading).aria_level(2).aria_label(self.t("devices.discovered")).font_weight(FontWeight::SEMIBOLD).text_size(px(18.)).child(self.t("devices.discovered")));
        let candidates: Vec<_> = state.discovered.iter().filter(|device| !state.saved.iter().any(|saved| saved.id == device.id)).collect();
        if candidates.is_empty() { discovered = discovered.child(div().id("devices-empty").test_support().text_color(cx.theme().muted_foreground)
            .child(self.t(if state.scanning { "devices.searching" } else { "devices.noDiscovered" }))); }
        for device in candidates {
            let target = device.clone();
            discovered = discovered.child(div().id(SharedString::from(format!("device-found-{}", device.id))).test_support()
                .flex().items_center().justify_between().gap_4().py_2()
                .child(div().min_w_0().flex_1().flex().flex_col().gap_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(device.name.clone()))
                    .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(device.id.clone()))
                    .when(!device.pairing_available, |row| row.child(div().id(SharedString::from(format!("device-pairing-unavailable-{}", device.id)))
                        .test_support().role(Role::Label).aria_label(self.t("devices.pairingUnavailable"))
                        .text_size(px(13.)).text_color(cx.theme().muted_foreground).child(self.t("devices.pairingUnavailable")))))
                .child(SurfaceButton::new(SharedString::from(format!("device-pair-{}", device.id)), SurfaceButtonVariant::LightBrand, dark)
                    .label(self.t("devices.pair")).disabled(busy || !state.loaded || !device.pairing_available)
                    .accessibility_label(format!("{} {}", self.t("devices.pair"), device.name))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_device_dialog(target.id.clone(), target.name.clone(), false, window, cx)))));
        }
        page.child(saved).child(discovered).into_any_element()
    }
}
