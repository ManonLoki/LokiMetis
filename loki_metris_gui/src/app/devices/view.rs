//! 设备页只管理已配对卡片；新的客户端配对请求由全局模态接收。

use super::Shell;
use crate::app::surface::{StaticCard, StaticMaterial, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, Role,
    base::TestSupportExt as _, component::{ActiveTheme, Icon},
    div, prelude::*, px,
};

impl Shell {
    /// 页面只投影真实状态；发现、取消和配对继续由各自语义按钮拥有。
    pub(in crate::app) fn devices_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.devices.snapshot;
        let dark = cx.theme().mode.is_dark();
        let mut page = div().id("devices-page").test_support().flex().flex_col().gap(px(28.))
            .child(div().min_w_0().flex().flex_col().gap_2()
                .child(div().id("devices-title").test_support().role(Role::Heading).aria_level(1)
                    .aria_label(self.t("devices.title")).font_weight(FontWeight::BOLD)
                    .text_size(px(24.)).child(self.t("devices.title")))
                .child(div().text_color(cx.theme().muted_foreground).child(self.t("devices.subtitle"))));
        if let Some(error) = self.devices.error.or(state.error).or(state.discovery_error).or(state.link_error) {
            page = page.child(div().id("devices-error").test_support().role(Role::Status)
                .aria_label(self.device_error_text(error)).text_color(cx.theme().danger)
                .child(self.device_error_text(error)));
        } else if let Some(completed) = state.completed {
            let feedback = self.t(match completed { "paired" => "devices.paired", "restored" => "devices.recovery.restored", _ => "devices.removed" });
            page = page.child(div().id("devices-feedback").test_support().role(Role::Status)
                .aria_label(feedback.clone()).child(feedback));
        }
        if state.pairing {
            page = page.child(div().id("devices-pending").test_support().role(Role::Status)
                .aria_label(self.t("devices.working")).flex().items_center().gap_3()
                .child(self.t("devices.working"))
                .child(SurfaceButton::new("devices-cancel", SurfaceButtonVariant::SubtleBrand, dark)
                    .label(self.t("devices.cancel"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(service) = &this.dashboard.service { service.cancel_device_activity(); }
                        this.refresh_devices(cx);
                    }))));
        }
        let mut saved = div().id("devices-saved").test_support().flex().flex_col().gap_4()
            .child(self.device_section_title("devices-saved-title", "devices.saved", state.saved.len(), cx));
        if !state.loaded || state.saved.is_empty() {
            let message = if !state.loaded {
                if state.pending { "devices.loading" } else { "devices.notLoaded" }
            } else { "devices.noSaved" };
            saved = saved.child(StaticCard::new("devices-saved-empty", dark, StaticMaterial::Panel)
                .p_6().flex().items_center().gap_3()
                .child(Icon::new(IconName::MonitorSmartphone).size(px(32.)).text_color(cx.theme().muted_foreground))
                .child(div().id("devices-empty-message").test_support().role(Role::Label).aria_label(self.t(message))
                    .font_weight(FontWeight::SEMIBOLD).child(self.t(message)))
                .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child(self.t("devices.savedHint"))));
        } else {
            // 宽度扣除80px侧栏和现有页面padding后，每卡至少约320px；不保留空白卡槽。
            let columns = if self.viewport_width >= 1280. { 3 } else if self.viewport_width >= 880. { 2 } else { 1 };
            saved = saved.child(div().id("devices-saved-grid").test_support().w_full().grid().grid_cols(columns)
                .gap_5().children(state.saved.iter().map(|device| self.paired_device_card(device, cx))));
        }
        page.child(saved).into_any_element()
    }

    /// 分区标题统一使用原生二级标题及轻量计数，避免再套一层卡片。
    pub(super) fn device_section_title(&self, id: &'static str, key: &str, count: usize, cx: &Context<Self>) -> AnyElement {
        div().flex().items_center().gap_3()
            .child(div().id(id).test_support().role(Role::Heading).aria_level(2)
                .aria_label(format!("{} ({count})", self.t(key))).text_size(px(18.))
                .font_weight(FontWeight::SEMIBOLD).child(self.t(key)))
            .child(div().px_2().py(px(2.)).rounded(px(6.)).text_size(px(12.))
                .bg(cx.theme().secondary).text_color(cx.theme().muted_foreground).child(count.to_string()))
            .into_any_element()
    }
}
