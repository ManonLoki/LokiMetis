//! 真实Style→CPU chain→Scene节点→Metal Quad/Path像素；只覆盖均匀border几何增量。
//! 不构造App/NSApplication/原生窗口，不修改冻结包的shader或ABI。
use crate::metal_renderer::MetalHeadlessRenderer;
use gpui::{
    Bounds, ContentMask, Corners, Edges, Hsla, Overflow, ParentClipChain,
    ParentRoundedClip, PathBuilder, Pixels, Quad, Scene, Style, point,
    px, size,
};

fn style_clip(radii: [f32; 4], height: f32, color: Option<Hsla>) -> ParentRoundedClip {
    Style {
        clip_rounded_children: true,
        overflow: point(Overflow::Hidden, Overflow::Hidden),
        border_widths: Edges::all(px(10.).into()),
        border_color: color,
        corner_radii: Corners {
            top_left: px(radii[0]).into(),
            top_right: px(radii[1]).into(),
            bottom_right: px(radii[2]).into(),
            bottom_left: px(radii[3]).into(),
        },
        ..Style::default()
    }
    .parent_rounded_clip(
        Bounds::new(point(px(112.), px(37.)), size(px(100.), px(height))),
        px(16.),
    )
    .expect("opt-in hidden均匀border夹具")
}

fn scene(clip: Option<ParentRoundedClip>, factor: f32, path: bool) -> Scene {
    let mut scene = Scene::default();
    // 后代覆盖整个外框，保证透明border和旧角见证点在baseline中实际可见。
    let b: Bounds<Pixels> = Bounds::new(point(px(112.), px(37.)), size(px(100.), px(100.)));
    let mask = ContentMask {
        bounds: b.scale(factor),
    };
    if path {
        let mut builder = PathBuilder::fill();
        builder.move_to(point(px(112.), px(37.)));
        builder.line_to(point(px(212.), px(37.)));
        builder.line_to(point(px(212.), px(137.)));
        builder.line_to(point(px(112.), px(137.)));
        builder.close();
        let mut path = builder.build().unwrap().scale(factor);
        path.color = gpui::white().into();
        path.content_mask = mask;
        scene.insert_primitive(path);
    } else {
        scene.insert_primitive(Quad {
            bounds: b.scale(factor),
            content_mask: mask,
            background: gpui::white().into(),
            ..Default::default()
        });
    }
    if let Some(clip) = clip {
        let id = scene.intern_parent_clip_chain(&ParentClipChain::default().push(clip), factor);
        assert_ne!(id.head, 0);
        // 独立renderer测试typed vectors显式赋head；正式Window赋值已由冻结包真实测试证明。
        for quad in &mut scene.quads {
            quad.parent_clip = id;
        }
        for path in &mut scene.paths {
            path.parent_clip = id;
        }
    }
    scene.finish();
    scene
}

fn render(renderer: &mut MetalHeadlessRenderer, scene: &Scene, factor: f32) -> image::RgbaImage {
    crate::metal_test_support::render_scene(renderer, scene, (240., 160.), factor)
}

/// outside/inside是逻辑像素左上角；按实际factor取对应设备像素中心，同时检查CPU判定。
fn verify_pixels(clip: ParentRoundedClip, outside: (f32, f32), inside: (f32, f32)) {
    let mut renderer = MetalHeadlessRenderer::new();
    for factor in [1., 2.] {
        let clear = render(&mut renderer, &Scene::default(), factor);
        for path in [false, true] {
            let baseline = render(&mut renderer, &scene(None, factor, path), factor);
            let image = render(&mut renderer, &scene(Some(clip), factor, path), factor);
            let coord = |(x, y): (f32, f32)| ((x * factor) as u32, (y * factor) as u32);
            let (x, y) = coord(outside);
            assert!(
                baseline.get_pixel(x, y)[0] > 250,
                "outside baseline不可空 {factor}/{path}"
            );
            assert_eq!(
                image.get_pixel(x, y),
                clear.get_pixel(x, y),
                "外部像素必须完全被裁掉 {factor}/{path}"
            );
            let logical = point(px((x as f32 + 0.5) / factor), px((y as f32 + 0.5) / factor));
            assert!(
                !clip.contains(&logical),
                "真实CPU命中必须与离开AA带的Metal像素一致"
            );
            let (x, y) = coord(inside);
            assert!(
                baseline.get_pixel(x, y)[0] > 250,
                "inside baseline不可空 {factor}/{path}"
            );
            assert_eq!(
                image.get_pixel(x, y),
                baseline.get_pixel(x, y),
                "内部RGBA不得被改变 {factor}/{path}"
            );
            let logical = point(px((x as f32 + 0.5) / factor), px((y as f32 + 0.5) / factor));
            assert!(clip.contains(&logical));
        }
    }
}

#[test]
fn parent_clip_geometry_metal_transparent_border_quad_and_path() {
    for color in [None, Some(gpui::hsla(0., 0., 0., 0.)), Some(gpui::black())] {
        verify_pixels(style_clip([20.; 4], 80., color), (117., 77.), (162., 77.));
    }
}

#[test]
fn parent_clip_geometry_metal_outer_overlap_precedes_uniform_border() {
    // 左上内角50；安全见证local(14.5,13.5)新距离+0.917、旧−0.266。
    verify_pixels(
        style_clip([120., 80., 60., 40.], 80., Some(gpui::black())),
        (136., 60.),
        (172., 87.),
    );
    // 外右上used8<border10后变直角；此旧错误圆角像素必须完整保留。
    verify_pixels(
        style_clip([140., 20., 80., 60.], 80., Some(gpui::black())),
        (138., 56.),
        (201., 47.),
    );
}

#[test]
fn parent_clip_geometry_metal_used_inner_radius_is_not_renormalized() {
    // 90px同心内角落在80×80内缘；新距离+2.057、旧80px角-2.061，远离AA带。
    verify_pixels(
        style_clip([100., 0., 0., 0.], 100., None),
        (150., 68.),
        (172., 97.),
    );
}
