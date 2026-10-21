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
    /// 认证元信息优先于保存名称；所有IP和联机状态继续仅来自服务presence。
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
        let mut card = StaticCard::new(SharedString::from(format!("device-saved-{}", device.id)), dark, StaticMaterial::Panel)
            .test_support().p_5().gap_4().h_full()
            .child(div().flex().items_center().justify_between().gap_2()
                .child(div().min_w_0().flex_1().text_size(px(12.)).text_color(cx.theme().muted_foreground)
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
            .child(device_artwork(&device.id, artwork_path(info.map(|info| info.platform), &name),
                self.t("devices.illustration").replace("{{name}}", &name)))
            .child(div().id(SharedString::from(format!("device-name-{}", device.id))).test_support()
                .role(Role::Label).aria_label(name.clone()).truncate().text_size(px(20.))
                .font_weight(FontWeight::SEMIBOLD).child(name))
            .child(div().id(SharedString::from(format!("device-status-{}", device.id))).test_support()
                .role(Role::Label).aria_label(status.clone()).flex().items_start().gap_2().text_size(px(12.))
                .text_color(if connected { cx.theme().primary } else { cx.theme().muted_foreground })
                .child(div().mt(px(5.)).size(px(6.)).flex_shrink_0().rounded_full()
                    .bg(if connected { cx.theme().primary } else { cx.theme().muted_foreground }))
                .child(status));
        let mut address = div().w_full().min_w_0().px_3().py_3().rounded(px(12.))
            .bg(cx.theme().secondary).flex().items_center().gap_2()
            .child(Icon::new(IconName::Wifi).size(px(16.)).text_color(cx.theme().muted_foreground));
        if let Some(peer) = presence.and_then(|presence| presence.peer) {
            let ip = match peer {
                std::net::SocketAddr::V6(value) if value.scope_id() != 0 => format!("{}%{}", value.ip(), value.scope_id()),
                other => other.ip().to_string(),
            };
            let label = self.t(if connected { "devices.ip" } else { "devices.lastIp" }).replace("{{ip}}", &ip);
            address = address.child(div().id(SharedString::from(format!("device-ip-{}", device.id)))
                .test_support().role(Role::Label).aria_label(label.clone()).min_w_0().truncate().text_size(px(13.)).child(label));
        } else {
            address = address.child(div().id(SharedString::from(format!("device-ip-pending-{}", device.id)))
                .test_support().role(Role::Label).aria_label(self.t("devices.ipPending"))
                .text_size(px(13.)).text_color(cx.theme().muted_foreground).child(self.t("devices.ipPending")));
        }
        // 弹性空白使同一行卡片的IP区对齐，长状态或Watch内容仍按正常流扩展。
        card = card.child(div().flex_1()).child(address);
        if let Some(presence) = presence {
            let mut metadata = div().min_w_0().flex().flex_col().gap_1()
                .text_size(px(11.)).text_color(cx.theme().muted_foreground);
            if let Some(info) = info {
                metadata = metadata.child(div().truncate().child(format!("{} {}", self.t("devices.appVersion"), info.app_version)));
            }
            if let Some(age) = presence.since_last_contact {
                metadata = metadata.child(self.t("devices.lastContact").replace("{{seconds}}", &age.as_secs().to_string()));
            }
            card = card.child(metadata);
            if let Some(watch) = &presence.companion { card = card.child(self.paired_watch_status(&device.id, watch, connected, cx)); }
        }
        if connection == DeviceConnectionState::NotConnected && state.discovered.iter().any(|candidate| candidate.id == device.id) {
            card = card.child(div().id(SharedString::from(format!("device-seen-{}", device.id))).test_support()
                .role(Role::Label).aria_label(self.t("devices.seen")).text_size(px(11.))
                .text_color(cx.theme().muted_foreground).child(self.t("devices.seen")));
        }
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
