//! 浏览器式焦点滚动使用完整视口与真实 caret；回调不拥有业务状态或第二套文本。

use crate::app::{Page, PageScroll};
use gpui_kit::{App, Bounds, Context, Entity, FocusHandle, Focusable, Pixels, Point, Size, WeakEntity, Window, px};
use gpui_kit::component::input::TextareaState;
use super::super::state::Route;
use loki_metris_core::{AiTool, HookBehavior};

/// 真实图片字段的两种原生按钮，不增加第二套控件业务状态。
#[derive(Clone, Copy)]
pub(in crate::app::monitor) enum PickerFocus { Clear, Trigger }

/// 在途回调只保留弱编辑身份或值类型字段身份；原生 FocusHandle 由 keyed 状态拥有。
#[derive(Clone)]
enum Subject {
    /// 文案输入框。
    Textarea(WeakEntity<TextareaState>),
    /// 图片选择器按钮。
    Picker { tool: AiTool, behavior: HookBehavior, kind: PickerFocus },
}

impl Subject {
    /// 文本仍使用真实编辑实体，按钮仍使用绑定到原生 Button 的同一 handle。
    fn focus(&self, state: &Entity<RevealState>, cx: &App) -> Option<FocusHandle> {
        match self {
            Self::Textarea(input) => Some(input.upgrade()?.focus_handle(cx)),
            Self::Picker { .. } => state.read(cx).button_focus.clone(),
        }
    }
    /// 原生 caret 布局已经包含真实行号、选择端和内部编辑滚动，不估算固定字高。
    fn anchor(&self, state: &Entity<RevealState>, cx: &App) -> Option<Bounds<Pixels>> {
        match self {
            Self::Textarea(input) => input.upgrade()?.read(cx).cursor_layout().map(|layout| layout.0),
            Self::Picker { .. } => Some(state.read(cx).bounds),
        }
    }
}

/// 原请求的焦点、页面和手动滚动权限不因几何重试而重新取得。
#[derive(Clone, Copy)]
struct RevealLease {
    /// 请求时的页面代次。
    page_revision: u64,
    /// 请求时的手动滚动代次；用户滚动后租约失效。
    manual_revision: u64,
    /// 请求时的焦点代次。
    focus_generation: u64,
    /// 请求时的滚动偏移。
    offset: Point<Pixels>,
    /// 请求时的视口尺寸。
    viewport: Size<Pixels>,
    /// 请求时的缩放比例。
    scale: f32,
}

impl RevealLease {
    /// 只接受新几何产生的原生 Y 夹紧；不重新取得用户滚动、导航或焦点权限。
    fn renew_geometry(&self, current: Self, maximum: Pixels) -> Option<Self> {
        let same_owner = self.page_revision == current.page_revision
            && self.manual_revision == current.manual_revision
            && self.focus_generation == current.focus_generation;
        let native_clamp = (self.viewport != current.viewport || self.scale != current.scale)
            && current.offset.x == self.offset.x
            && current.offset.y == self.offset.y.clamp(-maximum, px(0.));
        (same_owner && (self.offset == current.offset || native_clamp)).then_some(Self {
            offset: current.offset, viewport: current.viewport, scale: current.scale, ..*self
        })
    }
}

/// keyed 生命周期内记录实际焦点边沿和最近几何，不持续追赶手动滚动。
pub(in crate::app::monitor) struct RevealState {
    /// 输入框当前持有焦点。
    focused: bool,
    /// 获得焦点的次数；每次焦点边沿递增。
    focus_generation: u64,
    /// 几何修订号。
    revision: u64,
    /// 最近一次绘制的边界。
    bounds: Bounds<Pixels>,
    /// 几何未就绪时等待重试的滚入租约。
    retry: Option<RevealLease>,
    /// 选择器按钮的焦点句柄。
    button_focus: Option<FocusHandle>,
}

impl RevealState {
    /// 第一次挂载仍采样真实焦点；空 bounds 在首次 prepaint 后才参与回调。
    pub(super) fn new() -> Self {
        Self { focused: false, focus_generation: 0, revision: 0, bounds: Bounds::default(), retry: None, button_focus: None }
    }

    /// 只创建将交给原生 Button::track_focus 的真实 handle，状态仍按控件卸载释放。
    pub(in crate::app::monitor) fn button(cx: &mut Context<Self>) -> Self {
        Self { button_focus: Some(cx.focus_handle()), ..Self::new() }
    }

    /// 外层将同一 handle 交给原生 Button，不能把包装元素变为额外焦点节点。
    pub(in crate::app::monitor) fn button_focus(&self) -> FocusHandle {
        self.button_focus.as_ref().unwrap().clone()
    }
}

impl PageScroll {
    /// 页面、子页或 Agent 真实变化立即淘汰旧回调，同帧返回也不能复活。
    pub(in crate::app) fn invalidate(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    /// 页面真实滚轮只撤销未完成滚入，偏移仍由原生 Div 或内部编辑器拥有。
    pub(in crate::app) fn cancel_pending_reveal(&self) {
        self.manual_revision.set(self.manual_revision.get().wrapping_add(1));
    }

    /// 回调时直接核对壳层事实；不能只依赖上一次已绘制页面的快照。
    fn still_owns(&self, subject: &Subject, window: &Window, cx: &App) -> bool {
        let Some(owner) = self.owner.upgrade() else { return false; };
        let shell = owner.read(cx);
        let state = &shell.monitor.catalog;
        if shell.native_window.window_id() != window.window_handle().window_id()
            || shell.page != Page::Monitor
            || shell.monitor.route != Route::Management
            || shell.agent_settings.loading
            || shell.agent_settings.error.is_some()
            || !state.profiles_loaded
            || state.profiles_error.is_some()
            || state.images_error.is_some()
            || shell.release_notes_modal.is_some()
            || shell.skin_modal_open()
            || shell.source_modal_open()
        { return false; }
        let Some(tool) = shell.monitor_tool() else { return false; };
        let Some(editor) = state.editors.iter().find(|editor| editor.draft.tool == tool) else { return false; };
        match subject {
            Subject::Textarea(input) => input.upgrade().is_some_and(|input| editor.inputs.as_ref()
                .is_some_and(|inputs| inputs.iter().any(|candidate| candidate.entity_id() == input.entity_id()))),
            Subject::Picker { tool: owner, behavior, kind } => *owner == tool && !state.images_loading && !state.uploading
                && editor.draft.hooks.iter().find(|hook| hook.behavior == *behavior)
                    .is_some_and(|hook| matches!(kind, PickerFocus::Trigger) || !hook.image.trim().is_empty()),
        }
    }
}

/// prepaint 只登记请求，下一帧确认仍挂载、仍获焦且滚动没有被用户接管后才移动。
pub(super) fn observe(
    state: &Entity<RevealState>,
    input: &Entity<TextareaState>,
    target: &PageScroll,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    observe_subject(state, Subject::Textarea(input.downgrade()), target, bounds, window, cx);
}

/// 图片字段复用同一租约与下一帧机制，原生 Button 保留键盘、点击和辅助树。
#[expect(clippy::too_many_arguments, reason = "图片字段身份的三个组成部分与滚动目标、边界和 GPUI 上下文彼此独立")]
pub(in crate::app::monitor) fn observe_picker(
    state: &Entity<RevealState>, tool: AiTool, behavior: HookBehavior, kind: PickerFocus,
    target: &PageScroll, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App,
) {
    observe_subject(state, Subject::Picker { tool, behavior, kind }, target, bounds, window, cx);
}

/// 只有真实焦点新进入才取得一次权限；页面、用户滚轮和焦点世代仍分别撤销请求。
fn observe_subject(
    state: &Entity<RevealState>, subject: Subject, target: &PageScroll,
    bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App,
) {
    let focused = subject.focus(state, cx).is_some_and(|focus| focus.is_focused(window));
    let focus_generation = window.focus_generation();
    let current = RevealLease {
        page_revision: target.revision.get(),
        manual_revision: target.manual_revision.get(),
        focus_generation,
        offset: target.handle.offset(),
        viewport: window.viewport_size(),
        scale: window.scale_factor(),
    };
    let request = state.update(cx, |state, _| {
        state.bounds = bounds;
        let entered = state.focused != focused || (focused && state.focus_generation != focus_generation);
        let lease = if entered {
            state.focused = focused;
            state.focus_generation = focus_generation;
            state.retry = None;
            focused.then_some(current)
        } else {
            state.retry.take().filter(|_| focused)
                .and_then(|old| old.renew_geometry(current, target.handle.max_offset().y))
        };
        let lease = lease?;
        state.revision = state.revision.wrapping_add(1);
        Some((state.revision, lease))
    });
    let Some((revision, lease)) = request else { return; };
    let state = state.downgrade();
    let target = target.clone();
    window.on_next_frame(move |window, cx| {
        let Some(state) = state.upgrade() else { return; };
        if target.revision.get() != lease.page_revision
            || target.manual_revision.get() != lease.manual_revision
            || window.focus_generation() != lease.focus_generation
            || target.handle.offset() != lease.offset
            || !window.is_visible()
            || !subject.focus(&state, cx).is_some_and(|focus| focus.is_focused(window))
            || !target.still_owns(&subject, window, cx)
        { return; }
        if !state.read(cx).focused || state.read(cx).revision != revision { return; }
        if window.viewport_size() != lease.viewport || window.scale_factor() != lease.scale {
            state.update(cx, |state, _| state.retry = Some(lease));
            window.refresh();
            return;
        }
        let viewport = target.handle.bounds();
        if viewport.is_empty() { return; }
        let Some(bounds) = subject.anchor(&state, cx) else { return; };
        let delta = if bounds.top() < viewport.top() || bounds.bottom() > viewport.bottom() {
            viewport.center().y - bounds.center().y
        } else { px(0.) };
        let current = target.handle.offset();
        let y = (current.y + delta).clamp(-target.handle.max_offset().y, px(0.));
        if y != current.y {
            target.handle.set_offset(Point { x: current.x, y });
            window.refresh();
        }
    });
}
