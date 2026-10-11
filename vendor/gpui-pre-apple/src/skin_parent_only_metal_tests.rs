//! Skin图片只让父R19裁切；真实Quad作为一次coverage oracle，ownR19错误接线必须失败。
use crate::metal_renderer::MetalHeadlessRenderer;
use gpui::{
    AtlasKey, Bounds, ContentMask, Corners, DevicePixels, ParentClipChain, ParentRoundedClip,
    PlatformHeadlessRenderer, PolychromeSprite, Quad, RenderImage, RenderImageParams, Scene, point,
    px, size,
};
use std::{borrow::Cow, sync::Arc};

fn image_scene(renderer: &MetalHeadlessRenderer, factor: f32, own_radius: f32) -> Scene {
    let mut scene = Scene::default();
    let logical = Bounds::new(point(px(20.), px(20.)), size(px(100.), px(60.)));
    let radii = Corners {
        top_left: px(19.),
        top_right: px(19.),
        bottom_left: px(0.),
        bottom_right: px(0.),
    };
    let id = scene.intern_parent_clip_chain(
        &ParentClipChain::default().push(ParentRoundedClip::new(logical, radii)),
        factor,
    );
    let data = Arc::new(RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255])),
    )]));
    let bytes = data.as_bytes(0).unwrap();
    let mut tile = renderer
        .sprite_atlas()
        .get_or_insert_with(
            AtlasKey::Image(RenderImageParams {
                image_id: data.id,
                frame_index: 0,
            }),
            &mut || {
                Ok(Some((
                    size(DevicePixels(4), DevicePixels(4)),
                    Cow::Borrowed(bytes),
                )))
            },
        )
        .unwrap()
        .unwrap();
    // 2×2纯白内纹理四周保留实像素，排除采样到图集透明边缘造成的第二种coverage。
    tile.bounds.origin = tile.bounds.origin + point(DevicePixels(1), DevicePixels(1));
    tile.bounds.size = size(DevicePixels(2), DevicePixels(2));
    let b = logical.scale(factor);
    scene.insert_primitive(PolychromeSprite {
        order: 0,
        pad: 0,
        grayscale: false.into(),
        opacity: 1.,
        bounds: b,
        content_mask: ContentMask { bounds: b },
        corner_radii: Corners {
            top_left: px(own_radius).scale(factor),
            top_right: px(own_radius).scale(factor),
            bottom_left: px(0.).scale(factor),
            bottom_right: px(0.).scale(factor),
        },
        tile,
        rounded_clip_bounds: b,
        parent_clip: Default::default(),
    });
    scene.polychrome_sprites[0].parent_clip = id;
    scene.finish();
    scene
}

fn quad_scene(factor: f32) -> Scene {
    let mut scene = Scene::default();
    let logical = Bounds::new(point(px(20.), px(20.)), size(px(100.), px(60.)));
    let radii = Corners {
        top_left: px(19.),
        top_right: px(19.),
        bottom_left: px(0.),
        bottom_right: px(0.),
    };
    let id = scene.intern_parent_clip_chain(
        &ParentClipChain::default().push(ParentRoundedClip::new(logical, radii)),
        factor,
    );
    let b = logical.scale(factor);
    scene.insert_primitive(Quad {
        bounds: b,
        content_mask: ContentMask { bounds: b },
        background: gpui::white().into(),
        ..Default::default()
    });
    scene.quads[0].parent_clip = id;
    scene.finish();
    scene
}

fn render(renderer: &mut MetalHeadlessRenderer, scene: &Scene, factor: f32) -> image::RgbaImage {
    crate::metal_test_support::render_scene(renderer, scene, (140., 100.), factor)
}

#[test]
fn skin_parent_only_metal_r19_image_matches_single_coverage_and_rejects_duplicate_own_mask() {
    let mutant = std::env::var_os("LOKI_PARENT_CLIP_VERIFY_DUPLICATE_OWN").is_some();
    let mut renderer = MetalHeadlessRenderer::new();
    for factor in [1., 2.] {
        let oracle = render(&mut renderer, &quad_scene(factor), factor);
        // 单独mutant执行把实际own半径改19，完整single-coverage oracle不变，必须真实RED。
        let selected = image_scene(&renderer, factor, if mutant { 19. } else { 0. });
        let image = render(&mut renderer, &selected, factor);
        let duplicate = image_scene(&renderer, factor, 19.);
        let duplicate = render(&mut renderer, &duplicate, factor);
        let mut partial = 0;
        let mut duplicate_difference = 0;
        // 真圆弧AA区离矩形边界至少1px；own0自身矩形coverage为1。
        for y in (21. * factor) as u32..(39. * factor) as u32 {
            for x in (21. * factor) as u32..(39. * factor) as u32 {
                let expected = oracle.get_pixel(x, y);
                assert_eq!(
                    image.get_pixel(x, y),
                    expected,
                    "Skin父R19须恰好一次coverage {factor}/{x}/{y}; ownR19错误接线必须杀死此断言"
                );
                if expected[0] > 0 && expected[0] < 255 {
                    partial += 1;
                    if duplicate.get_pixel(x, y) != expected {
                        duplicate_difference += 1;
                    }
                }
            }
        }
        assert!(partial > 0, "必须实际存在非二值AA像素");
        assert_eq!(
            duplicate_difference, partial,
            "错误ownR19必须改变每个实际AA像素"
        );
    }
}
