//! 原生控件的透明绘制适配：空心焦点线与禁用辅助状态不改变布局或动作。

use gpui_kit::{
    A11ySubtreeBuilder, App, BorderStyle, Bounds, Edges, Element, ElementId, FocusHandle,
    GlobalElementId, InspectorElementId, Interactivity, InteractiveElement, IntoElement, LayoutId,
    Pixels, Rgba, StatefulInteractiveElement, Window, accesskit, quad, transparent_black,
};

/// 只借用已有交互对象来使用状态化辅助回调，不参与绘制或创建代理节点。
struct StatefulControl<'a>(&'a mut Interactivity);
impl InteractiveElement for StatefulControl<'_> {
    /// 回调仍写入Kit向内层原生Div转交的同一个交互对象。
    fn interactivity(&mut self) -> &mut Interactivity {
        self.0
    }
}
impl StatefulInteractiveElement for StatefulControl<'_> {}

/// RenderOnce包装不转发辅助角色；在实际原节点构建后补充禁用位。
/// 仅用于尚未提供合成辅助回调的控件；既有回调须先组合，避免替换。
pub(in crate::app) fn mark_render_once_disabled(interactivity: &mut Interactivity) {
    StatefulControl(interactivity)
        .a11y_synthetic_children(|builder| builder.parent_node().set_disabled());
}

/// 焦点线沿实际控件边缘向外绘制，透明中心保留原背景及其alpha。
pub(in crate::app) struct FocusRing {
    pub focus: FocusHandle,
    pub radius: Pixels,
    pub gap: Pixels,
    pub width: Pixels,
    pub color: Rgba,
}

/// 完整转发具体元素，保持同一ID、布局、命中盒及单一辅助节点。
pub(in crate::app) struct NativeControl<E> {
    inner: E,
    disabled: bool,
    ring: Option<FocusRing>,
}
impl<E: Element> NativeControl<E> {
    /// 适配拥有辅助角色的具体Element；RenderOnce控件改用原交互对象回调。
    pub fn new(inner: E, disabled: bool) -> Self {
        Self {
            inner,
            disabled,
            ring: None,
        }
    }
    /// 只在键盘可见焦点时绘制，不用会填满透明中心的BoxShadow。
    pub fn focus_ring(mut self, ring: FocusRing) -> Self {
        self.ring = Some(ring);
        self
    }
}
impl<E: Element> IntoElement for NativeControl<E> {
    type Element = Self;
    /// 直接进入元素生命周期，避免再增加一层AnyElement作用域。
    fn into_element(self) -> Self {
        self
    }
}
impl<E: Element> Element for NativeControl<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    /// 使用内层ID，测试和辅助技术仍定位原控件。
    fn id(&self) -> Option<ElementId> {
        self.inner.id()
    }
    /// 检查器继续使用原生控件的构造位置。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }
    /// 不制造代理角色或额外辅助节点。
    fn a11y_role(&self) -> Option<accesskit::Role> {
        self.inner.a11y_role()
    }
    /// 保留名称、selected和原动作，再补原生组件尚未导出的禁用位。
    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
    }
    /// 原生控件的合成子节点沿原生命周期构建一次。
    fn a11y_synthetic_children(
        &mut self,
        state: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(state, builder);
    }
    /// 直接转发具体元素，尺寸和按键状态由原控件决定。
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // HTML disabled会取消自己的焦点；generation变化也取消禁用前的原生持键。
        if self.disabled
            && self
                .ring
                .as_ref()
                .is_some_and(|ring| ring.focus.is_focused(window))
        {
            window.blur(cx);
        }
        self.inner.request_layout(id, inspector, window, cx)
    }
    /// 保留原命中盒、焦点和位移，不增加布局容器。
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.inner
            .prepaint(id, inspector, bounds, request, window, cx)
    }
    /// 焦点线中心透明；控件内容、边框及输入行为仍由内层绘制。
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        state: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(ring) = &self.ring
            && !self.disabled
            && ring.focus.is_focused(window)
            && window.last_input_was_keyboard()
        {
            let spread = ring.gap + ring.width;
            let radius = ring
                .radius
                .min(bounds.size.height / 2.)
                .min(bounds.size.width / 2.);
            window.paint_quad(quad(
                bounds.dilate(spread),
                radius + spread,
                transparent_black(),
                Edges::all(ring.width),
                ring.color,
                BorderStyle::Solid,
            ));
        }
        self.inner
            .paint(id, inspector, bounds, request, state, window, cx);
    }
}
