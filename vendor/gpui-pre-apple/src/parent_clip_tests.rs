//! 真实离屏Metal读回：全部Apple图元、空scene、原viewport与路径后encoder恢复。
//! 不创建NSApplication、窗口、Services，也不操作用户应用；mono夹具是确定性atlas覆盖图。

use crate::metal_renderer::{MetalHeadlessRenderer, PathRasterizationVertex, SurfaceBounds};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeightOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use gpui::{
    AtlasKey, AtlasTile, Background, BorderStyle, Bounds, ContentMask, Corners, DevicePixels,
    Edges, FontId, GlyphId, MonochromeSprite, PaintSurface, ParentClipChain, ParentClipId,
    ParentClipNode, ParentRoundedClip, PathBuilder, PlatformHeadlessRenderer, PolychromeSprite,
    Quad, RenderGlyphParams, RenderImage, RenderImageParams, ScaledPixels, Scene, Shadow,
    TransformationMatrix, Underline, point, px, size,
};
use std::{
    borrow::Cow,
    mem::{offset_of, size_of},
    sync::Arc,
};

fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn child(f: f32) -> Bounds<ScaledPixels> {
    bounds(330. * f, 33. * f, 20. * f, 20. * f)
}
fn viewport(f: f32) -> Bounds<ScaledPixels> {
    bounds(24. * f, 37. * f, 322. * f, 160. * f)
}
fn chain(nested: bool) -> ParentClipChain {
    let first = ParentRoundedClip::new(
        Bounds::new(point(px(24.), px(37.)), size(px(322.), px(160.))),
        Corners {
            top_left: px(20.),
            top_right: px(20.),
            bottom_left: px(0.),
            bottom_right: px(0.),
        },
    );
    let mut chain = ParentClipChain::default().push(first);
    if nested {
        chain = chain.push(ParentRoundedClip::new(
            Bounds::new(point(px(20.), px(30.)), size(px(330.), px(180.))),
            Corners::all(px(30.)),
        ));
    }
    chain
}

/// 两种atlas走真实Mono/Poly pipeline，保护区消除线性采样的边缘污染。
fn atlas(renderer: &MetalHeadlessRenderer, mono: bool) -> AtlasTile {
    let (key, bytes) = if mono {
        (
            AtlasKey::Glyph(RenderGlyphParams {
                font_id: FontId(0),
                glyph_id: GlyphId(123),
                font_size: px(14.),
                subpixel_variant: point(0, 0),
                scale_factor: 1.,
                is_emoji: false,
                subpixel_rendering: false,
                dilation: 0,
            }),
            vec![255u8; 16],
        )
    } else {
        let data = Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255])),
        )]));
        (
            AtlasKey::Image(RenderImageParams {
                image_id: data.id,
                frame_index: 0,
            }),
            data.as_bytes(0).unwrap().to_vec(),
        )
    };
    let mut tile = renderer
        .sprite_atlas()
        .get_or_insert_with(key, &mut || {
            Ok(Some((
                size(DevicePixels(4), DevicePixels(4)),
                Cow::Borrowed(&bytes),
            )))
        })
        .unwrap()
        .unwrap();
    tile.bounds.origin = tile.bounds.origin + point(DevicePixels(1), DevicePixels(1));
    tile.bounds.size = size(DevicePixels(2), DevicePixels(2));
    tile
}

/// 真NV12/IOSurface是Metal的Surface实现要求；锁定后按各plane实际stride填充。
fn surface() -> CFRetained<CVPixelBuffer> {
    let empty = CFDictionary::<CFType, CFType>::empty();
    let keys: [&CFString; 2] = unsafe {
        [
            kCVPixelBufferIOSurfacePropertiesKey,
            kCVPixelBufferMetalCompatibilityKey,
        ]
    };
    let values: [&CFType; 2] = [&empty, CFBoolean::new(true)];
    let options = CFDictionary::from_slices(&keys, &values);
    let mut raw = std::ptr::null_mut();
    // SAFETY: 输出指针指向本地变量；成功时 CoreVideo 返回 +1 引用，由 CFRetained 接管。
    let buffer = unsafe {
        assert_eq!(
            CVPixelBufferCreate(
                None,
                4,
                4,
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                Some(options.as_opaque()),
                std::ptr::NonNull::from(&mut raw),
            ),
            kCVReturnSuccess
        );
        CFRetained::from_raw(std::ptr::NonNull::new(raw).unwrap())
    };
    // SAFETY: CVPixelBuffer已锁定；长度取自当前plane的实际height×bytes_per_row。
    unsafe {
        assert_eq!(
            CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            kCVReturnSuccess
        );
        for plane in 0..2 {
            let length = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane)
                * CVPixelBufferGetHeightOfPlane(&buffer, plane);
            let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane);
            assert!(!base.is_null());
            std::ptr::write_bytes(
                base.cast::<u8>(),
                if plane == 0 { 200 } else { 128 },
                length,
            );
        }
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            kCVReturnSuccess
        );
    }
    buffer
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    QuadFast,
    QuadBorder,
    Shadow,
    Underline,
    Mono,
    Poly,
    Path,
    Surface,
}
fn insert(scene: &mut Scene, renderer: &MetalHeadlessRenderer, kind: Kind, f: f32) {
    let b = child(f);
    let mask = ContentMask {
        bounds: viewport(f),
    };
    let white = gpui::white();
    match kind {
        Kind::QuadFast | Kind::QuadBorder => scene.insert_primitive(Quad {
            bounds: b,
            content_mask: mask,
            background: white.into(),
            border_color: white,
            border_widths: Edges::all(ScaledPixels(if matches!(kind, Kind::QuadBorder) {
                8. * f
            } else {
                0.
            })),
            border_style: BorderStyle::Solid,
            ..Default::default()
        }),
        Kind::Shadow => scene.insert_primitive(Shadow {
            order: 0,
            blur_radius: ScaledPixels(0.2 * f),
            bounds: b,
            corner_radii: Default::default(),
            content_mask: mask,
            color: white,
            element_bounds: b,
            element_corner_radii: Default::default(),
            inset: 0,
            pad: 0,
            parent_clip: Default::default(),
        }),
        Kind::Underline => scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            bounds: b,
            content_mask: mask,
            color: white,
            thickness: ScaledPixels(20. * f),
            wavy: false.into(),
            parent_clip: Default::default(),
        }),
        Kind::Mono => scene.insert_primitive(MonochromeSprite {
            order: 0,
            pad: 0,
            bounds: b,
            content_mask: mask,
            color: white,
            tile: atlas(renderer, true),
            transformation: TransformationMatrix::default(),
            parent_clip: Default::default(),
        }),
        Kind::Poly => scene.insert_primitive(PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.,
            bounds: b,
            content_mask: mask,
            corner_radii: Default::default(),
            tile: atlas(renderer, false),
            rounded_clip_bounds: b,
            parent_clip: Default::default(),
        }),
        Kind::Path => {
            let mut builder = PathBuilder::fill();
            builder.move_to(point(px(330.), px(33.)));
            builder.line_to(point(px(350.), px(33.)));
            builder.line_to(point(px(350.), px(53.)));
            builder.line_to(point(px(330.), px(53.)));
            builder.close();
            let mut path = builder.build().unwrap().scale(f);
            path.color = white.into();
            path.content_mask = mask;
            scene.insert_primitive(path);
        }
        Kind::Surface => scene.insert_primitive(PaintSurface {
            order: 0,
            bounds: b,
            content_mask: mask,
            image_buffer: surface(),
            parent_clip: Default::default(),
        }),
    }
}
/// 独立renderer测试显式给最终typed vectors赋head；生产Window→Scene赋值由core真实测试负责。
fn set_head(scene: &mut Scene, id: ParentClipId) {
    for q in &mut scene.quads {
        q.parent_clip = id;
    }
    for s in &mut scene.shadows {
        s.parent_clip = id;
    }
    for u in &mut scene.underlines {
        u.parent_clip = id;
    }
    for s in &mut scene.monochrome_sprites {
        s.parent_clip = id;
    }
    for s in &mut scene.polychrome_sprites {
        s.parent_clip = id;
    }
    for p in &mut scene.paths {
        p.parent_clip = id;
    }
    for s in &mut scene.surfaces {
        s.parent_clip = id;
    }
}
fn render(renderer: &mut MetalHeadlessRenderer, mut scene: Scene, f: f32) -> image::RgbaImage {
    scene.finish();
    crate::metal_test_support::render_scene(renderer, &scene, (400., 240.), f)
}
fn sample(img: &image::RgbaImage, x: f32, y: f32, f: f32) -> u8 {
    img.get_pixel((x * f) as u32, (y * f) as u32)[0]
}

/// 同一生成C头和Rust布局分别检查；默认关闭的旧ABI另由非feature编译核对。
#[test]
fn parent_clip_metal_rust_and_cbindgen_layout_agree() {
    assert_eq!(size_of::<ParentClipId>(), 16);
    assert_eq!(size_of::<ParentClipNode>(), 48);
    assert_eq!(offset_of!(ParentClipNode, parent), 32);
    assert_eq!(offset_of!(ParentClipNode, padding), 36);
    assert_eq!(size_of::<ContentMask<ScaledPixels>>(), 16);
    assert_eq!(size_of::<Quad>(), 176);
    assert_eq!(offset_of!(Quad, parent_clip), 160);
    assert_eq!(size_of::<Shadow>(), 128);
    assert_eq!(size_of::<Underline>(), 80);
    assert_eq!(size_of::<MonochromeSprite>(), 128);
    assert_eq!(size_of::<PolychromeSprite>(), 128);
    assert_eq!(size_of::<PathRasterizationVertex>(), 120);
    assert_eq!(offset_of!(PathRasterizationVertex, parent_clip), 104);
    assert_eq!(size_of::<SurfaceBounds>(), 48);
    assert_eq!(offset_of!(SurfaceBounds, parent_clip), 32);
}

/// 每种实际Apple fragment都先确认未裁版本可见，再证明父角外与实际opaque黑底清屏一致、角内仍正常。
#[test]
fn parent_clip_metal_all_primitive_fragment_pixels() {
    let mut renderer = MetalHeadlessRenderer::new();
    for f in [1., 2.] {
        let clear = render(&mut renderer, Scene::default(), f);
        for nested in [false, true] {
            for kind in [
                Kind::QuadFast,
                Kind::QuadBorder,
                Kind::Shadow,
                Kind::Underline,
                Kind::Mono,
                Kind::Poly,
                Kind::Path,
                Kind::Surface,
            ] {
                let mut original = Scene::default();
                insert(&mut original, &renderer, kind, f);
                let original = render(&mut renderer, original, f);
                assert!(
                    sample(&original, 345., 38., f) > 100,
                    "baseline outside must draw {kind:?}/{f}/{nested}"
                );
                let mut clipped = Scene::default();
                let id = clipped.intern_parent_clip_chain(&chain(nested), f);
                insert(&mut clipped, &renderer, kind, f);
                set_head(&mut clipped, id);
                let clipped = render(&mut renderer, clipped, f);
                assert!(
                    clipped.get_pixel((345. * f) as u32, (38. * f) as u32)
                        == clear.get_pixel((345. * f) as u32, (38. * f) as u32),
                    "parent outside {kind:?}/{f}/{nested}"
                );
                let inside = ((336. * f) as u32, (45. * f) as u32);
                assert!(
                    sample(&original, 336., 45., f) > 100,
                    "baseline inside must draw {kind:?}/{f}/{nested}"
                );
                assert_eq!(
                    clipped.get_pixel(inside.0, inside.1),
                    original.get_pixel(inside.0, inside.1),
                    "parent inside RGBA must be unchanged {kind:?}/{f}/{nested}"
                );
            }
        }
    }
}

/// Path中间pass后主encoder被重建，随后的Quad须重新绑定节点buffer。
#[test]
fn parent_clip_metal_path_pass_rebinds_following_primitive() {
    let mut renderer = MetalHeadlessRenderer::new();
    let mut scene = Scene::default();
    let id = scene.intern_parent_clip_chain(&chain(false), 1.);
    insert(&mut scene, &renderer, Kind::Path, 1.);
    insert(&mut scene, &renderer, Kind::QuadFast, 1.);
    scene.quads[0].background = Background::from(gpui::red());
    set_head(&mut scene, id);
    let image = render(&mut renderer, scene, 1.);
    assert!(sample(&image, 345., 38., 1.) <= 2);
    let inside = image.get_pixel(336, 45);
    assert!(
        inside[0] > 250 && inside[1] < 2 && inside[2] < 2,
        "following Quad must remain red: {inside:?}"
    );
}

/// 空场景和非法零面积clip都绑定有效节点，非法clip绝不成为head0无裁切。
#[test]
fn parent_clip_metal_empty_scene_and_invalid_clip_fail_closed() {
    let mut renderer = MetalHeadlessRenderer::new();
    let empty = render(&mut renderer, Scene::default(), 1.);
    assert!(empty.pixels().all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
    let mut scene = Scene::default();
    let id = scene.intern_parent_clip_chain(
        &ParentClipChain::default().push(ParentRoundedClip::default()),
        1.,
    );
    assert_ne!(id.head, 0);
    insert(&mut scene, &renderer, Kind::QuadFast, 1.);
    set_head(&mut scene, id);
    let image = render(&mut renderer, scene, 1.);
    assert_eq!(image, empty, "非零空clip的实际RGBA必须完全等于真实空Scene");
}

/// 外祖先与head各有独立必要性，仅读最近节点或仅祖先都必须失败。
#[test]
fn parent_clip_metal_traverses_ancestors_and_intersects_head() {
    let mut renderer = MetalHeadlessRenderer::new();
    for f in [1., 2.] {
        let clear = render(&mut renderer, Scene::default(), f);
        let original = chain(false);
        let broad = ParentRoundedClip::new(
            Bounds::new(point(px(0.), px(0.)), size(px(400.), px(240.))),
            Corners::default(),
        );
        // head接受(345,38)，只有原始祖先拒绝，验证GPU真实遍历parent。
        let ancestor_required = original.push(broad);
        assert!(broad.contains(&point(px(345.5), px(38.5))));
        let mut scene = Scene::default();
        let id = scene.intern_parent_clip_chain(&ancestor_required, f);
        insert(&mut scene, &renderer, Kind::QuadFast, f);
        set_head(&mut scene, id);
        let image = render(&mut renderer, scene, f);
        assert_eq!(
            image.get_pixel((345. * f) as u32, (38. * f) as u32),
            clear.get_pixel((345. * f) as u32, (38. * f) as u32)
        );
        // (340,45)在祖先20px角内，在head30px角外，验证取交集而非只用祖先。
        let tight = ParentRoundedClip::new(original.as_slice()[0].bounds, Corners::all(px(30.)));
        assert!(original.contains(&point(px(340.5), px(45.5))));
        assert!(!tight.contains(&point(px(340.5), px(45.5))));
        let mut scene = Scene::default();
        let id = scene.intern_parent_clip_chain(&original.push(tight), f);
        insert(&mut scene, &renderer, Kind::QuadFast, f);
        set_head(&mut scene, id);
        let image = render(&mut renderer, scene, f);
        assert_eq!(
            image.get_pixel((340. * f) as u32, (45. * f) as u32),
            clear.get_pixel((340. * f) as u32, (45. * f) as u32)
        );
        assert!(
            sample(&image, 332., 50., f) > 100,
            "nested交集仍有实际可见部分"
        );
    }
}

/// 重复同一clip不能让真实AA边缘alpha平方；fixture必须存在非二值边缘。
#[test]
fn parent_clip_metal_duplicate_ancestors_do_not_square_edge_coverage() {
    let mut renderer = MetalHeadlessRenderer::new();
    let one = chain(false);
    let two = one.push(one.as_slice()[0]);
    let mut images = Vec::new();
    for clips in [&one, &two] {
        let mut scene = Scene::default();
        let id = scene.intern_parent_clip_chain(clips, 1.);
        insert(&mut scene, &renderer, Kind::QuadFast, 1.);
        set_head(&mut scene, id);
        images.push(render(&mut renderer, scene, 1.));
    }
    assert!(
        images[0].pixels().any(|p| p[0] > 0 && p[0] < 255),
        "必须真实绘出抗锯齿边缘"
    );
    assert_eq!(images[0], images[1], "max距离合并不能重复乘coverage");
}

/// 背景保持原有无clip绘制；父角外必须透出真实红底，不能通过黑色遮罩伪造。
#[test]
fn parent_clip_metal_preserves_underlying_scene_color() {
    let mut renderer = MetalHeadlessRenderer::new();
    let make_background = || {
        let mut scene = Scene::default();
        scene.insert_primitive(Quad {
            bounds: bounds(0., 0., 400., 240.),
            content_mask: ContentMask {
                bounds: bounds(0., 0., 400., 240.),
            },
            background: gpui::red().into(),
            ..Default::default()
        });
        scene
    };
    let background = render(&mut renderer, make_background(), 1.);
    let mut scene = make_background();
    let id = scene.intern_parent_clip_chain(&chain(false), 1.);
    insert(&mut scene, &renderer, Kind::QuadFast, 1.);
    scene.quads.last_mut().unwrap().parent_clip = id;
    let image = render(&mut renderer, scene, 1.);
    assert_eq!(image.get_pixel(345, 38), background.get_pixel(345, 38));
    assert!(image.get_pixel(336, 45)[1] > 250, "角内保留白色子图元");
}
