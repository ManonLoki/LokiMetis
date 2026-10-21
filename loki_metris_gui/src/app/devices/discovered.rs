//! 待配对候选采用紧凑列表；未认证地址与名称不影响已配对卡片的可信状态。

use super::Shell;
use crate::app::surface::{StaticCard, StaticMaterial, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, Role, SharedString,
    base::TestSupportExt as _, component::{ActiveTheme, Disableable, Icon},
    div, prelude::*, px,
};

impl Shell {
    /// 候选按真实发现顺序排列；列表行与图标都不代理配对动作。
    pub(super) fn discovered_devices(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.devices.snapshot;
        let dark = cx.theme().mode.is_dark();
        let candidates: Vec<_> = state.discovered.iter()
            .filter(|device| !state.saved.iter().any(|saved| saved.id == device.id)).collect();
        let mut list = StaticCard::new("devices-discovered-list", dark, StaticMaterial::Panel)
            .px_5().flex().flex_col();
        if candidates.is_empty() {
            list = list.child(div().id("devices-empty").test_support().py_6().flex().items_center().gap_4()
                .child(Icon::new(IconName::Search).size(px(24.)).text_color(cx.theme().muted_foreground))
                .child(div().flex_1().text_size(px(13.)).text_color(cx.theme().muted_foreground)
                    .child(self.t(if state.scanning { "devices.searching" } else { "devices.noDiscovered" }))));
        }
        for (index, device) in candidates.iter().enumerate() {
            let target = (*device).clone();
            let mut details = div().min_w_0().flex_1().flex().flex_col().gap_1()
                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(device.name.clone()))
                .child(div().truncate().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(device.id.clone()));
            if !device.pairing_available {
                details = details.child(div().id(SharedString::from(format!("device-pairing-unavailable-{}", device.id)))
                    .test_support().role(Role::Label).aria_label(self.t("devices.pairingUnavailable"))
                    .text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t("devices.pairingUnavailable")));
            }
            list = list.child(div().id(SharedString::from(format!("device-found-{}", device.id))).test_support()
                .flex().items_center().gap_4().py_4()
                .when(index > 0, |row| row.border_t_1().border_color(cx.theme().border))
                .child(div().size(px(40.)).flex_shrink_0().flex().items_center().justify_center()
                    .rounded(px(12.)).bg(cx.theme().secondary)
                    .child(Icon::new(IconName::Smartphone).size(px(20.)).text_color(cx.theme().muted_foreground)))
                .child(details)
                .child(SurfaceButton::new(SharedString::from(format!("device-pair-{}", device.id)), SurfaceButtonVariant::LightBrand, dark)
                    .label(self.t("devices.pair")).disabled(state.pending || !state.loaded || !device.pairing_available)
                    .accessibility_label(format!("{} {}", self.t("devices.pair"), device.name))
                    .on_click(cx.listener(move |this, _, window, cx| this.open_device_dialog(target.id.clone(), target.name.clone(), false, window, cx)))));
        }
        div().id("devices-discovered").test_support().flex().flex_col().gap_4()
            .child(self.device_section_title("devices-discovered-title", "devices.discovered", candidates.len(), cx))
            .child(list)
            .into_any_element()
    }
}
