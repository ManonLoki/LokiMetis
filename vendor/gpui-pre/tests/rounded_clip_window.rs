#![cfg(feature = "test-support")]
//! 实际 Window 绘制验证新旧入口、原始圆角坐标和空视口零上传；不声称模拟平台产生 GPU 像素。

use gpui::{AppContext, Bounds, Context, Corners, ImageRoundedClip, IntoElement, Render, RenderImage, ScaledPixels, Styled, TestAppContext, Window, canvas, point, px, size};
use std::sync::Arc;

/// 两个入口使用同一帧，视口与图片交集故意不同以复现原始圆心丢失。
struct Probe { image: Arc<RenderImage>, empty: bool }
impl Render for Probe {
    /// 原生 canvas 提供 paint 阶段，图片图元直接进入 Window 的已完成帧。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let image = self.image.clone();
        let empty = self.empty;
        canvas(|bounds, _, _| bounds, move |viewport, _, window, _| {
            let image_bounds = Bounds::new(viewport.origin + point(px(10.), px(0.)), size(px(20.), px(80.)));
            let corners = Corners::all(px(20.));
            let clip = ImageRoundedClip {
                bounds: if empty { Bounds::new(viewport.origin, size(px(0.), px(80.))) } else { viewport },
                corner_radii: corners.clone(),
            };
            window.paint_image_with_rounded_clip(image_bounds, image_bounds, clip, image.clone(), 0, false).unwrap();
            if !empty {
                window.paint_image(viewport, image_bounds, corners, image.clone(), 0, false).unwrap();
            }
        }).w(px(320.)).h(px(180.))
    }
}

/// 共享小帧不依赖图片解码器，不改变产品缓存或租约。
fn image() -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![image::Frame::new(image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255])))]))
}

/// 新入口保留原始视口半径，旧入口仍按交集限制；两个图元复用同一个 atlas tile。
#[gpui::test]
fn window_preserves_independent_clip_and_legacy_api(cx: &mut TestAppContext) {
    for factor in [1., 2.] {
        let data = image();
        let handle = cx.add_window(|_, _| Probe { image: data.clone(), empty: false });
        cx.simulate_window_scale_factor_change(handle.into(), factor);
        cx.update_window(handle.into(), |_, window, cx| {
            window.refresh(); window.draw(cx).clear(cx);
            let sprites = window.painted_polychrome_sprites();
            assert_eq!(sprites.len(), 2);
            let (rounded, legacy) = (&sprites[0], &sprites[1]);
            assert_eq!(rounded.bounds, legacy.bounds);
            assert_eq!(rounded.tile, legacy.tile);
            assert_eq!(rounded.rounded_clip_bounds.origin.x, ScaledPixels(0.));
            assert_eq!(rounded.rounded_clip_bounds.size.width, ScaledPixels(320. * factor));
            assert_eq!(rounded.corner_radii.top_left, ScaledPixels(20. * factor));
            assert_eq!(legacy.rounded_clip_bounds, legacy.bounds);
            assert_eq!(legacy.corner_radii.top_left, ScaledPixels(10. * factor));
            assert!(window.has_image_atlas_entry(&data));
            window.drop_image(data.clone()).unwrap();
            assert!(!window.has_image_atlas_entry(&data));
        }).unwrap();
    }
}

/// 空裁切在图集申请前结束，不能遗留纹理条目或图片图元。
#[gpui::test]
fn empty_rounded_clip_does_not_upload(cx: &mut TestAppContext) {
    let data = image();
    let handle = cx.add_window(|_, _| Probe { image: data.clone(), empty: true });
    cx.update_window(handle.into(), |_, window, cx| {
        window.refresh(); window.draw(cx).clear(cx);
        assert!(window.painted_polychrome_sprites().is_empty());
        assert!(!window.has_image_atlas_entry(&data));
    }).unwrap();
}
