//! 从真实 Metal shader 读回像素，证明 Contain 原始圆心、祖先 mask、帧、透明度和图集释放。

use crate::metal_renderer::MetalHeadlessRenderer;
use gpui::{AtlasTile, Bounds, ContentMask, Corners, DevicePixels, PlatformHeadlessRenderer, Point, PolychromeSprite, RenderImage, RenderImageParams, ScaledPixels, Scene, point, size};
use std::{borrow::Cow, sync::Arc};

/// 全白及半透明帧消除采样纹理差异，让错误圆角和透明度具有可见读回证据。
fn fixture() -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![
        image::Frame::new(image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]))),
        image::Frame::new(image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 128]))),
        image::Frame::new(image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 255, 0, 255]))),
    ]))
}

/// 原图键只包含 image id 与 frame，圆角视口绝不参与图片上传或缓存身份。
fn tile(renderer: &MetalHeadlessRenderer, data: &RenderImage, frame: usize) -> AtlasTile {
    let mut tile = renderer.sprite_atlas().get_or_insert_with(RenderImageParams { image_id: data.id, frame_index: frame }.into(), &mut || {
        Ok(Some((data.size(frame), Cow::Borrowed(data.as_bytes(frame).unwrap()))))
    }).unwrap().unwrap();
    // 内侧子纹理保留各边一个源像素保护区，避免线性采样读到相邻 atlas 内容。
    tile.bounds.origin = tile.bounds.origin + point(DevicePixels(1), DevicePixels(1));
    tile.bounds.size = size(DevicePixels(2), DevicePixels(2));
    tile
}

/// 按完整视口设置圆角，按真实图片矩形设置几何，祖先矩形裁切独立保留。
fn scene(tile: AtlasTile, viewport: Bounds<ScaledPixels>, image: Bounds<ScaledPixels>, radius: f32, mask: Bounds<ScaledPixels>) -> Scene {
    let mut scene = Scene::default();
    scene.insert_primitive(PolychromeSprite {
        order: 0, pad: 0, grayscale: false.into(), opacity: 1.,
        bounds: image, content_mask: ContentMask { bounds: mask },
        corner_radii: Corners::all(ScaledPixels(radius)), tile, rounded_clip_bounds: viewport,
    });
    scene.finish();
    scene
}

/// 解析几何只判定远离抗锯齿边界的内外点，像素本身来自真实 GPU。
fn distance(point: Point<f32>, viewport: Bounds<ScaledPixels>, radius: f32) -> f32 {
    let x = point.x.clamp(viewport.origin.x.0 + radius, viewport.origin.x.0 + viewport.size.width.0 - radius);
    let y = point.y.clamp(viewport.origin.y.0 + radius, viewport.origin.y.0 + viewport.size.height.0 - radius);
    ((point.x - x).powi(2) + (point.y - y).powi(2)).sqrt() - radius
}

/// 留白小于半径、等于半径、超过半径及横竖方向都沿完整 viewport 的圆曲线裁切。
#[test]
fn contain_rounding_uses_original_viewport_pixels() {
    let mut renderer = MetalHeadlessRenderer::new();
    let data = fixture();
    let tile = tile(&renderer, &data, 0);
    for factor in [1., 1.5, 2.] {
        let viewport = Bounds::new(point(ScaledPixels(0.), ScaledPixels(0.)), size(ScaledPixels(80. * factor), ScaledPixels(60. * factor)));
        for vertical in [false, true] {
            for gap in [0., 0.5, 4., 10., 19.5, 20., 20.5] {
                let image = if vertical {
                    Bounds::new(point(ScaledPixels(0.), ScaledPixels(gap * factor)), size(viewport.size.width, ScaledPixels((60. - gap * 2.) * factor)))
                } else {
                    Bounds::new(point(ScaledPixels(gap * factor), ScaledPixels(0.)), size(ScaledPixels((80. - gap * 2.) * factor), viewport.size.height))
                };
                let rendered = renderer.render_scene_to_image(&scene(tile, viewport, image, 20. * factor, viewport), size(DevicePixels((80. * factor) as i32), DevicePixels((60. * factor) as i32))).unwrap();
                for y in 0..rendered.height() {
                    for x in 0..rendered.width() {
                        let point = point(x as f32 + 0.5, y as f32 + 0.5);
                        if point.x < image.origin.x.0 + 1. || point.x > image.origin.x.0 + image.size.width.0 - 1.
                            || point.y < image.origin.y.0 + 1. || point.y > image.origin.y.0 + image.size.height.0 - 1. { continue; }
                        let distance = distance(point, viewport, 20. * factor);
                        if distance < -1. { assert!(rendered.get_pixel(x,y)[0] >= 250, "inside {factor}/{vertical}/{gap}/{x}/{y}"); }
                        if distance > 1. { assert!(rendered.get_pixel(x,y)[0] <= 2, "outside {factor}/{vertical}/{gap}/{x}/{y}"); }
                    }
                }
            }
        }
    }
}

/// 同原图与帧反复绘制复用纹理，半透明帧保留合成结果，移除每帧后图集无条目。
#[test]
fn rounded_clip_keeps_frames_alpha_and_atlas_identity() {
    let mut renderer = MetalHeadlessRenderer::new();
    let data = fixture();
    let viewport = Bounds::new(point(ScaledPixels(0.),ScaledPixels(0.)), size(ScaledPixels(80.),ScaledPixels(60.)));
    let image = Bounds::new(point(ScaledPixels(10.),ScaledPixels(0.)), size(ScaledPixels(60.),ScaledPixels(60.)));
    let first = tile(&renderer,&data,0);
    assert_eq!(first, tile(&renderer,&data,0));
    let legacy = renderer.render_scene_to_image(&scene(first,image,image,20.,viewport),size(DevicePixels(80),DevicePixels(60))).unwrap();
    let correct = renderer.render_scene_to_image(&scene(first,viewport,image,20.,viewport),size(DevicePixels(80),DevicePixels(60))).unwrap();
    assert!(correct.get_pixel(17,1)[0] >= 250);
    assert!(legacy.get_pixel(17,1)[0] <= 2);
    let mut mask = viewport; mask.size.width = ScaledPixels(40.);
    let clipped = renderer.render_scene_to_image(&scene(first,viewport,image,20.,mask),size(DevicePixels(80),DevicePixels(60))).unwrap();
    assert!(clipped.get_pixel(50,30)[0] <= 2);
    let alpha = tile(&renderer,&data,1);
    let rendered = renderer.render_scene_to_image(&scene(alpha,viewport,image,20.,viewport),size(DevicePixels(80),DevicePixels(60))).unwrap();
    assert!((126..=130).contains(&rendered.get_pixel(40,30)[0]));
    let green = tile(&renderer,&data,2);
    let rendered = renderer.render_scene_to_image(&scene(green,viewport,image,20.,viewport),size(DevicePixels(80),DevicePixels(60))).unwrap();
    assert!(rendered.get_pixel(40,30)[1] >= 250); assert!(rendered.get_pixel(40,30)[0] <= 2);
    for frame in 0..3 {
        let key = RenderImageParams {image_id:data.id,frame_index:frame}.into();
        renderer.sprite_atlas().remove(&key);
        let mut rebuilt = false;
        renderer.sprite_atlas().get_or_insert_with(key.clone(), &mut || {
            rebuilt = true;
            Ok(Some((data.size(frame), Cow::Borrowed(data.as_bytes(frame).unwrap()))))
        }).unwrap().unwrap();
        assert!(rebuilt, "removed frame must execute a new atlas upload");
        renderer.sprite_atlas().remove(&key);
    }
}
