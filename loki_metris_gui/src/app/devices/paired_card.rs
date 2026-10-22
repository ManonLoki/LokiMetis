//! 已配对卡片突出设备图、名称和可信IP，移除与Watch信息保留独立语义。

use super::{Shell, artwork::{artwork_path, device_artwork, watch_artwork}};
use crate::app::surface::{StaticCard, StaticMaterial, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, Role, SharedString,
    base::TestSupportExt as _, component::{ActiveTheme, Disableable},
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
        let platform = info.map_or_else(|| self.t("devices.generic"), |info| match info.platform {
            DevicePlatform::Android => "Android".into(), DevicePlatform::Ios => "iOS".into(),
        });
        let status_tag = self.device_connection_tag("device-status", &device.id, status, connected, cx);
        let mut identity = div().min_w_0().flex().items_center().gap_2()
            .child(div().id(SharedString::from(format!("device-name-{}", device.id))).test_support()
                .role(Role::Label).aria_label(name.clone()).min_w_0().flex_1().truncate().text_size(px(18.))
                .font_weight(FontWeight::SEMIBOLD).child(name.clone()));
        if let Some(info) = info {
            identity = identity.child(div().id(SharedString::from(format!("device-version-{}", device.id)))
                .test_support().role(Role::Label).aria_label(info.app_version.clone())
                .min_w_0().flex_shrink_0().max_w(gpui_kit::relative(0.35)).truncate()
                .text_size(px(11.)).text_color(cx.theme().muted_foreground).child(info.app_version.clone()));
        }
        let mut details = div().id(SharedString::from(format!("device-details-{}", device.id))).test_support()
            .min_w_0().flex_1().flex().flex_col().gap(px(6.))
            .child(div().id(SharedString::from(format!("device-header-{}", device.id))).test_support()
                .min_w_0().flex().items_center().justify_between().gap_2()
                .child(div().min_w_0().flex_1().flex().items_center().gap_2()
                    .child(div().id(SharedString::from(format!("device-platform-{}", device.id))).test_support()
                        .role(Role::Label).aria_label(platform.clone()).min_w_0().truncate().text_size(px(11.))
                        .text_color(cx.theme().muted_foreground).child(platform))
                    .child(status_tag))
                .child(SurfaceButton::new(SharedString::from(format!("device-remove-{}", device.id)), SurfaceButtonVariant::SubtleRed, dark)
                    .label(self.t("devices.remove")).disabled(state.pending)
                    .accessibility_label(format!("{} {}", self.t("devices.remove"), name))
                    .on_click({
                        let id = device.id.clone(); let name = name.clone();
                        cx.listener(move |this, _, window, cx| this.open_device_dialog(id.clone(), name.clone(), true, window, cx))
                    })))
            .child(identity);
        let mut address = div().id(SharedString::from(format!("device-address-{}", device.id))).test_support()
            .flex_1().min_w_0().flex().items_center();
        if let Some(peer) = presence.and_then(|presence| presence.peer) {
            let ip = match peer {
                std::net::SocketAddr::V6(value) if value.scope_id() != 0 => format!("{}%{}", value.ip(), value.scope_id()),
                other => other.ip().to_string(),
            };
            let label = self.t(if connected { "devices.ip" } else { "devices.lastIp" }).replace("{{ip}}", &ip);
            address = address.child(div().id(SharedString::from(format!("device-ip-{}", device.id)))
                .test_support().role(Role::Label).aria_label(label.clone()).min_w_0().flex_1().truncate().text_size(px(12.)).child(label));
        } else {
            address = address.child(div().id(SharedString::from(format!("device-ip-pending-{}", device.id)))
                .test_support().role(Role::Label).aria_label(self.t("devices.ipPending"))
                .min_w_0().flex_1().truncate().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t("devices.ipPending")));
        }
        let mut metadata = div().id(SharedString::from(format!("device-metadata-{}", device.id))).test_support()
            .min_w_0().flex().items_center().gap_2().child(address);
        if let Some(presence) = presence {
            if let Some(age) = presence.since_last_contact {
                let contact = self.t("devices.lastContact").replace("{{seconds}}", &age.as_secs().to_string());
                metadata = metadata.child(div().id(SharedString::from(format!("device-contact-{}", device.id)))
                    .test_support().role(Role::Label).aria_label(contact.clone()).min_w_0().flex_shrink_0().truncate()
                    .max_w(gpui_kit::relative(0.6)).text_size(px(11.))
                    .text_color(cx.theme().muted_foreground).child(contact));
            }
        }
        details = details.child(metadata);
        if let Some(watch) = presence.and_then(|presence| presence.companion.as_ref()) {
            details = details.child(self.paired_watch_status(&device.id, watch, connected, cx));
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

    /// 手机和附属Watch共用绿色在线Tag，辅助名称与悬停提示保留完整通信原因。
    fn device_connection_tag(&self, prefix: &str, id: &str, status: String, connected: bool, cx: &Context<Self>) -> AnyElement {
        let tag = self.t(if connected { "devices.online" } else { "devices.offline" });
        div().id(SharedString::from(format!("{prefix}-{id}"))).test_support()
            .role(Role::Label).aria_label(status.clone()).flex_shrink_0().px_2().py(px(2.))
            .rounded(px(6.)).text_size(px(11.))
            .bg(if connected { cx.theme().success.opacity(0.12) } else { cx.theme().secondary })
            .text_color(if connected { cx.theme().success } else { cx.theme().muted_foreground })
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(status.clone()).build(window, cx))
            .child(div().id(SharedString::from(format!("{prefix}-tag-{id}"))).test_support()
                .role(Role::Label).aria_label(tag.clone()).child(tag)).into_any_element()
    }

    /// Watch在iPhone内以小图和真实名称展示；独立回执与父连接共同决定在线，不提供独立设备操作。
    fn paired_watch_status(&self, id: &str, watch: &WatchCompanionView, connected: bool, cx: &Context<Self>) -> AnyElement {
        let reachable = connected && watch.reachable;
        let status = self.t(if reachable { "devices.watch.connected" } else { "devices.watch.unavailable" });
        let name = format!("Watch · {}", watch.info.name);
        let sync = watch.since_last_sync.map_or_else(|| self.t("devices.watch.noSync"), |age|
            self.t("devices.watch.lastSync").replace("{{seconds}}", &age.as_secs().to_string()));
        let mut details = div().id(SharedString::from(format!("device-watch-details-{id}"))).test_support()
            .min_w_0().flex_1().flex().flex_col().gap_1()
            .text_size(px(12.)).text_color(cx.theme().muted_foreground)
            .child(div().min_w_0().flex().items_center().gap_2()
                .child(div().id(SharedString::from(format!("device-watch-name-{id}"))).test_support()
                    .role(Role::Label).aria_label(name.clone()).min_w_0().flex_1().truncate()
                    .font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().foreground).child(name))
                .child(self.device_connection_tag("device-watch-status", id, status, reachable, cx)))
            .child(div().truncate().child(format!("watchOS · {} · {}", watch.info.app_version, self.t("devices.watch.viaPhone"))))
            .child(div().id(SharedString::from(format!("device-watch-sync-{id}"))).test_support().role(Role::Label)
                .aria_label(sync.clone()).truncate().child(sync));
        if let Some(age) = watch.since_last_contact {
            details = details.child(div().truncate().child(self.t("devices.lastContact").replace("{{seconds}}", &age.as_secs().to_string())));
        }
        div().id(SharedString::from(format!("device-watch-{id}"))).test_support()
            .min_w_0().pt_3().border_t_1().border_color(cx.theme().border).flex().items_start().gap_2()
            .child(watch_artwork(id, self.t("devices.illustration").replace("{{name}}", &watch.info.name)))
            .child(details).into_any_element()
    }
}
