//! 同一真实Metal视图从中性图标切换到嵌入手机/平板及Watch，直接核对最终像素。

use super::artwork::{artwork_path, device_artwork, watch_artwork};
use crate::app::surface::{CardPriority, StaticCard, StaticMaterial};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext, Bounds, Context, Entity, HeadlessAppContext,
    IntoElement, NoopTextSystem, Pixels, Render, Window, div, prelude::*, px, rgb, size,
    test::TestWindowExt,
};
use loki_metris_core::devices::link::{DeviceFormFactor, DevicePlatform};
use std::sync::Arc;

/// 只持有测试展示输入；实际路径选择、img及Contain全部复用生产设备插图函数。
struct ArtworkProbe {
    /// 两种真实静态卡片材质分别执行，覆盖深浅色父层绘制。
    dark: bool,
    /// 先绘制同窗空背景，避免把原生清屏色误当作产品背景。
    visible: bool,
    /// 两个移动槽同时从缺失形态切到手机、平板，再回到中性状态。
    form_factor: Option<DeviceFormFactor>,
}

/// 空槽只用于同窗无图片像素基线；尺寸和id保持一致，不把空槽当成功绘制结果。
fn empty_slot(id: &'static str, width: f32, height: f32) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .w(px(width))
        .h(px(height))
        .flex_shrink_0()
        .into_any_element()
}

impl Render for ArtworkProbe {
    /// 设备插图包在生产StaticCard的相同材质、优先级和padding内，覆盖父层clip/opacity。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut cards = Vec::new();
        for (id, platform) in [
            ("ios", DevicePlatform::Ios),
            ("android", DevicePlatform::Android),
        ] {
            let artwork = if self.visible {
                device_artwork(
                    id,
                    artwork_path(Some(platform), self.form_factor),
                    format!("{id} test device"),
                )
            } else {
                empty_slot(
                    if id == "ios" {
                        "device-artwork-ios"
                    } else {
                        "device-artwork-android"
                    },
                    96.,
                    112.,
                )
            };
            let mut details = div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
                .child(div().h(px(56.)));
            if id == "ios" {
                details = details.child(if self.visible && self.form_factor.is_some() {
                    watch_artwork("ios", "Watch test device".into())
                } else {
                    empty_slot("device-watch-artwork-ios", 40., 48.)
                });
            }
            cards.push(
                StaticCard::new(
                    if id == "ios" {
                        "device-saved-ios"
                    } else {
                        "device-saved-android"
                    },
                    self.dark,
                    StaticMaterial::Panel,
                )
                .priority(CardPriority::Secondary)
                .test_support()
                .w(px(320.))
                .p_4()
                .h_full()
                .flex_row()
                .items_start()
                .gap_4()
                .child(artwork)
                .child(details),
            );
        }
        div()
            .size_full()
            .bg(rgb(0x5c284e))
            .text_color(cx.theme().foreground)
            .p(px(16.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(16.))
                    .h(px(160.))
                    .children(cards),
            )
    }
}

/// 驱动真实资源异步解码和正式render/prepaint/paint；截图始终读取Metal最终帧。
fn capture(cx: &mut HeadlessAppContext, handle: AnyWindowHandle) -> image::RgbaImage {
    for _ in 0..4 {
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .expect("设备插图测试窗口必须存活");
        cx.run_until_parked();
    }
    cx.capture_screenshot(handle)
        .expect("设备插图必须从真实Metal读回")
}

/// 在原根实体原窗口更新形态，覆盖异步图片替换，而非每轮重新创建测试视图。
fn show(
    cx: &mut HeadlessAppContext,
    probe: &Entity<ArtworkProbe>,
    form_factor: Option<DeviceFormFactor>,
) {
    probe.update(cx, |probe, cx| {
        probe.visible = true;
        probe.form_factor = form_factor;
        cx.notify();
    });
}

/// 通过生产设备槽的测试标识取布局，只用它定位最终像素，不把布局当作绘制成功。
fn slot(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, id: &str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, _| window.find(id.to_owned()).bounds())
        .expect("设备图像槽必须存在")
}

/// 以逻辑槽定位物理截图，对比同窗背景或前一状态；轻微混合舍入不算有效图像。
fn changed_pixels(
    image: &image::RgbaImage,
    reference: &image::RgbaImage,
    bounds: Bounds<Pixels>,
) -> (usize, usize) {
    assert_eq!(image.dimensions(), reference.dimensions());
    let scale = image.width() as f32 / 688.;
    let left = (f32::from(bounds.left()) * scale).ceil() as u32;
    let right = (f32::from(bounds.right()) * scale).floor() as u32;
    let top = (f32::from(bounds.top()) * scale).ceil() as u32;
    let bottom = (f32::from(bounds.bottom()) * scale).floor() as u32;
    let mut changed = 0;
    for y in top..bottom {
        for x in left..right {
            let actual = image.get_pixel(x, y).0;
            let expected = reference.get_pixel(x, y).0;
            if (0..3).any(|channel| actual[channel].abs_diff(expected[channel]) > 3) {
                changed += 1;
            }
        }
    }
    (changed, ((right - left) * (bottom - top)) as usize)
}

/// 大面积主体像素必须区别于空背景及中性图标，避免只画了小fallback也被判成功。
fn assert_device_visible(
    image: &image::RgbaImage,
    empty: &image::RgbaImage,
    fallback: &image::RgbaImage,
    bounds: Bounds<Pixels>,
    label: &str,
) {
    let (body, total) = changed_pixels(image, empty, bounds);
    assert!(
        body > total / 8,
        "{label}必须有实际设备主体像素：{body}/{total}"
    );
    let (replacement, _) = changed_pixels(image, fallback, bounds);
    assert!(
        replacement > total / 8,
        "{label}必须替换中性fallback，不能保留原小图标：{replacement}/{total}"
    );
}

/// Contain上下留白必须保留背景；传入已避开插值边缘的几何保证区，不采样透明主体边角。
fn assert_vertical_padding(
    image: &image::RgbaImage,
    empty: &image::RgbaImage,
    bounds: Bounds<Pixels>,
    safe_padding: f32,
    label: &str,
) {
    let mut top = bounds;
    top.size.height = px(safe_padding);
    let mut bottom = bounds;
    bottom.origin.y = bounds.bottom() - px(safe_padding);
    bottom.size.height = px(safe_padding);
    assert_eq!(
        changed_pixels(image, empty, top).0,
        0,
        "{label}上方Contain留白不能被拉伸填满"
    );
    assert_eq!(
        changed_pixels(image, empty, bottom).0,
        0,
        "{label}下方Contain留白不能被拉伸填满"
    );
}

/// 实际嵌入图片经过生产img(path)绘制；四移动图与Watch在同一窗口替换后都保留主体和Contain。
#[test]
fn actual_metal_mobile_artwork_updates_from_fallback_to_phone_and_tablet() {
    for dark in [false, true] {
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(NoopTextSystem),
            Arc::new(crate::assets::Assets),
            || {
                let renderer = gpui_kit::platform::current_headless_renderer()?
                    .expect("设备插图回归必须创建真实Metal渲染器");
                Ok(Some(renderer))
            },
        );
        cx.update(|app| {
            gpui_kit::init(app);
            Theme::change(
                if dark {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                },
                None,
                app,
            );
        });
        let handle = cx
            .open_window(size(px(688.), px(192.)), |_, app| {
                app.new(|_| ArtworkProbe {
                    dark,
                    visible: false,
                    form_factor: None,
                })
            })
            .expect("创建隔离的设备插图Metal窗口");
        let probe = handle.update(&mut cx, |_, _, cx| cx.entity()).unwrap();
        let handle: AnyWindowHandle = handle.into();
        let empty = capture(&mut cx, handle);

        show(&mut cx, &probe, None);
        let fallback = capture(&mut cx, handle);
        let ios = slot(&mut cx, handle, "device-artwork-ios");
        let android = slot(&mut cx, handle, "device-artwork-android");
        for bounds in [ios, android] {
            assert!(
                changed_pixels(&fallback, &empty, bounds).0 > 0,
                "未知形态仍须显示中性图标"
            );
        }

        show(&mut cx, &probe, Some(DeviceFormFactor::Phone));
        let phones = capture(&mut cx, handle);
        let watch = slot(&mut cx, handle, "device-watch-artwork-ios");
        for (bounds, label) in [
            (ios, "iPhone"),
            (android, "Android phone"),
            (watch, "Watch"),
        ] {
            assert_device_visible(&phones, &empty, &fallback, bounds, label);
        }
        // 512×512原图在96×112槽形成8px上下留白；Watch的40×48槽形成4px，避开1px过滤边界。
        assert_vertical_padding(&phones, &empty, ios, 7., "iPhone");
        assert_vertical_padding(&phones, &empty, android, 7., "Android phone");
        assert_vertical_padding(&phones, &empty, watch, 3., "Watch");

        show(&mut cx, &probe, Some(DeviceFormFactor::Tablet));
        let tablets = capture(&mut cx, handle);
        for (bounds, label) in [(ios, "iPad"), (android, "Android tablet")] {
            assert_device_visible(&tablets, &empty, &fallback, bounds, label);
            let (replacement, total) = changed_pixels(&tablets, &phones, bounds);
            assert!(
                replacement > total / 8,
                "{label}必须显示新平板图，不能沿用上一轮手机图"
            );
        }
        assert_vertical_padding(&tablets, &empty, ios, 7., "iPad");
        // 768×512横平板在96×112槽形成24px上下留白，避开1px过滤边界。
        assert_vertical_padding(&tablets, &empty, android, 23., "Android tablet");
        assert_eq!(
            changed_pixels(&tablets, &phones, watch).0,
            0,
            "父设备换图不能吞掉或改变相同Watch图片"
        );

        show(&mut cx, &probe, None);
        let reset = capture(&mut cx, handle);
        for bounds in [ios, android] {
            assert_eq!(
                changed_pixels(&reset, &fallback, bounds).0,
                0,
                "当前连接缺失形态时恢复中性图标，不能残留上一轮平板图"
            );
        }
        assert_eq!(
            changed_pixels(&reset, &empty, watch).0,
            0,
            "未挂载Watch不得残留旧图片"
        );
    }
}
