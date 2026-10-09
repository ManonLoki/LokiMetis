//! 焦点首次进入时只滚动被遮挡部分；回调不拥有业务状态或第二套文本。

use crate::app::{Page, PageScroll};
use gpui_kit::{App, Bounds, Entity, EntityId, Focusable, Pixels, Point, Size, Window, px};
use gpui_kit::component::input::TextareaState;
use super::super::state::Route;

/// 原请求的焦点、页面和手动滚动权限不因几何重试而重新取得。
#[derive(Clone, Copy)]
struct RevealLease {
    page_revision: u64,
    manual_revision: u64,
    focus_generation: u64,
    offset: Point<Pixels>,
    viewport: Size<Pixels>,
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
    focused: bool,
    focus_generation: u64,
    revision: u64,
    bounds: Bounds<Pixels>,
    retry: Option<RevealLease>,
}

impl RevealState {
    /// 第一次挂载仍采样真实焦点；空 bounds 在首次 prepaint 后才参与回调。
    pub(super) fn new() -> Self {
        Self { focused: false, focus_generation: 0, revision: 0, bounds: Bounds::default(), retry: None }
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
    fn still_owns(&self, input: EntityId, window: &Window, cx: &App) -> bool {
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
        { return false; }
        let Some(tool) = shell.monitor_tool() else { return false; };
        state.editors.iter().find(|editor| editor.draft.tool == tool)
            .and_then(|editor| editor.inputs.as_ref())
            .is_some_and(|inputs| inputs.iter().any(|candidate| candidate.entity_id() == input))
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
    let focused = input.focus_handle(cx).is_focused(window);
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
    let input = input.downgrade();
    let target = target.clone();
    window.on_next_frame(move |window, cx| {
        let (Some(state), Some(input)) = (state.upgrade(), input.upgrade()) else { return; };
        if target.revision.get() != lease.page_revision
            || target.manual_revision.get() != lease.manual_revision
            || window.focus_generation() != lease.focus_generation
            || target.handle.offset() != lease.offset
            || !window.is_visible()
            || !input.focus_handle(cx).is_focused(window)
            || !target.still_owns(input.entity_id(), window, cx)
        { return; }
        if !state.read(cx).focused || state.read(cx).revision != revision { return; }
        if window.viewport_size() != lease.viewport || window.scale_factor() != lease.scale {
            state.update(cx, |state, _| state.retry = Some(lease));
            window.refresh();
            return;
        }
        let viewport = target.handle.bounds();
        if viewport.is_empty() { return; }
        let bounds = state.read(cx).bounds;
        let delta = if bounds.size.height > viewport.size.height || bounds.top() < viewport.top() {
            viewport.top() - bounds.top()
        } else if bounds.bottom() > viewport.bottom() {
            viewport.bottom() - bounds.bottom()
        } else { px(0.) };
        let current = target.handle.offset();
        let y = (current.y + delta).clamp(-target.handle.max_offset().y, px(0.));
        if y != current.y {
            target.handle.set_offset(Point { x: current.x, y });
            window.refresh();
        }
    });
}
