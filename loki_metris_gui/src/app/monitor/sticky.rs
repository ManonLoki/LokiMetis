//! 局部粘滞保留真实文档布局和 AX/dispatch 父路径，正文仅排除栏内鼠标命中。

use crate::app::PageScroll;
use gpui_kit::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, point, px,
};

/// 保存栏与页头只需要 CSS bottom12 和 top0 两种约束。
#[derive(Clone, Copy)]
pub(super) enum Edge { Header, Footer }

/// 无辅助节点的包装保留子布局，并区分正常绘制、Tab顺序和粘滞绘制。
#[derive(Clone, Copy)]
enum Kind { Scope, Order(isize), Sticky(Edge) }

/// 两处实际布局共享同一吸附计算；不猜测控件高度，也不扣除页底露出缝隙。
fn painted_bounds(edge: Edge, bounds: Bounds<Pixels>, viewport: Bounds<Pixels>, container: Option<Bounds<Pixels>>) -> Bounds<Pixels> {
    let shift = if viewport.is_empty() { px(0.) } else { match edge {
        Edge::Header => bounds.top().max(viewport.top()).min(container.map(|parent| parent.bottom()).unwrap_or(bounds.bottom()) - bounds.size.height) - bounds.top(),
        Edge::Footer => (viewport.bottom() - px(12.) - bounds.bottom()).min(px(0.)),
    }};
    Bounds { origin: bounds.origin + point(px(0.), shift), ..bounds }
}

/// deferred 顺序与 Tab 顺序分别拥有明确职责，不用第二套按钮或编辑实体。
pub(super) struct MonitorFlow {
    /// 被包装的内容。
    child: Option<AnyElement>,
    /// 页面滚动。
    target: PageScroll,
    /// 粘滞区域类型。
    kind: Kind,
}

impl MonitorFlow {
    /// 只使用本帧 request_layout 的页尾 ID；原生布局结束后即可读取真实高度。
    fn body_exclusions(&self, window: &mut Window) -> Vec<Bounds<Pixels>> {
        let (header, footer_layout, container) = {
            let regions = self.target.regions.borrow();
            (regions.header, regions.footer_layout, regions.container)
        };
        let mut excluded = Vec::with_capacity(2);
        if let Some(header) = header { excluded.push(header); }
        if let Some(layout) = footer_layout {
            excluded.push(painted_bounds(Edge::Footer, window.layout_bounds(layout), self.target.handle.bounds(), container));
        }
        excluded
    }

    /// 整页父边界在子项 prepaint 之前更新，页头不得越过自己的文档底边。
    pub(super) fn scope(child: impl IntoElement, target: PageScroll) -> Self {
        Self { child: Some(child.into_any_element()), target, kind: Kind::Scope }
    }

    /// 原生 TabStops 在 paint 登记，因此 deferred 项必须显式保留其文档顺序。
    pub(super) fn ordered(child: impl IntoElement, target: PageScroll, index: isize) -> Self {
        Self { child: Some(child.into_any_element()), target, kind: Kind::Order(index) }
    }

    /// 页头0、正文1、保存栏2；本地层 save1/header2 始终低于原生 Dialog 的10以上层。
    pub(super) fn sticky(child: impl IntoElement, target: PageScroll, edge: Edge) -> Self {
        let order = match edge { Edge::Header => 0, Edge::Footer => 2 };
        let child = Self::ordered(child, target.clone(), order).into_any_element();
        Self { child: Some(child), target, kind: Kind::Sticky(edge) }
    }
}

impl IntoElement for MonitorFlow {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 包装没有额外样式盒，始终采用内部原生控件的 LayoutId。
    fn into_element(self) -> Self { self }
}

impl Element for MonitorFlow {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段无需向绘制阶段传递状态。
    type PrepaintState = ();
    /// 不插入新的 keyed 或可访问身份，原生子控件仍拥有自己的稳定 ID。
    fn id(&self) -> Option<gpui_kit::ElementId> { None }
    /// 本地元素没有额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }
    /// 返回子项的同一布局 ID，让 sticky 项继续占据真实文档高度。
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let layout = self.child.as_mut().unwrap().request_layout(window, cx);
        if matches!(self.kind, Kind::Sticky(Edge::Footer)) {
            self.target.regions.borrow_mut().footer_layout = Some(layout);
        }
        (layout, ())
    }
    /// 正常边界先登记，粘滞项只在同一父 mask 内平移，不能逃逸父滚动裁切。
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        match self.kind {
            Kind::Scope => {
                self.target.regions.borrow_mut().container = Some(bounds);
                self.child.as_mut().unwrap().prepaint(window, cx);
            }
            Kind::Order(1) => {
                let excluded = self.body_exclusions(window);
                window.with_hitbox_exclusions(&excluded, |window| self.child.as_mut().unwrap().prepaint(window, cx));
            },
            Kind::Order(_) => { self.child.as_mut().unwrap().prepaint(window, cx); },
            Kind::Sticky(edge) => {
                let viewport = self.target.handle.bounds();
                let mut regions = self.target.regions.borrow_mut();
                let painted = painted_bounds(edge, bounds, viewport, regions.container);
                let priority = match edge {
                    Edge::Header => { regions.header = Some(painted); 2 }
                    Edge::Footer => { regions.footer = Some(painted); 1 }
                };
                drop(regions);
                let mask = window.content_mask();
                window.defer_draw_in_parent(self.child.take().unwrap(), window.element_offset() + painted.origin - bounds.origin, priority, Some(mask));
            }
        }
    }
    /// 只给自然文档位置分组，不改变控件的原生 focus、AX 或点击处理。
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        match self.kind {
            Kind::Scope => self.child.as_mut().unwrap().paint(window, cx),
            Kind::Order(index) => window.with_tab_group(Some(index), |window| self.child.as_mut().unwrap().paint(window, cx)),
            Kind::Sticky(_) => {},
        }
    }
}
