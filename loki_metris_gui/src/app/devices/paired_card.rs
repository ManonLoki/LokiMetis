//! 已配对卡片突出设备图、名称和可信IP，移除与Watch信息保留独立语义。

use super::{Shell, artwork::{artwork_path, device_artwork}};
use crate::app::surface::{StaticCard, StaticMaterial, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, Role, SharedString,
    base::TestSupportExt as _, component::{ActiveTheme, Disableable, Icon},
    div, prelude::*, px,
};
use loki_metris_core::devices::{SavedDeviceSummary, companion::WatchCompanionView,
    link::DevicePlatform, presence::{DeviceConnectionState, UnavailableReason}};

impl Shell {
    /// 左图右文的紧凑卡片只调整展示；认证元信息、IP和连接状态仍来自服务presence。
    pub(super) fn paired_device_card(&self, device: &SavedDeviceSummary, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.devices.snapshot;
        let dark = cx.theme().mode.is_dark();
        let presence = state.presence.iter().find(|presence| presence.device_id == device.id);
        let info = presence.and_then(|presence| presence.info.as_ref());
        let connection = presence.map_or(DeviceConnectionState::NotConnected, |presence| presence.state);
        let connected = connection == DeviceConnectionState::Reachable;
        let name = info.map_or_else(|| device.name.clone(), |info| info.name.clone());
        let status = self.t(match connection {
            DeviceConnectionState::Reachable => "devices.connected",
            DeviceConnectionState::NotConnected => "devices.notConnected",
            DeviceConnectionState::Unavailable(UnavailableReason::Background) => "devices.background",
            DeviceConnectionState::Unavailable(_) => "devices.unreachable",
        });
        let mut details = div().id(SharedString::from(format!("device-details-{}", device.id))).test_support()
            .min_w_0().flex_1().flex().flex_col().gap_2()
            .child(div().id(SharedString::from(format!("device-header-{}", device.id))).test_support()
                .min_w_0().flex().items_center().justify_between().gap_2()
                .child(div().min_w_0().flex_1().truncate().text_size(px(11.)).text_color(cx.theme().muted_foreground)
                    .child(info.map_or_else(|| self.t("devices.generic"), |info| match info.platform {
                        DevicePlatform::Android => "Android".into(), DevicePlatform::Ios => "iOS".into(),
                    })))
                .child(SurfaceButton::new(SharedString::from(format!("device-remove-{}", device.id)), SurfaceButtonVariant::SubtleRed, dark)
                    .label(self.t("devices.remove")).disabled(state.pending)
                    .accessibility_label(format!("{} {}", self.t("devices.remove"), name))
                    .on_click({
                        let id = device.id.clone(); let name = name.clone();
                        cx.listener(move |this, _, window, cx| this.open_device_dialog(id.clone(), name.clone(), true, window, cx))
                    })))
            .child(div().min_w_0().flex().flex_col().gap_1()
                .child(div().id(SharedString::from(format!("device-name-{}", device.id))).test_support()
                    .role(Role::Label).aria_label(name.clone()).truncate().text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                .child(div().id(SharedString::from(format!("device-status-{}", device.id))).test_support()
                    .role(Role::Label).aria_label(status.clone()).min_w_0().flex().items_start().gap_2().text_size(px(12.))
                    .text_color(if connected { cx.theme().primary } else { cx.theme().muted_foreground })
                    .child(div().mt(px(5.)).size(px(6.)).flex_shrink_0().rounded_full()
                        .bg(if connected { cx.theme().primary } else { cx.theme().muted_foreground }))
                    .child(div().min_w_0().flex_1().child(status))));
        let mut address = div().id(SharedString::from(format!("device-address-{}", device.id))).test_support()
            .w_full().min_w_0().px_2().py_1().rounded(px(8.))
            .bg(cx.theme().secondary).flex().items_center().gap_2()
            .child(Icon::new(IconName::Wifi).size(px(14.)).text_color(cx.theme().muted_foreground));
        if let Some(peer) = presence.and_then(|presence| presence.peer) {
            let ip = match peer {
                std::net::SocketAddr::V6(value) if value.scope_id() != 0 => format!("{}%{}", value.ip(), value.scope_id()),
                other => other.ip().to_string(),
            };
            let label = self.t(if connected { "devices.ip" } else { "devices.lastIp" }).replace("{{ip}}", &ip);
            address = address.child(div().id(SharedString::from(format!("device-ip-{}", device.id)))
                .test_support().role(Role::Label).aria_label(label.clone()).min_w_0().truncate().text_size(px(12.)).child(label));
        } else {
            address = address.child(div().id(SharedString::from(format!("device-ip-pending-{}", device.id)))
                .test_support().role(Role::Label).aria_label(self.t("devices.ipPending"))
                .min_w_0().flex_1().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t("devices.ipPending")));
        }
        details = details.child(address);
        if let Some(presence) = presence {
            let mut metadata = div().id(SharedString::from(format!("device-metadata-{}", device.id))).test_support()
                .min_w_0().flex().flex_col().gap_1().text_size(px(11.)).text_color(cx.theme().muted_foreground);
            if let Some(info) = info {
                let version = format!("{} {}", self.t("devices.appVersion"), info.app_version);
                metadata = metadata.child(div().id(SharedString::from(format!("device-version-{}", device.id)))
                    .role(Role::Label).aria_label(version.clone()).truncate().child(version));
            }
            if let Some(age) = presence.since_last_contact {
                metadata = metadata.child(self.t("devices.lastContact").replace("{{seconds}}", &age.as_secs().to_string()));
            }
            details = details.child(metadata);
            if let Some(watch) = &presence.companion { details = details.child(self.paired_watch_status(&device.id, watch, connected, cx)); }
        }
        if connection == DeviceConnectionState::NotConnected && state.discovered.iter().any(|candidate| candidate.id == device.id) {
            details = details.child(div().id(SharedString::from(format!("device-seen-{}", device.id))).test_support()
                .role(Role::Label).aria_label(self.t("devices.seen")).text_size(px(11.))
                .text_color(cx.theme().muted_foreground).child(self.t("devices.seen")));
        }
        let card = StaticCard::new(SharedString::from(format!("device-saved-{}", device.id)), dark, StaticMaterial::Panel)
            .test_support().p_4().h_full().flex_row().items_start().gap_4()
            .child(device_artwork(&device.id, artwork_path(info.map(|info| info.platform), presence.and_then(|presence| presence.form_factor)),
                self.t("devices.illustration").replace("{{name}}", &name)))
            .child(details);
        card.into_any_element()
    }

    /// Watch只投影在其iPhone之内，不生成独立图片、地址或配对/移除入口。
    fn paired_watch_status(&self, id: &str, watch: &WatchCompanionView, connected: bool, cx: &Context<Self>) -> AnyElement {
        let status = self.t(if connected && watch.reachable { "devices.watch.connected" } else { "devices.watch.unavailable" });
        let sync = watch.since_last_sync.map_or_else(|| self.t("devices.watch.noSync"), |age|
            self.t("devices.watch.lastSync").replace("{{seconds}}", &age.as_secs().to_string()));
        let mut row = div().id(SharedString::from(format!("device-watch-{id}"))).test_support()
            .pt_3().border_t_1().border_color(cx.theme().border).flex().flex_col().gap_1()
            .text_size(px(12.)).text_color(cx.theme().muted_foreground)
            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(format!("Watch · {}", watch.info.name)))
            .child(div().truncate().child(format!("watchOS · {} · {}", watch.info.app_version, self.t("devices.watch.viaPhone"))))
            .child(div().id(SharedString::from(format!("device-watch-status-{id}"))).test_support().role(Role::Label)
                .aria_label(status.clone()).child(status))
            .child(div().id(SharedString::from(format!("device-watch-sync-{id}"))).test_support().role(Role::Label)
                .aria_label(sync.clone()).child(sync));
        if let Some(age) = watch.since_last_contact {
            row = row.child(self.t("devices.lastContact").replace("{{seconds}}", &age.as_secs().to_string()));
        }
        row.into_any_element()
    }
}
