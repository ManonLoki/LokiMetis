//! 管理页文本域只适配呈现，原生 Kit 继续拥有编辑、焦点、辅助树、注册和菜单。

mod editor_style;
mod motion;
pub(in crate::app::monitor) mod reveal;
#[cfg(test)]
mod reveal_tests;
#[cfg(all(test, target_os = "macos"))]
mod projection_tests;
#[cfg(test)]
mod tests;

use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, Edges, Element, ElementId, Entity, Focusable, FontWeight,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Rgba, SharedString, Window,
    component::{
        Sizable, Size,
        input::{Textarea, TextareaState},
    },
    prelude::*,
    px, quad, relative, rgb, transparent_black,
};
use motion::BorderTransition;
use crate::app::PageScroll;
use reveal::RevealState;

/// 四个真实输入各自保留状态 ID，包装不增加辅助节点或代理动作。
pub(super) struct MonitorTextarea {
    input: Entity<TextareaState>,
    dark: bool,
    label: SharedString,
    reveal: Option<PageScroll>,
}

impl MonitorTextarea {
    /// catalog 拥有编辑实体；本适配层不复制文本、不建立第二套业务订阅。
    pub(super) fn new(
        input: &Entity<TextareaState>,
        dark: bool,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            input: input.clone(),
            dark,
            label: label.into(),
            reveal: None,
        }
    }

    /// 只关联所属页面的滚动机制，不修改原生输入、辅助树或编辑动作。
    pub(super) fn reveal_into(mut self, target: PageScroll) -> Self {
        self.reveal = Some(target);
        self
    }
}

impl IntoElement for MonitorTextarea {
    type Element = Self;
    /// 直接参与原生元素生命周期，布局与可访问属性仍来自内部真实 Textarea。
    fn into_element(self) -> Self {
        self
    }
}

/// 预绘制采样描边和焦点滚动请求，不修改命中几何或编辑器状态。
pub(super) struct PaintState {
    border: Rgba,
    running: bool,
}

impl Element for MonitorTextarea {
    type RequestLayoutState = (AnyElement, Entity<BorderTransition>, Option<Entity<RevealState>>);
    type PrepaintState = PaintState;

    /// 实体 ID 隔离相邻输入，也使卸载时的动画状态按原生 keyed-state 生命周期释放。
    fn id(&self) -> Option<ElementId> {
        Some(("monitor-textarea", self.input.entity_id()).into())
    }
    /// 本地包装无额外检查器来源，内层 Kit 保留原生控件信息。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    /// Small 的原生内距为8/2，外层增加4/3.5，合计12/5.5；透明边框保留真实1px占位。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let owner = window.use_keyed_state("monitor-textarea-border", cx, |_, _| {
            BorderTransition::new()
        });
        let mut inner = Textarea::new(&self.input)
            .appearance(false)
            .with_size(Size::Small)
            .w_full()
            .h(px(56.4))
            .border_1()
            .border_color(transparent_black())
            .rounded(px(12.))
            .px(px(4.))
            .py(px(3.5))
            .text_size(px(14.))
            .font_weight(FontWeight::NORMAL)
            .line_height(relative(1.55))
            .text_color(rgb(if self.dark { 0xc9c9c9 } else { 0x000000 }))
            .aria_label(self.label.clone())
            .into_any_element();
        let layout = inner.request_layout(window, cx);
        // Kit 在原生布局时投影全局编辑色；此处在 TextElement 预绘制前完成局部覆盖。
        let style = editor_style::project(self.dark, cx);
        self.input
            .update(cx, |input, _| input.set_editor_style(style));
        let reveal = self.reveal.as_ref().map(|_| {
            window.use_keyed_state("monitor-textarea-reveal", cx, |_, _| RevealState::new())
        });
        (layout, (inner, owner, reveal))
    }

    /// 每帧读取真实输入焦点，避免只有 editor 通知时父 Shell 的边框状态滞后。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> PaintState {
        let focused = self.input.focus_handle(cx).is_focused(window);
        let target = rgb(match (self.dark, focused) {
            (false, false) => 0xced4da,
            (false, true) => 0xc43b2d,
            (true, false) => 0x424242,
            (true, true) => 0xd95a44,
        });
        let now = cx.background_executor().now();
        let (border, running) = request
            .1
            .update(cx, |motion, _| motion.sample(target, now));
        request.0.prepaint(window, cx);
        if let (Some(target), Some(state)) = (&self.reveal, &request.2) {
            reveal::observe(state, &self.input, target, bounds, window, cx);
        }
        PaintState { border, running }
    }

    /// 背景与单层描边在原生编辑器下方绘制；appearance(false) 消除 Kit 的额外焦点环。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        paint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_quad(quad(
            bounds,
            px(12.),
            rgb(if self.dark { 0x2e2e2e } else { 0xffffff }),
            Edges::all(px(1.)),
            paint.border,
            BorderStyle::Solid,
        ));
        if paint.running
            && window.is_visible()
            && !bounds.intersect(&window.content_mask().bounds).is_empty()
        {
            window.request_animation_frame();
        }
        request.0.paint(window, cx);
    }
}
