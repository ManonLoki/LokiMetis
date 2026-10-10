use crate::{Context, Entity, IntoElement, ParentElement, Render, RequestFrameOptions, StyleRefinement, Styled, TestAppContext, div};
use std::{cell::Cell, rc::Rc};

/// 真正可缓存的子Entity；强制重绘必须增加次数，纯呈现则复用现有Scene。
struct CountedChild(Rc<Cell<usize>>);

impl Render for CountedChild {
    /// 实际进入Entity渲染时计数，避免只观察调度标志的假阳性。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.set(self.0.get() + 1);
        div().child("frame demand")
    }
}

/// 根视图持有缓存子节点，便于区分真正强制渲染与重复present。
struct CachedRoot(Entity<CountedChild>);

impl Render for CachedRoot {
    /// 同一子Entity使用真实GPUI缓存，普通干净帧不得重新render。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.0.clone().cached(StyleRefinement::default().size(px(100.))))
    }
}

/// 用生产模式禁用测试自动flush绘制，所有结果均经过真实帧请求入口。
fn open_cached(cx: &mut TestAppContext) -> (AnyWindowHandle, Rc<Cell<usize>>) {
    cx.app.borrow_mut().mode = crate::app::GpuiMode::Production;
    let renders = Rc::new(Cell::new(0));
    let handle = cx.add_window({
        let renders = renders.clone();
        move |_, cx| CachedRoot(cx.new(|_| CountedChild(renders)))
    });
    cx.update_window(handle.into(), |_, window, _| window.active.set(true)).unwrap();
    settle(cx, handle.into());
    (handle.into(), renders)
}

/// 有界消费真实平台调度及测试compositor回执，最终必须完全停派。
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    let platform = cx.test_window(handle);
    for _ in 0..8 {
        if !platform.simulate_scheduled_frame() { break; }
    }
    assert!(!platform.frame_scheduled(), "静态窗口应在有界帧数内停派");
    cx.update_window(handle, |_, window, _| {
        assert!(!window.invalidator.is_dirty());
        assert!(!window.needs_present.get());
        assert!(window.next_frame_callbacks.borrow().is_empty());
    }).unwrap();
}

/// 使用真实线程绘制scope重入；App只读借用保证错误越过早退时立即暴露借用冲突。
fn reenter(cx: &mut TestAppContext, handle: AnyWindowHandle, requests: &[RequestFrameOptions]) {
    let app = cx.app.clone();
    let platform = cx.test_window(handle);
    let app_read = app.borrow();
    let scope = ElementArenaScope::enter(&app_read.element_arena);
    for &request in requests { platform.simulate_frame_request(request); }
    let clear = scope.exit(&app_read.element_arena);
    drop(app_read);
    clear.clear(&mut app.borrow_mut());
}

/// 强制与呈现需求分别以及多次合并都跨越重入保留；服务后再次静态停派。
#[gpui::test]
fn reentrant_flags_survive_and_wake_without_app_borrow(cx: &mut TestAppContext) {
    let forced = RequestFrameOptions { force_render: true, ..Default::default() };
    let presented = RequestFrameOptions { require_presentation: true, ..Default::default() };
    let both = RequestFrameOptions { force_render: true, require_presentation: true, ..Default::default() };
    for (requests, expect_render) in [
        (vec![forced], true), (vec![presented], false), (vec![both], true),
        (vec![forced, presented, forced], true), (vec![presented, presented], false),
    ] {
        let (handle, renders) = open_cached(cx);
        let platform = cx.test_window(handle);
        let before = renders.get();
        let wakes = platform.frame_wake_count();
        reenter(cx, handle, &requests);
        assert_eq!(renders.get(), before, "绘制scope内不得重入render");
        assert!(platform.frame_wake_count() > wakes, "被消费的帧信号必须重新唤醒");
        // 默认TestWindow的waker只计数；此处交付已经观察到的唤醒，不伪称其自动排队。
        platform.schedule_frame();
        assert!(platform.simulate_scheduled_frame());
        assert_eq!(renders.get(), before + usize::from(expect_render), "延期force必须恰好重绘一次真实子Entity");
        assert!(platform.frame_scheduled(), "纯呈现也必须实际调用平台draw并等待compositor");
        settle(cx, handle);
        let served = renders.get();
        platform.simulate_frame_request(RequestFrameOptions::default());
        assert_eq!(renders.get(), served, "延期标志应只消费一次");
        assert!(!platform.frame_scheduled());
    }
}

/// 高频输入产生真实一秒期限，期限内显式续帧唤醒，过期后允许停派。
#[gpui::test]
fn high_rate_input_rearms_until_its_real_deadline(cx: &mut TestAppContext) {
    let (handle, renders) = open_cached(cx);
    let platform = cx.test_window(handle);
    cx.update_window(handle, |_, window, _| {
        let mut tracker = window.input_rate_tracker.borrow_mut();
        for _ in 0..6 { tracker.record_input(); }
        assert_eq!(tracker.timestamps.len(), 6, "夹具记录必须位于原生100ms窗口内");
        assert_eq!(tracker.sustain_until.duration_since(*tracker.timestamps.last().unwrap()), Duration::from_secs(1));
        assert!(tracker.is_high_rate());
    }).unwrap();
    let before = renders.get();
    for _ in 0..2 {
        let wakes = platform.frame_wake_count();
        platform.simulate_frame_request(RequestFrameOptions::default());
        assert!(platform.frame_wake_count() > wakes, "高频输入维持期必须显式续帧");
        assert!(platform.frame_scheduled());
        assert_eq!(renders.get(), before, "持续呈现不需要重复render");
    }
    cx.update_window(handle, |_, window, _| {
        // scheduler::Instant是墙钟；直接设置真实期限边界，避免advance_clock假绿。
        window.input_rate_tracker.borrow_mut().sustain_until = Instant::now();
    }).unwrap();
    settle(cx, handle);
    let wakes = platform.frame_wake_count();
    platform.simulate_frame_request(RequestFrameOptions::default());
    assert_eq!(platform.frame_wake_count(), wakes);
    assert_eq!(renders.get(), before);
}

/// 有限动画通过真实request_animation_frame逐帧失效，最终清理后不能持续空转。
struct FiniteAnimation(Rc<Cell<usize>>);

impl Render for FiniteAnimation {
    /// 每次实际render申请下一帧，第四帧完成动画；不模拟墙钟时长。
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.set(self.0.get() + 1);
        if self.0.get() < 4 { window.request_animation_frame(); }
        div().child("finite animation")
    }
}

/// 每次动画需求均经平台消费，四次实际绘制后连同compositor回执完全停派。
#[gpui::test]
fn finite_animation_runs_and_then_parks(cx: &mut TestAppContext) {
    cx.app.borrow_mut().mode = crate::app::GpuiMode::Production;
    let renders = Rc::new(Cell::new(0));
    let handle = cx.add_window({ let renders = renders.clone(); move |_, _| FiniteAnimation(renders) });
    cx.update_window(handle.into(), |_, window, _| window.active.set(true)).unwrap();
    let platform = cx.test_window(handle.into());
    for _ in 0..8 {
        if !platform.simulate_scheduled_frame() { break; }
    }
    assert_eq!(renders.get(), 4);
    settle(cx, handle.into());
    assert!(!platform.simulate_scheduled_frame());
}
