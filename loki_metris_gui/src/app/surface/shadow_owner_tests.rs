//! 真实材质 owner 验证固定几何复用及隐藏、材质替换时的及时释放。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, IntoElement, Render, TestAppContext, WindowOptions,
    WindowVisibility, canvas, prelude::*, test::TestWindowExt,
};
use std::time::Duration;

/// 可变尺寸、主题和材质共用同一 owner，闭包仅持有弱引用。
struct Probe {
    owner: Entity<MaterialState>,
    dark: bool,
    radius: Pixels,
    width: Pixels,
    height: Pixels,
    left: Pixels,
    material: MaterialKind,
}

impl Render for Probe {
    /// 调用生产材质路径，所有生成图像都实际绘制到测试窗口图集。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.downgrade();
        let (dark, radius, material) = (self.dark, self.radius, self.material);
        gpui_kit::div().pl(self.left).child(canvas(|_, _, _| (), move |bounds, _, window, cx| {
            owner.update(cx, |state, cx| state.paint(bounds, dark, Geometry {
                radius, top_width: px(1.), top_accent: None,
            }, material, window, cx)).unwrap();
        }).w(self.width).h(self.height))
    }
}

/// 初始可见的普通表格卡片不引入光斑、磁盘或后台服务。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>, Entity<MaterialState>) {
    cx.update(gpui_kit::init);
    let (handle, probe) = cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
        let owner = cx.new(|cx| {
            let mut state = MaterialState::new(window, cx);
            state.visible = true;
            state
        });
        cx.new(|_| Probe { owner, dark: false, radius: px(20.), width: px(320.),
            height: px(180.), left: px(0.), material: MaterialKind::Static(StaticMaterial::Table) })
    }).unwrap());
    let owner = cx.update(|cx| probe.read(cx).owner.clone());
    (handle, probe, owner)
}

/// 连续主题样本仅更新图像；平移不重建，尺寸、圆角、比例各失效一次。
#[gpui_kit::test]
fn theme_frames_reuse_geometry_and_geometric_changes_rebuild(cx: &mut TestAppContext) {
    let (handle, probe, owner) = open(cx);
    let mut previous = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let state = owner.read(cx);
        assert_eq!(state.shadow.geometry_audit(), (1, true));
        state.shadow.image().unwrap().id
    }).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.image().unwrap().id, previous);
        probe.update(cx, |probe, cx| { probe.dark = true; cx.notify(); });
        window.render_frame(cx);
    }).unwrap();
    for millis in [80, 70, 149] {
        cx.executor().advance_clock(Duration::from_millis(millis));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let state = owner.read(cx);
            assert_eq!(state.shadow.geometry_audit(), (1, true));
            let image = state.shadow.image().unwrap();
            assert_ne!(image.id, previous, "实际主题样本应继续变化");
            previous = image.id;
        }).unwrap();
    }
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (1, true));
        previous = owner.read(cx).shadow.image().unwrap().id;
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.image().unwrap().id, previous);
        probe.update(cx, |probe, cx| { probe.left = px(9.); cx.notify(); });
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (1, true));
        assert_eq!(owner.read(cx).shadow.image().unwrap().id, previous);
        for expected in 2..=4 {
            probe.update(cx, |probe, cx| {
                match expected { 2 => probe.width += px(1.), 3 => probe.height += px(1.), _ => probe.radius = px(12.) }
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(owner.read(cx).shadow.geometry_audit(), (expected, true));
        }
    }).unwrap();
    cx.simulate_window_scale_factor_change(handle, 1.25);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (5, true));
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (5, true));
    }).unwrap();
}

/// 同 ID 改为筛选条即清几何；隐藏同步释放当前及退休图，无须等待后帧。
#[gpui_kit::test]
fn filter_bar_and_hidden_window_release_resident_geometry(cx: &mut TestAppContext) {
    let (handle, probe, owner) = open(cx);
    let first = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let first = owner.read(cx).shadow.image().unwrap();
        assert!(window.has_image_atlas_entry(&first));
        probe.update(cx, |probe, cx| { probe.material = MaterialKind::Static(StaticMaterial::FilterBar); cx.notify(); });
        window.render_frame(cx);
        let state = owner.read(cx);
        assert_eq!(state.shadow.geometry_audit(), (1, false));
        assert!(state.shadow.image().is_none());
        first
    }).unwrap();
    let restored = cx.update_window(handle, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(!window.has_image_atlas_entry(&first));
        probe.update(cx, |probe, cx| { probe.material = MaterialKind::Static(StaticMaterial::Table); cx.notify(); });
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (2, true));
        owner.read(cx).shadow.image().unwrap()
    }).unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (2, false));
        assert!(owner.read(cx).shadow.image().is_none());
        assert!(!window.has_image_atlas_entry(&restored));
        assert!(!window.has_image_atlas_entry(&first));
    }).unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
    cx.update_window(handle, |_, window, cx| {
        owner.update(cx, |state, _| state.visible = true);
        window.render_frame(cx);
        assert_eq!(owner.read(cx).shadow.geometry_audit(), (3, true));
    }).unwrap();
}
