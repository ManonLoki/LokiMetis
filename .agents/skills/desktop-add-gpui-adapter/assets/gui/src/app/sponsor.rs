//! 固定原生赞助模板：完整介绍、等高三档与独立收款区，内容和媒体仅取受管品牌源。

use super::Shell;
use gpui_kit::{
    AnyElement, Context, Div, FontWeight, IntoElement, ObjectFit, Role, Window,
    assets::IconName, base::TestSupportExt as _, component::{ActiveTheme, Icon},
    div, img, prelude::*, px, relative,
};

impl Shell {
    /// 页面自己拥有背景与留白；父滚动槽不再叠加标题栏、分割线或外层内边距。
    pub(super) fn sponsor_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let content_width = f32::from(window.viewport_size().width) - self.sidebar_width();
        div()
            .id("sponsor-page").test_support().relative().w_full().min_w_0()
            .flex_grow(1.).flex_shrink_0().flex().flex_col().px_6().pt_3().pb_4()
            .bg(cx.theme().background)
            .child(img("sponsor/bg.jpg").absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover).opacity(0.16))
            .child(
                div().relative().w_full().min_w_0().flex_grow(1.).flex().flex_col().gap_3()
                    .child(self.sponsor_intro(cx))
                    .child(
                        div().id("sponsor-tiers").test_support().w_full().min_w_0().flex_grow(1.)
                            .grid().grid_cols(if content_width >= 1000. { 3 } else { 1 }).items_stretch().gap_3()
@@SPONSOR_TIERS@@
                    )
                    .child(self.sponsor_payment(content_width >= 924., cx)),
            )
            .into_any_element()
    }

    /// 标题、维护边界、图标能力栈与完整 PC 说明共享居中轴线并自动换行。
    fn sponsor_intro(&self, cx: &Context<Self>) -> AnyElement {
        div().id("sponsor-intro").test_support().w_full().min_w_0().flex().flex_col().items_center().gap_1().text_center()
            .child(
                div().id("sponsor-title").test_support().w_full().min_w_0().whitespace_normal()
                    .text_size(px(20.)).font_weight(FontWeight::BOLD).text_color(cx.theme().primary)
                    .child(self.t("sponsor.title")),
            )
            .child(
                div().w_full().min_w_0().whitespace_normal().text_sm().font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{}{}", self.t("sponsor.subtitle_no_service"), self.t("sponsor.subtitle_thanks"))),
            )
            .child(
                div().w_full().min_w_0().flex().flex_col().items_center().gap_1().mt_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().primary).child(self.t("sponsor.capabilities_label")))
                    .child(
                        div().w_full().min_w_0().flex().flex_wrap().justify_center().gap_x_4().gap_y_1()
                            .children([
                                (IconName::PanelsTopLeft, "sponsor.capability_browser"),
                                (IconName::Globe, "sponsor.capability_proxy"),
                                (IconName::Cloud, "sponsor.capability_cloud"),
                                (IconName::Bot, "sponsor.capability_rpa"),
                            ].into_iter().map(|(icon, key)| {
                                div().flex().items_center().gap_2()
                                    .child(Icon::new(icon).size(px(20.)).text_color(cx.theme().primary))
                                    .child(div().text_sm().font_weight(FontWeight::BOLD).child(self.t(key)))
                            })),
                    )
                    .child(
                        div().w_full().min_w_0().whitespace_normal().text_xs().text_color(cx.theme().muted_foreground)
                            .child(format!("{}{}{}", self.t("sponsor.pc_focus"), self.t("sponsor.pc_platform"), self.t("sponsor.pc_growing"))),
                    ),
            )
            .into_any_element()
    }

    /// 三档完整主说明与附注由品牌 profile 生成，卡片在网格中等宽等高。
    fn sponsor_tier(&self, index: usize, name: &str, price: u16, image: &'static str, image_label: &str, benefits: &[(&str, Option<&str>)], cx: &Context<Self>) -> AnyElement {
        self.sponsor_surface(cx).id(("sponsor-tier", index)).role(Role::Article).aria_label(self.t(name)).test_support().min_w_0()
            .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(cx.theme().primary).child(self.t("sponsor.scan_to_sponsor")))
            .child(
                div().id(("sponsor-tier-summary", index)).test_support().flex().items_center().min_h(px(72.)).gap_3()
                    .child(
                        div().id(("sponsor-illustration", index)).role(Role::Image).aria_label(self.t(image_label)).test_support()
                            .flex_shrink_0().size(px(64.)).rounded(px(16.)).bg(gpui_kit::rgb(0xFFFFFF)).overflow_hidden()
                            .child(img(image).size_full().rounded(px(16.)).object_fit(ObjectFit::Contain)),
                    )
                    .child(
                        div().flex_1().min_w_0().flex().flex_col().gap_1()
                            .child(div().w_full().min_w_0().whitespace_normal().text_size(px(18.)).line_height(relative(1.25))
                                .font_weight(FontWeight::BOLD).text_color(cx.theme().primary).child(self.t(name)))
                            .child(
                                div().flex().items_baseline().gap_1()
                                    .child(div().text_size(px(if price >= 1000 { 26. } else { 32. })).line_height(relative(1.1))
                                        .font_weight(FontWeight::EXTRA_BOLD).child(price.to_string()))
                                    .child(div().text_lg().text_color(cx.theme().muted_foreground).child(self.t("sponsor.currency_unit"))),
                            ),
                    ),
            )
            .child(div().id(("sponsor-tier-divider", index)).test_support().h(px(1.)).w_full().bg(cx.theme().border))
            .child(
                div().flex().flex_col().gap_2().children(benefits.iter().map(|(main, note)| {
                    div().flex().items_start().gap_2()
                        .child(
                            div().flex_shrink_0().size(px(17.)).mt(px(2.)).rounded_full().flex().items_center().justify_center()
                                .bg(cx.theme().success)
                                .child(Icon::new(IconName::Check).size(px(11.)).text_color(cx.theme().success_foreground)),
                        )
                        .child(
                            div().flex_1().min_w_0().flex().flex_col().gap_1()
                                .child(div().w_full().min_w_0().whitespace_normal().text_sm().font_weight(FontWeight::SEMIBOLD).child(self.t(main)))
                                .when_some(*note, |column, note| column.child(div().w_full().min_w_0().whitespace_normal().text_xs().text_color(cx.theme().muted_foreground).child(self.t(note)))),
                        )
                })),
            )
            .into_any_element()
    }

    /// 宽窗左说明右二维码，窄窗上下排；二维码保留原始比例与固定白色扫描底。
    fn sponsor_payment(&self, wide: bool, cx: &Context<Self>) -> AnyElement {
        self.sponsor_surface(cx).id("sponsor-payment").test_support()
            .child(
                div().w_full().min_w_0().grid().grid_cols(if wide { 2 } else { 1 }).gap_3()
                    .child(
                        div().id("sponsor-payment-copy").test_support().min_w_0().flex().flex_col().gap_3()
                            .child(
                                div().flex().items_center().gap_2()
                                    .child(div().flex_1().min_w_0().whitespace_normal().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().primary).child(self.t("sponsor.payment_instructions")))
                                    .child(Icon::new(IconName::ArrowRight).size(px(24.)).flex_shrink_0()),
                            )
                            .child(div().w_full().min_w_0().whitespace_normal().text_sm().text_color(cx.theme().muted_foreground).child(self.t("sponsor.sponsor_message"))),
                    )
                    .child(
                        div().id("sponsor-payment-codes").test_support().min_w_0().grid().grid_cols(2).gap_4()
                            .children([
                                ("sponsor/pay1.png", "sponsor.payment_wechat_alt"),
                                ("sponsor/pay2.png", "sponsor.payment_alipay_alt"),
                            ].into_iter().enumerate().map(|(index, (image, label))| {
                                div().id(("sponsor-payment-qr", index)).role(Role::Image).aria_label(self.t(label)).test_support()
                                    .min_w_0().flex().justify_center().border_1().border_color(cx.theme().border)
                                    .child(
                                        div().id(("sponsor-qr-image", index)).test_support().w(px(120.)).h(px(136.)).flex_shrink_0().bg(gpui_kit::rgb(0xFFFFFF))
                                            .child(img(image).size_full().object_fit(ObjectFit::Contain)),
                                    )
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 卡片只消费 Kit 完整语义颜色，切换明暗主题无需重建另一份页面。
    fn sponsor_surface(&self, cx: &Context<Self>) -> Div {
        div().w_full().min_w_0().flex().flex_col().gap_2().p_3().rounded(px(16.))
            .border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
    }
}

#[cfg(test)]
#[path = "sponsor_test.rs"]
mod tests;
