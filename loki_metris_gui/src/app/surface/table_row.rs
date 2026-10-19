//! 品牌表格的tbody行保留原生Row/Cell语义、列尺寸与子控件事件所有权。

use super::{motion::CssMotion, palette};
use gpui_kit::{
    A11ySubtreeBuilder, App, BorderStyle, Bounds, Context, Corners, Div, Edges, Element, ElementId, Entity, GlobalElementId,
    HoverListenerMode, InspectorElementId, IntoElement, LayoutId, Pixels, Role, Stateful, Styled,
    StyleRefinement, Visibility, Window, accesskit, div, prelude::*, px, quad, transparent_black,
    component::{ActiveTheme, AnyChildElement, ChildElement, Sizable, Size},
    base::{ObservedElement, TestSupportExt as _},
};
use std::time::Duration;

/// 原生观察包装与gpui-base TableRow相同，不产生额外布局或辅助节点。
type Inner = ObservedElement<Stateful<Div>>;

/// 行只拥有透明背景的纯交互时间线，不分配图片、GPU图集或后台任务。
pub(in crate::app) struct RowState {
    /// 指针悬停。
    hovered: bool,
    /// 背景色过渡。
    motion: CssMotion<4>,
}

impl RowState {
    /// 初始目标是透明底色，页面挂载不播放虚构的入场过渡。
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            hovered: false,
            motion: CssMotion::with_duration(cx.background_executor().now(), Duration::from_millis(160)),
        }
    }

    /// hover仅更新本行的背景目标，子按钮和选择框继续处理自己的输入事件。
    fn hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
}

/// 显式稳定ID用于保存行交互；序号由TableBody传入，仅决定辅助索引及行边框。
pub(in crate::app) struct BrandTableRow {
    /// 内层行元素。
    inner: Inner,
    /// 深色主题。
    dark: bool,
    /// 行序号，决定辅助索引与行边框。
    ix: usize,
    /// 行尺寸档位。
    size: Size,
    /// 末行底部圆角。
    bottom_corner_radius: Pixels,
    /// 单元格。
    children: Vec<AnyChildElement>,
    /// 测试中观察行状态。
    #[cfg(test)]
    owner_observer: Option<super::OwnerObserver<RowState>>,
}

impl BrandTableRow {
    /// 仅用于历史table-panel的tbody；表头继续使用普通TableRow。
    pub fn new(id: impl Into<ElementId>, dark: bool) -> Self {
        Self {
            inner: div().id(id).test_support().w_full().flex().flex_row().role(Role::Row).aria_row_index(1),
            dark,
            ix: 0,
            size: Size::default(),
            bottom_corner_radius: px(0.),
            children: Vec::new(),
            #[cfg(test)]
            owner_observer: None,
        }
    }

    /// 单元格仍使用gpui-component的ChildElement尺寸与列索引传递。
    pub fn child(mut self, child: impl ChildElement + 'static) -> Self {
        self.children.push(AnyChildElement::new(child));
        self
    }

    /// 同一行的每列按原TableRow顺序编号，不给文本或按钮增加代理动作。
    pub fn children<E: ChildElement + 'static>(mut self, children: impl IntoIterator<Item = E>) -> Self {
        self.children.extend(children.into_iter().map(AnyChildElement::new));
        self
    }

    /// 最后一行采用卡片边框内侧圆角；中间行保持直边，不改变布局或命中。
    pub fn bottom_corner_radius(mut self, radius: Pixels) -> Self {
        self.bottom_corner_radius = radius.max(px(0.));
        self
    }

    /// 弱引用观察不延长生产owner生命周期，用于真实卸载回归。
    #[cfg(test)]
    pub(super) fn observe_owner(mut self, observer: impl FnOnce(&Entity<RowState>) + 'static) -> Self {
        self.owner_observer = Some(Box::new(observer));
        self
    }
}

impl Styled for BrandTableRow {
    /// 原行宽度、文本和内边距仍交由调用者设置。
    fn style(&mut self) -> &mut StyleRefinement { self.inner.style() }
}

impl Sizable for BrandTableRow {
    /// Table的尺寸向每个真实单元格传递，不改变列宽或横向滚动约束。
    fn with_size(mut self, size: impl Into<Size>) -> Self { self.size = size.into(); self }
}

impl ChildElement for BrandTableRow {
    /// 辅助行号从1开始，稳定ID不因排序位置而变动。
    fn with_ix(mut self, ix: usize) -> Self {
        self.ix = ix;
        self.inner = self.inner.aria_row_index(ix + 1);
        self
    }
}

impl IntoElement for BrandTableRow {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 行直接参与原生布局，不插入额外Row或Group包裹层。
    fn into_element(self) -> Self { self }
}

impl Element for BrandTableRow {
    /// 内部行的布局状态与行交互状态实体。
    type RequestLayoutState = (<Inner as Element>::RequestLayoutState, Entity<RowState>);
    /// 沿用内部行的预绘制状态。
    type PrepaintState = <Inner as Element>::PrepaintState;

    /// 稳定产品行ID同时限定keyed owner和辅助树的作用域。
    fn id(&self) -> Option<ElementId> { Element::id(&self.inner) }
    /// 保留实际容器的源码检查位置。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { self.inner.source_location() }
    /// 暴露唯一真实Row，子Cell和按钮继续形成自己的辅助节点。
    fn a11y_role(&self) -> Option<accesskit::Role> { self.inner.a11y_role() }
    /// 行号等原生辅助属性完全来自真实容器。
    fn write_a11y_info(&self, node: &mut accesskit::Node) { self.inner.write_a11y_info(node); }
    /// 子节点沿原生容器路径构建，不复制或压平单元格语义。
    fn a11y_synthetic_children(&mut self, prepaint: &mut Self::PrepaintState, builder: &mut A11ySubtreeBuilder) {
        Element::a11y_synthetic_children(&mut self.inner, prepaint, builder);
    }

    /// 复用原TableRow的边框与列尺寸；纯交互owner由稳定行ID隔离。
    fn request_layout(&mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, Self::RequestLayoutState) {
        let owner = window.use_keyed_state("brand-table-row", cx, RowState::new);
        #[cfg(test)]
        if let Some(observer) = self.owner_observer.take() { observer(&owner); }
        let weak = owner.downgrade();
        self.inner.interactivity().hover_listener_mode(HoverListenerMode::InputModalityIndependent);
        self.inner.interactivity().on_hover(move |hovered, _, cx| {
            let _ = weak.update(cx, |state, cx| state.hover(*hovered, cx));
        });
        self.inner.style().border_color = Some(cx.theme().table_row_border);
        if self.ix > 0 { self.inner.style().border_widths.top = Some(gpui_kit::px(1.).into()); }
        self.inner.extend(std::mem::take(&mut self.children).into_iter().enumerate().map(|(ix, child)| child.into_any(ix, self.size)));
        let (layout, frame) = self.inner.request_layout(id, inspector, window, cx);
        (layout, (frame, owner))
    }

    /// 原生行命中与单元格布局共享同一个真实边界。
    fn prepaint(&mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, request: &mut Self::RequestLayoutState, window: &mut Window, cx: &mut App) -> Self::PrepaintState {
        self.inner.prepaint(id, inspector, bounds, &mut request.0, window, cx)
    }

    /// 背景位于原单元格内容下方，不成为点击或焦点目标。
    fn paint(&mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, request: &mut Self::RequestLayoutState, prepaint: &mut Self::PrepaintState, window: &mut Window, cx: &mut App) {
        request.1.update(cx, |state, cx| {
            let target = if state.hovered { palette::row_hover(self.dark) } else { [0.; 4] };
            let now = cx.background_executor().now();
            state.motion.set_target(target, now);
            let (color, running) = state.motion.sample(now);
            let visible = self.inner.style().visibility != Some(Visibility::Hidden)
                && window.is_visible() && !bounds.intersect(&window.content_mask().bounds).is_empty();
            if visible {
                if color[3] > 0. {
                    let mut background_bounds = bounds;
                    if self.bottom_corner_radius > px(0.) {
                        let visible = bounds.intersect(&window.content_mask().bounds);
                        background_bounds.origin.x = visible.origin.x;
                        background_bounds.size.width = visible.size.width;
                    }
                    let corners = Corners {
                        top_left: px(0.), top_right: px(0.),
                        bottom_left: self.bottom_corner_radius,
                        bottom_right: self.bottom_corner_radius,
                    };
                    window.paint_quad(quad(background_bounds, corners, palette::unpremultiply(color), Edges::default(), transparent_black(), BorderStyle::Solid));
                }
                if running { window.request_animation_frame(); }
            }
        });
        self.inner.paint(id, inspector, bounds, &mut request.0, prepaint, window, cx);
    }
}
