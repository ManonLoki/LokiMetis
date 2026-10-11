#![cfg(target_os = "macos")]
//! 同时启用父圆角、AX及精确输入排除，验证原始owner、deferred/replay和cache组合。

use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ContentMask, Context, Corners, Element, GlobalElementId,
    Hitbox, HitboxBehavior, HitboxId, InspectorElementId, IntoElement, LayoutId, Pixels, Point,
    Render, RenderImage, Role, Style, StyleRefinement, TestAppContext, Window, div, fill, point,
    prelude::*, px, rgba, size,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::Arc,
};

/// 真实父内容mask与owner Hitbox几何在排除scope内仍须完整。
#[derive(Clone)]
struct Observation {
    hitbox: Hitbox,
    mask: ContentMask<Pixels>,
    bounds: Bounds<Pixels>,
}

/// 自有最小Element创建真实Hitbox、半透明quad及图片，并读取实际AX节点bounds。
struct PaintedTarget {
    label: &'static str,
    records: Rc<RefCell<Vec<Observation>>>,
    ax_bounds: Rc<RefCell<Option<gpui_kit::accesskit::Rect>>>,
    image: Option<Arc<RenderImage>>,
}
impl IntoElement for PaintedTarget {
    type Element = Self;
    /// 无额外透明包装或假节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PaintedTarget {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;
    /// 同一真实控件拥有稳定元素身份。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        Some(self.label.into())
    }
    /// 测试元素无额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 辅助角色来自实际Element钩子，而不是手工构造TreeUpdate。
    fn a11y_role(&self) -> Option<Role> {
        Some(Role::Button)
    }
    /// 同一真实节点保留名称，范围由GPUI实际布局填写。
    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        node.set_label(self.label);
    }
    /// 固定逻辑100×100，仅用于明确观测半开边界与12px露出缝。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            size: size(px(100.).into(), px(100.).into()),
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }
    /// owner接收完整原Hitbox，Frame仅改变内部输入分片。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Hitbox {
        let mask = window.content_mask();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.records.borrow_mut().push(Observation {
            hitbox: hitbox.clone(),
            mask,
            bounds,
        });
        hitbox
    }
    /// 绘制真实半透明填充与原图；输入排除不得改变其bounds/UV/渲染mask。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut Hitbox,
        window: &mut Window,
        _: &mut App,
    ) {
        if let Some(image) = &self.image {
            window.paint_quad(fill(bounds, rgba(0xff000080)));
            window
                .paint_image(bounds, bounds, Corners::default(), image.clone(), 0, false)
                .unwrap();
        }
    }
    /// 原生实际AX节点bounds可读取，不增加任何观测辅助节点。
    fn a11y_synthetic_children(
        &mut self,
        _: &mut Hitbox,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        *self.ax_bounds.borrow_mut() = builder.parent_node().bounds();
    }
}

/// 只在实际child prepaint期间追加精确排除，保留正常布局/paint/AX调用。
struct ExclusionScope {
    child: AnyElement,
    excluded: Vec<Bounds<Pixels>>,
}
impl ExclusionScope {
    /// 每一层有自己的覆盖矩形列表，空列表不清除外层scope。
    fn new(child: impl IntoElement, excluded: Vec<Bounds<Pixels>>) -> Self {
        Self {
            child: child.into_any_element(),
            excluded,
        }
    }
}
impl IntoElement for ExclusionScope {
    type Element = Self;
    /// 不为scope制造可访问或焦点节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ExclusionScope {
    type RequestLayoutState = ();
    type PrepaintState = ();
    /// scope没有额外身份。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    /// scope没有额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 使用child实际LayoutId，不复制样式几何。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    /// 只影响实际输入区域，原内容mask留给图片可见性与绘制。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_hitbox_exclusions(&self.excluded, |window| self.child.prepaint(window, cx));
    }
    /// paint在scope之外执行，不能给半透明表面画实色遮罩。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// 对照legacy弹层和newparent的实际排除继承与跨帧几何replay。
struct DeferredTarget {
    child: Option<AnyElement>,
    legacy: bool,
}
impl IntoElement for DeferredTarget {
    type Element = Self;
    /// deferred无额外可访问节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for DeferredTarget {
    type RequestLayoutState = ();
    type PrepaintState = ();
    /// 稳定身份属于实际child。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    /// 测试包装没有检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// deferred仍占真实正常流布局。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.as_mut().unwrap().request_layout(window, cx), ())
    }
    /// 两个实际公开入口必须保持不同的输入scope继承合同。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let child = self.child.take().unwrap();
        let offset = window.element_offset();
        let mask = Some(window.content_mask());
        if self.legacy {
            window.defer_draw(child, offset, 1, mask);
        } else {
            window.defer_draw_in_parent(child, offset, 1, mask);
        }
    }
    /// 实际paint只由deferred流执行一次。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

/// 视图属性只持有观测与真实图片，不镜像任何输入业务状态。
#[derive(Clone)]
struct TargetState {
    records: Rc<RefCell<Vec<Observation>>>,
    ax_bounds: Rc<RefCell<Option<gpui_kit::accesskit::Rect>>>,
    image: Arc<RenderImage>,
    renders: Rc<Cell<usize>>,
}
impl TargetState {
    /// 图片仅8×8内存像素，无磁盘/asset异步依赖。
    fn new() -> Self {
        let pixels = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
        Self {
            records: Default::default(),
            ax_bounds: Default::default(),
            image: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
            renders: Default::default(),
        }
    }
    /// 每帧原生控件只记录一次prepaint，缓存复用时不伪造新记录。
    fn latest(&self) -> Observation {
        self.records
            .borrow()
            .last()
            .expect("真实prepaint缺失")
            .clone()
    }
    /// 同一控件的真实Element供直接与cached路径共同使用。
    fn target(&self) -> PaintedTarget {
        PaintedTarget {
            label: "excluded-target",
            records: self.records.clone(),
            ax_bounds: self.ax_bounds.clone(),
            image: Some(self.image.clone()),
        }
    }
}

/// 真实cached实体记录Render次数，用于证明key相等复用、变化及移除重建。
struct CachedTarget {
    state: TargetState,
    mode: usize,
}
impl Render for CachedTarget {
    /// mode0直接child；mode1newparent；mode2legacy popup。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.state.renders.set(self.state.renders.get() + 1);
        let child = self.state.target().into_any_element();
        if self.mode == 0 {
            child
        } else {
            DeferredTarget {
                child: Some(child),
                legacy: self.mode == 2,
            }
            .into_any_element()
        }
    }
}

/// 矩形状态属于根render输入；修改它不会notify cached子实体。
struct ScopeRoot {
    excluded: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    nested: Vec<Bounds<Pixels>>,
    radius: Rc<Cell<f32>>,
    state: TargetState,
    cached: Option<gpui_kit::Entity<CachedTarget>>,
    outside: Rc<RefCell<Vec<Observation>>>,
}
impl Render for ScopeRoot {
    /// 父滚动hitbox在scope外，第二个同位置控件证明scope结束后已恢复。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let child = self
            .cached
            .as_ref()
            .map(|entity| {
                entity
                    .clone()
                    .cached(StyleRefinement::default().size(px(100.)))
                    .into_any_element()
            })
            .unwrap_or_else(|| self.state.target().into_any_element());
        div()
            .id("exclusions-root")
            .role(Role::Group)
            .aria_label("exclusions-root")
            .relative()
            .size(px(100.))
            .on_scroll_wheel(|_, _, _| {})
            .child(
                div()
                    .size(px(100.))
                    .overflow_hidden()
                    .rounded(px(self.radius.get()))
                    .clip_rounded_children()
                    .child(ExclusionScope::new(
                        ExclusionScope::new(child, self.nested.clone()),
                        self.excluded.borrow().clone(),
                    )),
            )
            .child(div().absolute().inset_0().child(PaintedTarget {
                label: "outside-scope",
                records: self.outside.clone(),
                ax_bounds: Default::default(),
                image: None,
            }))
    }
}

/// 全部bounds是稳定窗口坐标，遵循原生半开区间。
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

/// 返回真实窗口与外部观测，不启用native Services或操作OS。
fn setup(
    cx: &mut TestAppContext,
    excluded: Vec<Bounds<Pixels>>,
    nested: Vec<Bounds<Pixels>>,
    cached: Option<usize>,
) -> (
    gpui_kit::AnyWindowHandle,
    TargetState,
    Rc<RefCell<Vec<Bounds<Pixels>>>>,
    Rc<RefCell<Vec<Observation>>>,
    Rc<Cell<f32>>,
) {
    let state = TargetState::new();
    let returned = state.clone();
    let excluded = Rc::new(RefCell::new(excluded));
    let list = excluded.clone();
    let outside: Rc<RefCell<Vec<Observation>>> = Default::default();
    let returned_outside = outside.clone();
    let radius = Rc::new(Cell::new(20.));
    let returned_radius = radius.clone();
    let handle = cx.add_window(move |_, cx| ScopeRoot {
        cached: cached.map(|mode| {
            cx.new(|_| CachedTarget {
                state: state.clone(),
                mode,
            })
        }),
        excluded,
        nested,
        radius,
        state,
        outside,
    });
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    (
        handle.into(),
        returned,
        list,
        returned_outside,
        returned_radius,
    )
}

/// 读取真实Frame全ID列表；同ID重复即失败，不能用bool或handler次数替代。
fn actual_ids(window: &Window, point: Point<Pixels>) -> Vec<HitboxId> {
    let ids = window.hit_test_ids_for_testing(point);
    let unique: HashSet<_> = ids.iter().copied().collect();
    assert_eq!(
        unique.len(),
        ids.len(),
        "真实Frame在{point:?}重复命中共享ID：{ids:?}"
    );
    ids
}

/// owner几何、实际quad/图片和真实AX范围应均不受输入覆盖排除影响。
fn assert_paint_owner_and_ax(
    window: &Window,
    state: &TargetState,
    expected_prepaint_mask: ContentMask<Pixels>,
    expected_paint_mask: ContentMask<Pixels>,
) {
    let record = state.latest();
    let scale = window.scale_factor();
    assert_eq!(record.bounds, rect(0., 0., 100., 100.));
    assert_eq!(record.hitbox.bounds, record.bounds);
    assert_eq!(record.mask, expected_prepaint_mask);
    assert_eq!(record.hitbox.content_mask, record.mask);
    assert!(
        !record.hitbox.parent_clips.is_empty(),
        "owner必须保存原始CPU圆角链"
    );
    let quads = window.painted_quads();
    assert_eq!(quads.len(), 1);
    assert_eq!(quads[0].bounds, record.bounds.scale(scale));
    assert_eq!(quads[0].content_mask, expected_paint_mask.scale(scale));
    assert_eq!(quads[0].background.as_solid().unwrap().a, 128. / 255.);
    let images = window.painted_polychrome_sprites();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].bounds, record.bounds.scale(scale));
    assert_eq!(images[0].content_mask, expected_paint_mask.scale(scale));
    assert_ne!(quads[0].parent_clip.head, 0);
    assert_ne!(images[0].parent_clip.head, 0);
    assert_eq!(
        images[0].corner_radii,
        Corners::all(px(0.).scale(scale)),
        "图片仅父链AA"
    );
    if window.is_a11y_active() {
        let bounds = state
            .ax_bounds
            .borrow()
            .expect("必须读到同一个真实AX节点bounds");
        assert_eq!(
            bounds,
            gpui_kit::accesskit::Rect {
                x0: 0.,
                y0: 0.,
                x1: (100. * scale) as f64,
                y1: (100. * scale) as f64
            }
        );
        let tree: serde_json::Value =
            serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
        assert_eq!(
            tree["nodes"]
                .as_object()
                .unwrap()
                .values()
                .filter(|node| node["aria"]["label"] == "excluded-target")
                .count(),
            1
        );
    }
}

/// 12px底缝、页头上方、横向露出和各切割边均须保持完整可见输入归属。
#[gpui_kit::test]
fn parent_clip_exclusions_exact_half_open_gaps_nested_and_scope_restore(cx: &mut TestAppContext) {
    let header = rect(0., 20., 100., 20.);
    let footer = rect(0., 70., 100., 18.);
    let nested = rect(30., 35., 20., 50.);
    let (handle, state, _, outside, _) = setup(cx, vec![header, footer], vec![nested], None);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        let mask = ContentMask { bounds: rect(0., 0., 100., 100.) };
        assert_paint_owner_and_ax(window, &state, mask.clone(), mask);
        let target = state.latest().hitbox.id;
        let restored = outside.borrow().last().unwrap().hitbox.id;
        let parents = actual_ids(window, point(px(10.), px(10.)))
            .into_iter()
            .filter(|id| *id != target && *id != restored)
            .collect::<Vec<_>>();
        assert_eq!(parents.len(), 1, "真实父overflow须恰有一个独立命中ID");
        let parent = parents[0];
        for x in [0., 29.999, 30., 49.999, 50., 99.999, 100.] {
            for y in [
                0., 19.999, 20., 34.999, 35., 39.999, 40., 69.999, 70., 84.999, 85., 87.999, 88.,
                99.999, 100.,
            ] {
                let at = point(px(x), px(y));
                let ids = actual_ids(window, at);
                let in_bounds = rect(0., 0., 100., 100.).contains(&at);
                let expected = in_bounds
                    && state.latest().hitbox.parent_clips.contains(&at)
                    && ![header, footer, nested]
                        .iter()
                        .any(|bounds| bounds.contains(&at));
                assert_eq!(ids.contains(&target), expected, "实际input补集错误：{at:?}");
                assert_eq!(
                    ids.contains(&restored),
                    in_bounds,
                    "退出scope须恢复全部露出输入"
                );
                assert_eq!(
                    ids.contains(&parent),
                    in_bounds,
                    "排除不得同时剪掉父overflow"
                );
            }
        }
    })
    .unwrap();
}

/// 全遮、重复覆盖及空矩形不可留下可命中的零面积片，也不可影响scope外父控件。
#[gpui_kit::test]
fn parent_clip_exclusions_full_empty_overlap_and_default_path(cx: &mut TestAppContext) {
    let full = rect(0., 0., 100., 100.);
    let (handle, state, exclusions, outside, _) =
        setup(cx, vec![full, full, rect(0., 0., 0., 100.)], vec![], None);
    cx.update_window(handle, |_, window, cx| {
        for _ in 0..3 {
            window.draw(cx).clear(cx);
            let target = state.latest().hitbox.id;
            let outside = outside.borrow().last().unwrap().hitbox.id;
            for at in [
                point(px(0.), px(0.)),
                point(px(50.), px(50.)),
                point(px(99.999), px(99.999)),
            ] {
                let ids = actual_ids(window, at);
                assert!(!ids.contains(&target));
                assert!(ids.contains(&outside));
                assert_eq!(ids.len(), 2);
            }
        }
        exclusions.borrow_mut().clear();
        window.draw(cx).clear(cx);
        let target = state.latest().hitbox.id;
        for at in [
            point(px(0.), px(0.)),
            point(px(50.), px(50.)),
            point(px(99.999), px(99.999)),
        ] {
            let ids = actual_ids(window, at);
            let expected = state.latest().hitbox.parent_clips.contains(&at);
            assert_eq!(ids.contains(&target), expected);
            assert_eq!(ids.len(), if expected { 3 } else { 2 });
        }
    })
    .unwrap();
}

/// 真缓存不能被render_frame强制refresh掩盖，same/changed/removed列表均走实际准入。
#[gpui_kit::test]
fn parent_clip_exclusions_cache_key_same_changed_removed_and_replay(cx: &mut TestAppContext) {
    let (handle, state, exclusions, _, _) =
        setup(cx, vec![rect(0., 40., 100., 20.)], vec![], Some(0));
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        let renders = state.renders.get();
        let id = state.latest().hitbox.id;
        for _ in 0..3 {
            window.draw(cx).clear(cx);
            assert_eq!(state.renders.get(), renders);
            assert!(!actual_ids(window, point(px(50.), px(50.))).contains(&id));
        }
        *exclusions.borrow_mut() = vec![rect(0., 60., 100., 20.)];
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), renders + 1);
        let id = state.latest().hitbox.id;
        assert!(actual_ids(window, point(px(50.), px(50.))).contains(&id));
        assert!(!actual_ids(window, point(px(50.), px(70.))).contains(&id));
        let mask = ContentMask { bounds: rect(0., 0., 100., 100.) };
        assert_paint_owner_and_ax(window, &state, mask.clone(), mask);
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), renders + 1);
        exclusions.borrow_mut().clear();
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), renders + 2);
        let id = state.latest().hitbox.id;
        assert!(actual_ids(window, point(px(50.), px(70.))).contains(&id));
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), renders + 2);
    })
    .unwrap();
}

/// 新父API明确继承input排除且inactive replay保持几何；legacy popup仍能逃离覆盖scope。
#[gpui_kit::test]
fn parent_clip_exclusions_parent_deferred_replay_and_legacy_escape(cx: &mut TestAppContext) {
    for mode in [1, 2] {
        let (handle, state, _, _, _) =
            setup(cx, vec![rect(0., 40., 100., 20.)], vec![], Some(mode));
        cx.update_window(handle, |_, window, cx| {
            window.draw(cx).clear(cx);
            let renders = state.renders.get();
            let id = state.latest().hitbox.id;
            for _ in 0..3 {
                window.draw(cx).clear(cx);
                assert_eq!(state.renders.get(), renders);
                assert_eq!(
                    actual_ids(window, point(px(50.), px(50.))).contains(&id),
                    mode == 2
                );
                assert!(actual_ids(window, point(px(50.), px(90.))).contains(&id));
                let paint_mask = ContentMask { bounds: rect(0., 0., 100., 100.) };
                // legacy 弹层在 prepaint 逃离矩形 mask，paint 仍使用提交时的父矩形。
                let prepaint_mask = if mode == 2 {
                    ContentMask { bounds: Bounds::new(Point::default(), window.viewport_size()) }
                } else {
                    paint_mask.clone()
                };
                assert_paint_owner_and_ax(window, &state, prepaint_mask, paint_mask);
            }
        })
        .unwrap();
    }
}

/// 父链变化本身必须失效缓存；同时保存exclusions key，不能只通过任一scope的独立测试。
#[gpui_kit::test]
fn parent_clip_exclusions_cache_radius_change_restores_and_retains_exclusions(
    cx: &mut TestAppContext,
) {
    let (handle, state, _, _, radius) = setup(cx, vec![rect(0., 40., 100., 20.)], vec![], Some(0));
    cx.update_window(handle, |_, window, cx| {
        window.draw(cx).clear(cx);
        let initial = state.renders.get();
        let id = state.latest().hitbox.id;
        let corner = point(px(10.), px(10.));
        assert!(actual_ids(window, corner).contains(&id));
        assert!(!actual_ids(window, point(px(50.), px(50.))).contains(&id));
        radius.set(40.);
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), initial + 1);
        let id = state.latest().hitbox.id;
        assert!(!actual_ids(window, corner).contains(&id));
        assert!(!actual_ids(window, point(px(50.), px(50.))).contains(&id));
        assert!(actual_ids(window, point(px(50.), px(10.))).contains(&id));
        let mask = ContentMask { bounds: rect(0., 0., 100., 100.) };
        assert_paint_owner_and_ax(window, &state, mask.clone(), mask);
        for _ in 0..3 {
            window.draw(cx).clear(cx);
            assert_eq!(state.renders.get(), initial + 1);
            let id = state.latest().hitbox.id;
            assert!(!actual_ids(window, corner).contains(&id));
        }
        radius.set(20.);
        window.draw(cx).clear(cx);
        assert_eq!(state.renders.get(), initial + 2);
        assert!(actual_ids(window, corner).contains(&state.latest().hitbox.id));
    })
    .unwrap();
}
