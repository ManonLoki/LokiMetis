//! 使用真实 paint_image 和 atlas 验证共享需求；外部 Arc 观察不参与资源拥有权。

use super::*;
use crate::app::surface::{raster::ShadowGeometry, textures::Retired};
use gpui_kit::{AppContext, Context, Corners, IntoElement, Render, TestAppContext, WindowOptions, canvas, prelude::*, px, test::TestWindowExt};

/// 极小图片只承担真实 atlas 生命周期检查，不替代阴影像素 oracle。
struct Probe { image: Arc<RenderImage> }
impl Render for Probe {
    /// 绘制共享实际 ImageId，之后可直接观察 atlas 是否保留或移除。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let image = self.image.clone();
        canvas(|_, _, _| (), move |bounds, _, window, _| {
            window.paint_image(bounds, bounds, Corners::default(), image, 0, false).unwrap();
        }).size(px(4.))
    }
}

/// 有效像素来自内存，测试不读取真实目录或创建异步资产。
fn pixels() -> Pixels { Pixels { width: 4, height: 4, bytes: vec![255; 64] } }

/// 两种主题生成不同完整样本键，比例或圆角仍由生产 Key 构造器编码。
fn key(dark: bool) -> Key { Key::new((320, 180), 1., 20., ShadowSample::new(dark, 0., 1.)) }

/// 同窗同键只生成一次；一个 owner 退场不会删除另一个仍使用的 atlas。
#[gpui_kit::test]
fn last_shadow_lease_releases_atlas_even_with_external_image_arcs(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut a = None;
    let mut b = None;
    let (handle, _) = cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        a = Some(acquire(key(false), window, cx, pixels));
        b = Some(acquire(key(false), window, cx, || panic!("相同实际键不可重新 raster")));
        assert!(Rc::ptr_eq(a.as_ref().unwrap(), b.as_ref().unwrap()));
        cx.new(|_| Probe { image: a.as_ref().unwrap().image.clone() })
    }).unwrap());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let image = a.as_ref().unwrap().image.clone();
        assert!(window.has_image_atlas_entry(&image));
        a.take().unwrap().release(window);
        assert!(window.has_image_atlas_entry(&image));
        assert_eq!(b.as_ref().unwrap().pool.borrow().images.len(), 1);
        b.take().unwrap().release(window);
        assert!(!window.has_image_atlas_entry(&image), "绘制和审计 Arc 不能保活 atlas 需求");
    }).unwrap();
}

/// A→B→A 先重新取得当前需求，再替换退休槽，旧回调不能删除重新使用的 A。
#[gpui_kit::test]
fn reacquired_shadow_survives_retirement_and_history_stays_bounded(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut current = None;
    let mut retired = Retired::default();
    let (handle, probe) = cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        current = Some(acquire(key(false), window, cx, pixels));
        cx.new(|_| Probe { image: current.as_ref().unwrap().image.clone() })
    }).unwrap());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let a = current.as_ref().unwrap().image.clone();
        let b = acquire(key(true), window, cx, pixels);
        retired.push_shared_shadow(current.replace(b).unwrap(), window);
        let again = acquire(key(false), window, cx, || panic!("退休但仍有效的 A 必须可重取"));
        assert_eq!(a.id, again.image.id);
        retired.push_shared_shadow(current.replace(again).unwrap(), window);
        assert!(window.has_image_atlas_entry(&a));
        retired.clear(window);
        assert!(window.has_image_atlas_entry(&a));
        for index in 0..100 {
            let mut changed = key(false);
            changed.sample[0] = (20. + f64::from(index)).to_bits();
            let next = acquire(changed, window, cx, pixels);
            retired.push_shared_shadow(current.replace(next).unwrap(), window);
            assert!(current.as_ref().unwrap().pool.borrow().images.len() <= 2);
        }
        let latest = current.as_ref().unwrap().image.clone();
        probe.update(cx, |probe, cx| { probe.image = latest.clone(); cx.notify(); });
        window.render_frame(cx);
        current.take().unwrap().release(window);
        retired.clear(window);
        assert!(!window.has_image_atlas_entry(&latest));
        assert!(!window.has_image_atlas_entry(&a));
    }).unwrap();
}

/// 不同窗口不得共用 lease；关闭 A 后外部旧需求仍不能影响 B 的 atlas。
#[gpui_kit::test]
fn shadow_pools_are_window_local_and_closed_pool_is_removed(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut leases = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..2 {
        let (handle, _) = cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let lease = acquire(key(false), window, cx, pixels);
            let image = lease.image.clone();
            leases.push(lease);
            cx.new(|_| Probe { image })
        }).unwrap());
        handles.push(handle);
    }
    assert_ne!(leases[0].image.id, leases[1].image.id);
    for handle in &handles { cx.update_window(*handle, |_, window, cx| window.render_frame(cx)).unwrap(); }
    cx.update_window(handles[0], |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(registry(cx).windows.len(), 1);
        assert!(leases[0].pool.borrow().images.is_empty());
    });
    cx.update_window(handles[1], |_, window, _| {
        assert!(window.has_image_atlas_entry(&leases[1].image));
        leases.remove(1).release(window);
    }).unwrap();
}

/// 长卡片的 canonical 像素完全相同，短轴、比例、圆角和动态参数仍区分键。
#[test]
fn canonical_shadow_keys_match_exact_pixels_without_merging_short_axes() {
    for scale in [1., 1.125, 1.5, 2.] {
        let a = ShadowGeometry::new(2000, 1800, scale, 20.);
        let b = ShadowGeometry::new(4000, 2400, scale, 20.);
        let sample = ShadowSample::new(false, 0., 1.);
        let ka = Key::new(a.canonical_dimensions(), scale as f32, 20., sample);
        let kb = Key::new(b.canonical_dimensions(), scale as f32, 20., sample);
        assert!(ka == kb);
        for sample in [sample, ShadowSample::new(true, 1., 1.)] {
            assert_eq!(a.render(sample.scaled(scale)).bytes, b.render(sample.scaled(scale)).bytes);
        }
        let short = ShadowGeometry::new(50, 1800, scale, 20.);
        assert!(ka != Key::new(short.canonical_dimensions(), scale as f32, 20., sample));
        assert!(ka != Key::new(a.canonical_dimensions(), scale as f32, 12., sample));
        let mut changed = sample;
        changed.alpha = f64::from_bits(changed.alpha.to_bits() + 1);
        assert!(ka != Key::new(a.canonical_dimensions(), scale as f32, 20., changed));
    }
}
