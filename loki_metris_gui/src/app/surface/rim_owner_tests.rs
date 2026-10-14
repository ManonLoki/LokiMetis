//! 真实材质 owner 验证描边几何复用、精确失效和消失路径的及时释放。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, Hsla, IntoElement, Render, RenderImage, TestAppContext,
    WindowOptions, WindowVisibility, canvas, prelude::*, rgb, test::TestWindowExt,
};
use std::{sync::Arc, time::Duration};

/// 同一 owner 跨主题、颜色与几何更新，绘制闭包只持有弱引用。
struct Probe {
    owner: Option<Entity<MaterialState>>,
    dark: bool,
    width: Pixels,
    height: Pixels,
    left: Pixels,
    radius: Pixels,
    top: Pixels,
    accent: Option<Hsla>,
    paint_subpixel: bool,
}

impl Render for Probe {
    /// 不启动业务服务，直接执行生产材质及真实测试图集路径。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.as_ref().map(Entity::downgrade);
        let (dark, radius, top_width, top_accent) = (self.dark, self.radius, self.top, self.accent);
        let paint_subpixel = self.paint_subpixel;
        gpui_kit::div().pl(self.left).child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
            if let Some(owner) = &owner {
                let bounds = if paint_subpixel {
                    Bounds::new(bounds.origin, gpui_kit::size(px(0.1), bounds.size.height))
                } else { bounds };
                owner.update(cx, |state, cx| state.paint(bounds, dark,
                    Geometry { radius, top_width, top_accent }, MaterialKind::Spotlight, window, cx)).unwrap();
            }
        }).w(self.width).h(self.height))
    }
}

/// 初始卡片具有 3px 顶 accent，且无 hover 光斑或磁盘/后台服务。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        let owner = cx.new(|cx| {
            let mut state = MaterialState::new(window, cx);
            state.visible = true;
            state
        });
        cx.new(|_| Probe { owner: Some(owner), dark: false, width: px(320.), height: px(180.),
            left: px(0.), radius: px(20.), top: px(3.), accent: Some(rgb(0xd7952c).into()), paint_subpixel: false })
    }).unwrap())
}

/// 读取实际准备次数和当前纹理，不以图像内容相等推断缓存命中。
fn audit(probe: &Entity<Probe>, cx: &gpui_kit::App) -> ((usize, bool), Option<Arc<RenderImage>>) {
    let owner = probe.read(cx).owner.as_ref().unwrap().read(cx);
    (owner.rim.geometry_audit(), owner.rim.image())
}

/// 主题动画只重着色；平移不重建，几何原始位值各自决定失效。
#[gpui_kit::test]
fn theme_recolors_reuse_geometry_and_raw_geometry_bits_invalidate(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let mut previous = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (geometry, image) = audit(&probe, cx);
        assert_eq!(geometry, (1, true));
        let first = image.unwrap().id;
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).1.unwrap().id, first);
        probe.update(cx, |probe, cx| { probe.dark = true; cx.notify(); });
        window.render_frame(cx);
        first
    }).unwrap();
    for millis in [80, 70, 149] {
        cx.executor().advance_clock(Duration::from_millis(millis));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let (geometry, image) = audit(&probe, cx);
            assert_eq!(geometry, (1, true));
            let image = image.unwrap();
            assert_ne!(image.id, previous);
            previous = image.id;
        }).unwrap();
    }
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        previous = audit(&probe, cx).1.unwrap().id;
        probe.update(cx, |probe, cx| { probe.left = px(9.); cx.notify(); });
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).0, (1, true));
        assert_eq!(audit(&probe, cx).1.unwrap().id, previous);
        for expected in 2..=5 {
            probe.update(cx, |probe, cx| {
                match expected {
                    2 => probe.width += px(1.), 3 => probe.height += px(1.),
                    4 => probe.radius = px(12.), _ => probe.top = px(1.),
                }
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(audit(&probe, cx).0, (expected, true));
        }
    }).unwrap();
    cx.simulate_window_scale_factor_change(handle, 1.3);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).0, (6, true));
    }).unwrap();
    let next_scale = f32::from_bits(1.3_f32.to_bits() + 1);
    for size in [321_f32, 181.] { assert_eq!((size * 1.3).round(), (size * next_scale).round()); }
    assert_eq!((12_f32 * 1.3 + 1.).ceil(), (12. * next_scale + 1.).ceil());
    cx.simulate_window_scale_factor_change(handle, next_scale);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).0, (7, true));
        for expected in 8..=13 {
            probe.update(cx, |probe, cx| {
                match expected {
                    8 => probe.radius = px(f32::from_bits(12_f32.to_bits() + 1)),
                    9 => probe.top = px(f32::from_bits(1_f32.to_bits() + 1)),
                    10 => probe.top = px(0.), 11 => probe.top = px(-0.),
                    12 => probe.radius = px(0.), _ => probe.radius = px(-0.),
                }
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(audit(&probe, cx).0, (expected, true));
        }
    }).unwrap();
}

/// accent 消失只退休描边；逻辑零尺寸及隐藏同步清空全部当前与退休材质。
#[gpui_kit::test]
fn accent_removal_zero_dimensions_and_hiding_release_geometry(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let first = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let first = audit(&probe, cx).1.unwrap();
        assert!(window.has_image_atlas_entry(&first));
        probe.update(cx, |probe, cx| { probe.accent = None; cx.notify(); });
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).0, (1, false));
        assert!(audit(&probe, cx).1.is_none());
        first
    }).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(!window.has_image_atlas_entry(&first));
        probe.update(cx, |probe, cx| { probe.accent = Some(rgb(0xd7952c).into()); cx.notify(); });
        window.render_frame(cx);
        assert_eq!(audit(&probe, cx).0, (2, true));
        for (zero_width, expected) in [(true, 3), (false, 4)] {
            let images = probe.read(cx).owner.as_ref().unwrap().read(cx).audit().1;
            assert!(images.len() >= 2);
            probe.update(cx, |probe, cx| {
                if zero_width { probe.width = px(0.); } else { probe.height = px(0.); }
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(audit(&probe, cx).0, (expected - 1, false));
            let owner = probe.read(cx).owner.as_ref().unwrap().read(cx);
            assert!(owner.audit().1.is_empty());
            assert!(!owner.shadow.geometry_audit().1);
            assert!(images.iter().all(|image| !window.has_image_atlas_entry(image)));
            probe.update(cx, |probe, cx| { probe.width = px(320.); probe.height = px(180.); cx.notify(); });
            window.render_frame(cx);
            assert_eq!(audit(&probe, cx).0, (expected, true));
        }
    }).unwrap();
    let images = cx.update(|cx| probe.read(cx).owner.as_ref().unwrap().read(cx).audit().1);
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(audit(&probe, cx).0, (4, false));
        assert!(images.iter().all(|image| !window.has_image_atlas_entry(image)));
    }).unwrap();
}

/// owner 卸载后不留下几何或图集，退休回调的弱引用不能保活实体。
#[gpui_kit::test]
fn owner_release_drops_current_and_retired_rim_images(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    let (images, weak) = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let mut images = vec![audit(&probe, cx).1.unwrap()];
        probe.update(cx, |probe, cx| { probe.top = px(4.); cx.notify(); });
        window.render_frame(cx);
        images.push(audit(&probe, cx).1.unwrap());
        assert_ne!(images[0].id, images[1].id);
        let weak = probe.read(cx).owner.as_ref().unwrap().downgrade();
        probe.update(cx, |probe, cx| { probe.owner = None; cx.notify(); });
        window.render_frame(cx);
        (images, weak)
    }).unwrap();
    cx.update_window(handle, |_, window, _cx| {
        assert!(weak.upgrade().is_none());
        assert!(images.iter().all(|image| !window.has_image_atlas_entry(image)));
    }).unwrap();
}

/// 非空逻辑宽度吸附为物理零后，同步释放 hover 光斑及所有退休图像。
#[gpui_kit::test]
fn pixel_snapped_empty_bounds_release_current_and_retired_materials(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        let owner = probe.read(cx).owner.as_ref().unwrap().clone();
        owner.update(cx, |state, _| {
            state.hovered = true;
            state.pointer = point(px(10.), px(10.));
        });
        window.render_frame(cx);
        let mut images = probe.read(cx).owner.as_ref().unwrap().read(cx).audit().1;
        assert!(images.len() >= 3);
        probe.update(cx, |probe, cx| { probe.dark = true; probe.top = px(4.); cx.notify(); });
        window.render_frame(cx);
        images.extend(probe.read(cx).owner.as_ref().unwrap().read(cx).audit().1);
        assert!(images.iter().all(|image| window.has_image_atlas_entry(image)));
        assert_eq!(window.pixel_snap(px(0.1)), px(0.));
        probe.update(cx, |probe, cx| { probe.paint_subpixel = true; cx.notify(); });
        window.render_frame(cx);
        let owner = probe.read(cx).owner.as_ref().unwrap().read(cx);
        assert!(owner.audit().1.is_empty());
        assert_eq!(owner.rim.geometry_audit(), (2, false));
        assert!(!owner.shadow.geometry_audit().1);
        assert!(owner.retired.is_empty());
        assert!(images.iter().all(|image| !window.has_image_atlas_entry(image)));
    }).unwrap();
}
