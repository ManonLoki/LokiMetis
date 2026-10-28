//! 设置页底部原样展示本地赞助媒体，不创建支付操作或远程请求。

use super::super::Shell;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, ObjectFit, Role, SharedString, Text,
    base::TestSupportExt as _, component::ActiveTheme, div, img, prelude::*, px,
};

impl Shell {
    /// 付款图保留原始比例与字节，双列断点和留白沿用参考设置页。
    pub(super) fn settings_sponsor(&self, cx: &Context<Self>) -> AnyElement {
        self.settings_surface("settings-sponsor",32., cx)
            .test_support()
            .gap_5()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.settings_title("settings.sponsor_title", cx))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(Text::new(
                                "settings-sponsor-description".into(),
                                self.t("settings.sponsor_description").into(),
                            )),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(if self.viewport_width >= 768. { 2 } else { 1 })
                    .gap_5()
                    .children(
                        [
                            (
                                "sponsor-wechat",
                                crate::assets::SPONSOR_WECHAT,
                                "settings.sponsor_wechat_label",
                                "settings.sponsor_wechat_alt",
                                1304. / 1777.,
                            ),
                            (
                                "sponsor-alipay",
                                crate::assets::SPONSOR_ALIPAY,
                                "settings.sponsor_alipay_label",
                                "settings.sponsor_alipay_alt",
                                1260. / 1890.,
                            ),
                        ]
                        .into_iter()
                        .map(|(id, path, label, alt, ratio)| {
                            self.settings_plain_surface(id, 16., cx)
                                .test_support()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight(650.))
                                        .child(Text::new(
                                            SharedString::from(format!("{id}-caption")).into(),
                                            self.t(label).into(),
                                        )),
                                )
                                .child(
                                    div()
                                        .id((id, 0usize))
                                        .test_support()
                                        .w_full()
                                        .h(px(420.))
                                        .flex()
                                        .justify_center()
                                        .rounded(px(12.))
                                        .overflow_hidden()
                                        .role(Role::Image)
                                        .aria_label(self.t(alt))
                                        .child(payment_image(id, path, ratio)),
                                )
                        }),
                    ),
            )
            .into_any_element()
    }
}

/// 固定原图比例和实际宽高，使已解码与占位阶段都保持420px高。
fn payment_image(
    id: &'static str,
    source: impl Into<gpui_kit::ImageSource>,
    ratio: f32,
) -> AnyElement {
    // GPUI 0.3.9 Img / Taffy 0.13 leaf布局在clamp后用天然比例抬高height；
    // 来源：https://github.com/DioxusLabs/taffy/blob/v0.13.0/src/compute/leaf.rs#L143-L149
    // 此处直接使用已锁定原图的适配宽度，升级后须由已解码回归证明可移除。
    img(source)
        .id((id, 1usize))
        .w(px(420. * ratio))
        .h(px(420.))
        .max_w_full()
        .max_h(px(420.))
        .aspect_ratio(ratio)
        .min_w_0()
        .min_h_0()
        .object_fit(ObjectFit::Contain)
        .test_support()
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, TestAppContext, test::TestWindowExt};

    /// 实际解码两幅嵌入媒体后仍保持完整420px比例，覆盖空资源窗口遗漏的二次布局。
    #[gpui_kit::test]
    fn decoded_sponsor_images_do_not_expand_past_the_reference_height(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        for bytes in [
            include_bytes!("../../../assets/sponsor/wechat-pay.png").as_slice(),
            include_bytes!("../../../assets/sponsor/alipay.jpg").as_slice(),
        ] {
            let pixels = image::load_from_memory(bytes).unwrap().to_rgba8();
            let ratio = pixels.width() as f32 / pixels.height() as f32;
            let image =
                std::sync::Arc::new(gpui_kit::RenderImage::new(vec![image::Frame::new(pixels)]));
            for width in [344., 599.] {
                window.draw(
                    gpui_kit::point(px(0.), px(0.)),
                    gpui_kit::size(px(width), px(500.)),
                    |_, _| {
                        div()
                            .w(px(width))
                            .h(px(420.))
                            .flex()
                            .justify_center()
                            .child(payment_image("decoded-payment", image.clone(), ratio))
                            .into_any_element()
                    },
                );
                window.update(|window, _| {
                    let bounds = window.find(("decoded-payment", 1usize)).bounds();
                    assert!(
                        (bounds.size.height - px(420.)).abs() < px(0.5),
                        "{bounds:?}"
                    );
                    assert!(
                        (bounds.size.width - px(420. * ratio)).abs() < px(0.5),
                        "{bounds:?}"
                    );
                    assert!((bounds.origin.x - px((width - 420. * ratio) / 2.)).abs() < px(0.5));
                });
            }
        }
    }

    /// 宽窄窗口均限制付款图为420逻辑像素高，图片不能越过所属figure。
    #[gpui_kit::test]
    fn sponsor_images_keep_fixed_display_height_and_stay_inside_cards(cx: &mut TestAppContext) {
        for width in [960., 1440.] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 1800.);
            cx.update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| this.navigate(crate::app::Page::Settings, cx));
                window.render_frame(cx);
                for id in ["sponsor-wechat", "sponsor-alipay"] {
                    let card = window.find(id).bounds();
                    let image = window.find((id, 1usize)).bounds();
                    assert_eq!(image.size.height, gpui_kit::px(420.));
                    assert!(image.origin.x >= card.origin.x && image.right() <= card.right());
                    assert!(image.origin.y >= card.origin.y && image.bottom() <= card.bottom());
                }
            })
            .unwrap();
        }
    }
}
