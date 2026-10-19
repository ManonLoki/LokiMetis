//! 图片字段使用原生 Button 的同一焦点 handle；包装只观察首次焦点与真实布局。

use super::textarea::reveal::{self, PickerFocus, RevealState};
use crate::app::PageScroll;
use gpui_kit::{
    AnyElement, App, Bounds, Element, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window, base::Button,
};
use loki_metris_core::{AiTool, HookBehavior};

/// 图片字段包装没有辅助角色、键盘动作或文本状态，内部 Button 独占交互。
struct PickerFocusElement {
    /// 被包装的选择器按钮。
    child: AnyElement,
    /// 滚入状态。
    state: Entity<RevealState>,
    /// 页面滚动。
    target: PageScroll,
    /// 所属 Agent。
    tool: AiTool,
    /// 所属行为。
    behavior: HookBehavior,
    /// 选择器中的焦点种类。
    kind: PickerFocus,
}

/// keyed handle 同时交给原生 Button 和弱租约，tool/behavior/kind 共同隔离字段身份。
pub(super) fn picker_button(
    button: Button, target: PageScroll, tool: AiTool, behavior: HookBehavior, kind: PickerFocus,
    window: &mut Window, cx: &mut App,
) -> AnyElement {
    let kind_key = match kind { PickerFocus::Clear => "clear", PickerFocus::Trigger => "trigger" };
    let key = format!("monitor-picker-focus-{}-{}-{kind_key}", loki_metris_core::ai_tool_name(tool), super::management::behavior_key(behavior));
    let state = window.use_keyed_state(key, cx, |_, cx| RevealState::button(cx));
    let focus = state.read(cx).button_focus();
    PickerFocusElement { child: button.track_focus(&focus).into_any_element(), state, target, tool, behavior, kind }.into_any_element()
}

impl IntoElement for PickerFocusElement {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 保留内部控件的原生 Element 生命周期。
    fn into_element(self) -> Self { self }
}

impl Element for PickerFocusElement {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段无需向绘制阶段传递状态。
    type PrepaintState = ();
    /// 原 Button 拥有稳定 ID，包装不添加身份或辅助节点。
    fn id(&self) -> Option<gpui_kit::ElementId> { None }
    /// 本地包装没有额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }
    /// 直接返回原生 Button 布局，不能增加宽高或额外点击区域。
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    /// 原生控件先登记真实焦点和命中几何，再观察同一个 handle 的进入边沿。
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.prepaint(window, cx);
        reveal::observe_picker(&self.state, self.tool, self.behavior, self.kind, &self.target, bounds, window, cx);
    }
    /// 绘制全部交回原 Button，不增加代理事件或焦点环。
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        self.child.paint(window, cx);
    }
}
