#![cfg(target_os = "macos")]
//! 直接调用真实Style父裁切helper；均匀border的常量oracle来自外圆角CSS归一后的几何。
//! 不构造App/Window/原生宿主，不代理或重写被测逻辑。
use gpui_kit::{
    Bounds, Corners, Edges, Hsla, Overflow, ParentRoundedClip, Pixels, Style, hsla, point, px, size,
};

/// 受测父容器的固定外边界。
fn outer_bounds() -> Bounds<Pixels> {
    Bounds::new(point(px(112.), px(37.)), size(px(100.), px(80.)))
}

/// 构造双轴 hidden、显式启用父圆角裁切的样式，圆角、边框宽度与颜色由参数给定。
fn style(radii: [f32; 4], border: f32, color: Option<Hsla>) -> Style {
    Style {
        clip_rounded_children: true,
        overflow: point(Overflow::Hidden, Overflow::Hidden),
        border_widths: Edges::all(px(border).into()),
        border_color: color,
        corner_radii: Corners {
            top_left: px(radii[0]).into(),
            top_right: px(radii[1]).into(),
            bottom_right: px(radii[2]).into(),
            bottom_left: px(radii[3]).into(),
        },
        ..Style::default()
    }
}

/// 由样式计算父圆角裁切；测试样式必定产生裁切。
fn clip(style: Style) -> ParentRoundedClip {
    style
        .parent_rounded_clip(outer_bounds(), px(16.))
        .expect("明确opt-in且双轴hidden必须生成真实父裁切")
}

/// 断言裁切边界正好是外边界扣除 10px 边框后的 padding-box。
fn assert_inner_bounds(clip: &ParentRoundedClip) {
    assert_eq!(
        clip.bounds,
        Bounds::new(point(px(122.), px(47.)), size(px(80.), px(60.)))
    );
}

/// Alpha0、没有border_color与opaque都占用同样10px布局边框，不能由颜色决定padding-box。
#[test]
fn parent_clip_geometry_transparent_uniform_border_uses_padding_box() {
    for color in [Some(hsla(0., 0., 0., 0.)), None, Some(gpui_kit::black())] {
        let clip = clip(style([20.; 4], 10., color));
        assert_inner_bounds(&clip);
        assert_eq!(clip.corner_radii, Corners::all(px(10.)));
        assert!(
            !clip.contains(&point(px(117.), px(77.))),
            "透明border布局区仍不能命中后代"
        );
        assert!(clip.contains(&point(px(162.), px(77.))));
    }
}

/// 100×80上[120,80,60,40]先统一乘0.5，再减10，used inner为[50,30,20,10]。
/// 原先先减后归一会得到[47.142857,30,21.428572,12.857143]并错误放行此见证点。
#[test]
fn parent_clip_geometry_normalizes_oversized_asymmetric_outer_radii_before_border() {
    let clip = clip(style([120., 80., 60., 40.], 10., Some(gpui_kit::black())));
    assert_inner_bounds(&clip);
    assert!(
        !clip.contains(&point(px(133.), px(65.))),
        "50px左上角必须拒绝旧逻辑放行的实际几何点"
    );
    assert!(clip.contains(&point(px(137.), px(67.))));
    assert_eq!(
        clip.corner_radii,
        Corners {
            top_left: px(50.),
            top_right: px(30.),
            bottom_right: px(20.),
            bottom_left: px(10.),
        }
    );
}

/// 外[140,20,80,60]统一乘0.4为[56,8,32,24]，减10后[46,0,22,14]。
/// 外右上used8小于border10，内右上为直角；不能保留旧归一顺序生成的3.333圆角。
#[test]
fn parent_clip_geometry_clamps_small_used_outer_corner_after_normalization() {
    let clip = clip(style([140., 20., 80., 60.], 10., Some(gpui_kit::black())));
    assert_inner_bounds(&clip);
    assert!(
        clip.contains(&point(px(201.5), px(47.5))),
        "内右上直角必须允许旧逻辑拒绝的真实像素中心"
    );
    assert_eq!(
        clip.corner_radii,
        Corners {
            top_left: px(46.),
            top_right: px(0.),
            bottom_right: px(22.),
            bottom_left: px(14.),
        }
    );
}

/// 外100×100单角100在减10px均匀边框后为90；内80×80不能再次把它压成80。
#[test]
fn parent_clip_geometry_used_inner_radius_survives_chain_scale_and_scene_intern() {
    use gpui_kit::{ParentClipChain, ScaledPixels, Scene};
    let clip = style([100., 0., 0., 0.], 10., None)
        .parent_rounded_clip(
            Bounds::new(point(px(112.), px(37.)), size(px(100.), px(100.))),
            px(16.),
        )
        .expect("均匀透明border也必须产生内缘");
    assert_eq!(
        clip.bounds,
        Bounds::new(point(px(122.), px(47.)), size(px(80.), px(80.)))
    );
    assert_eq!(clip.corner_radii.top_left, px(90.));
    let outside = point(px(150.5), px(68.5));
    let inside = point(px(172.5), px(97.5));
    assert!(
        !clip.contains(&outside),
        "同心90px圆角必须拒绝旧80px内角放行点"
    );
    assert!(clip.contains(&inside));
    let chain = ParentClipChain::default().push(clip);
    assert_eq!(chain.as_slice()[0], clip);
    assert!(!chain.contains(&outside));
    assert!(chain.contains(&inside));
    for factor in [1., 2.] {
        let node = clip.scale(factor);
        assert_eq!(node.corner_radii.top_left, ScaledPixels(90. * factor));
        assert_eq!(node.bounds, clip.bounds.scale(factor));
        let mut scene = Scene::default();
        let id = scene.intern_parent_clip_chain(&chain, factor);
        assert_ne!(id.head, 0);
        assert_eq!(scene.parent_clip_nodes[id.head as usize], node);
        assert_eq!(scene.intern_parent_clip_chain(&chain, factor), id);
        assert_eq!(
            scene.parent_clip_nodes.len(),
            2,
            "完整used几何必须被稳定去重"
        );
    }
    // 外角公开constructor仍执行CSS overlap；只有内部已used内缘走sanitize-only。
    let outer = ParentRoundedClip::new(clip.bounds, clip.corner_radii);
    assert_eq!(outer.corner_radii.top_left, px(80.));
}

/// sanitize-only不能丢失既有fail-closed合同，也不能让NaN/负数半径进入GPU节点。
#[test]
fn parent_clip_geometry_used_chain_sanitizes_invalid_geometry_without_new_overlap() {
    use gpui_kit::ParentClipChain;
    let invalid = ParentRoundedClip {
        bounds: Bounds::new(point(px(f32::NAN), px(0.)), size(px(80.), px(80.))),
        corner_radii: Corners::all(px(90.)),
    };
    let chain = ParentClipChain::default().push(invalid);
    assert!(!chain.contains(&point(px(0.), px(0.))));
    assert_eq!(chain.as_slice()[0], ParentRoundedClip::default());
    let invalid_radii = ParentRoundedClip {
        bounds: Bounds::new(point(px(0.), px(0.)), size(px(80.), px(80.))),
        corner_radii: Corners {
            top_left: px(90.),
            top_right: px(f32::NAN),
            bottom_right: px(-2.),
            bottom_left: px(f32::INFINITY),
        },
    };
    let sanitized = ParentClipChain::default().push(invalid_radii);
    assert_eq!(
        sanitized.as_slice()[0].corner_radii,
        Corners {
            top_left: px(90.),
            top_right: px(0.),
            bottom_right: px(0.),
            bottom_left: px(0.),
        }
    );
    assert_eq!(
        sanitized.as_slice()[0].scale(2.).corner_radii.top_left.0,
        180.
    );
}
