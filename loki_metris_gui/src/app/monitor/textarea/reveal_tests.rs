//! 真实窗口验证焦点进入的最小滚动；回归不读取 reveal 私有时间线或复制编辑状态。

use crate::app::{Page, Shell, PageScroll, monitor::state::Route};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, InputEvent as _, IntoElement, Pixels, Render,
    ScrollDelta, ScrollWheelEvent, TestAppContext, Window, WindowBounds, WindowOptions, div, point, px, size,
    component::input::TextareaState, prelude::*, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, AiTool, HookBehavior, public_monitor_ai_tools};

/// 用完整生产 Shell 的四字段建立隔离偏好，不读真实磁盘或用户剪贴板。
fn setup(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>, crate::app::tests::FinishWriter) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 400.);
    shell.update(cx, |shell, cx| {
        shell.page = Page::Monitor;
        shell.monitor.route = Route::Management;
        shell.monitor.catalog.accept_profiles(public_monitor_ai_tools().map(AiProfileDraft::default_for).collect());
        shell.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    (handle, shell, writer)
}

/// 读取实际 catalog 的同一原生输入，身份必须与生产消费者匹配。
fn input(shell: &Entity<Shell>, index: usize, cx: &gpui_kit::App) -> Entity<TextareaState> {
    shell.read(cx).monitor.catalog.editors[0].inputs.as_ref().unwrap()[index].clone()
}

/// 设备像素吸附最多改变一条边一个设备像素。
fn near(window: &Window, actual: Pixels, expected: Pixels) {
    assert!((actual - expected).abs() < px(1. / window.scale_factor()), "{actual:?} vs {expected:?}");
}

/// 根据公开实际几何计算浏览器式最小滚动目标，拒绝固定跳转或全部顶对齐。
fn expected_y(target: Bounds<Pixels>, viewport: Bounds<Pixels>, offset: Pixels, maximum: Pixels) -> Pixels {
    let delta = if target.top() < viewport.top() { viewport.top() - target.top() }
        else if target.bottom() > viewport.bottom() { viewport.bottom() - target.bottom() }
        else { px(0.) };
    (offset + delta).clamp(-maximum, px(0.))
}

/// 通过真正键盘绑定穿过中间按钮，不能直接伪造第二字段的 focused 标志。
fn tab_to(window: &mut Window, target: &Entity<TextareaState>, reverse: bool, cx: &mut gpui_kit::App) {
    for _ in 0..32 {
        window.press(if reverse { "shift-tab" } else { "tab" }, cx);
        if target.focus_handle(cx).is_focused(window) { return; }
    }
    panic!("原生 Tab 顺序未抵达目标输入");
}

/// 同排 Tab 不移动页面；下一排首次获焦时最小滚入，Shift+Tab 回上一排。
#[gpui_kit::test]
fn shell_textarea_tab_reveals_next_field_and_shift_tab_returns(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let idle = input(&shell, 0, cx);
        let running = input(&shell, 1, cx);
        let asking = input(&shell, 2, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        idle.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let first_row = scroll.offset();
        tab_to(window, &running, false, cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), first_row, "同排字段已可见时不能跳转");
        tab_to(window, &asking, false, cx);
        let target = window.find("monitor-content-input-asking").bounds();
        let viewport = scroll.bounds();
        let before = scroll.offset();
        assert!(target.bottom() > viewport.bottom(), "矮窗中的下一字段应真实离屏");
        let expected = expected_y(target, viewport, before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        assert_eq!(scroll.offset().x, before.x);
        let visible = window.find("monitor-content-input-asking").bounds();
        assert!(visible.top() >= viewport.top() - px(0.51));
        assert!(visible.bottom() <= viewport.bottom() + px(0.51));
        let down = scroll.offset();
        tab_to(window, &running, true, cx);
        let target = window.find("monitor-content-input-running").bounds();
        assert!(target.top() < viewport.top(), "反向目标应在已滚动视口上方");
        let expected = expected_y(target, viewport, down.y, scroll.max_offset().y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        assert!(scroll.offset().y > down.y);
        let visible = window.find("monitor-content-input-running").bounds();
        assert!(visible.top() >= viewport.top() - px(0.51));
        assert!(visible.bottom() <= viewport.bottom() + px(0.51));
        let restored = scroll.offset();
        tab_to(window, &idle, true, cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), restored);
        assert!(idle.focus_handle(cx).is_focused(window));
    }).unwrap();
}

/// 页面、子页、Agent 与输入卸载均在下一次 render 之前失效，旧输入仍存活也不能滚动。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_stale_page_agent_and_unmounted_input(cx: &mut TestAppContext) {
    for change in 0..4 {
        let (handle, shell, _writer) = setup(cx);
        cx.update_window(handle, |_, window, cx| {
            let old = input(&shell, 3, cx);
            let scroll = shell.read(cx).page_scroll.handle.clone();
            old.update(cx, |input, cx| input.focus(window, cx));
            window.render_frame(cx);
            let before = scroll.offset();
            assert!(window.find("monitor-content-input-error").bounds().bottom() > scroll.bounds().bottom());
            shell.update(cx, |shell, _| match change {
                0 => shell.page = Page::Settings,
                1 => shell.monitor.route = Route::Hooks,
                2 => shell.monitor.catalog.selected = Some(AiTool::ClaudeCode),
                3 => shell.monitor.catalog.editors[0].inputs = None,
                _ => unreachable!(),
            });
            // 尚未重新绘制；不能拿上帧页面或旧输入仍存活当作有效租约。
            assert!(old.focus_handle(cx).is_focused(window));
            window.simulate_next_frame(cx);
            assert_eq!(scroll.offset(), before, "陈旧类型 {change} 不能改变滚动");
        }).unwrap();
    }
}

/// 每种真实导航中心入口都在两次 render 之间往返，最终事实相同也不能复活旧回调。
fn round_trip(cx: &mut TestAppContext, change: usize) {
        let (handle, shell, _writer) = setup(cx);
        cx.update_window(handle, |_, window, cx| {
            let old = input(&shell, 3, cx);
            let scroll = shell.read(cx).page_scroll.handle.clone();
            old.update(cx, |input, cx| input.focus(window, cx));
            window.render_frame(cx);
            let before = scroll.offset();
            assert!(window.find("monitor-content-input-error").bounds().bottom() > scroll.bounds().bottom());
            shell.update(cx, |shell, cx| match change {
                0 => {
                    shell.navigate(Page::Settings, cx);
                    shell.navigate(Page::Monitor, cx);
                }
                1 => {
                    shell.select_monitor_route(Route::Hooks, cx);
                    shell.select_monitor_route(Route::Management, cx);
                }
                2 => {
                    shell.select_monitor_tool(AiTool::ClaudeCode, cx);
                    shell.select_monitor_tool(AiTool::Codex, cx);
                }
                _ => unreachable!(),
            });
            let current = shell.read(cx);
            assert_eq!(current.page, Page::Monitor);
            assert!(current.monitor.route == Route::Management);
            assert_eq!(current.monitor_tool(), Some(AiTool::Codex));
            assert_eq!(input(&shell, 3, cx).entity_id(), old.entity_id());
            assert!(old.focus_handle(cx).is_focused(window));
            window.simulate_next_frame(cx);
            assert_eq!(scroll.offset(), before, "往返类型 {change} 不能复活旧焦点请求");
        }).unwrap();
}

/// 主页面先离开再返回且未重绘，原来的输入实体仍存活也不能执行旧请求。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_page_round_trip(cx: &mut TestAppContext) {
    round_trip(cx, 0);
}

/// 真实 Monitor 子页入口往返必须撤销尚未执行的滚入。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_route_round_trip(cx: &mut TestAppContext) {
    round_trip(cx, 1);
}

/// 真实 Agent tab 入口往返必须撤销尚未执行的滚入。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_tool_round_trip(cx: &mut TestAppContext) {
    round_trip(cx, 2);
}

/// 同页重绘与同一 Route/Agent 的真实入口不撤销合法请求，也不逐帧重新排队。
#[gpui_kit::test]
fn shell_textarea_reveal_survives_same_selection_and_render_refresh(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Monitor, cx);
            shell.select_monitor_route(Route::Management, cx);
            shell.select_monitor_tool(AiTool::Codex, cx);
        });
        for _ in 0..3 { window.render_frame(cx); }
        assert_eq!(scroll.offset(), before, "同页重绘只更新几何，不提前滚动");
        let target = window.find("monitor-content-input-error").bounds();
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let revealed = scroll.offset();
        for _ in 0..3 {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
        }
        assert_eq!(scroll.offset(), revealed);
    }).unwrap();
}

/// 已提交启用集合可异步改变有效 Agent；回到原集合也不能复活此前焦点请求。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_accepted_agent_settings_round_trip(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        shell.update(cx, |shell, cx| {
            shell.accept_agent_settings(crate::monitor::settings::MonitorSettings {
                enabled_ai_tools: vec![AiTool::ClaudeCode], ..Default::default()
            }, cx);
            assert_eq!(shell.monitor_tool(), Some(AiTool::ClaudeCode));
            shell.accept_agent_settings(crate::monitor::settings::MonitorSettings {
                enabled_ai_tools: public_monitor_ai_tools().collect(), ..Default::default()
            }, cx);
            assert_eq!(shell.monitor_tool(), Some(AiTool::Codex));
        });
        assert_eq!(input(&shell, 3, cx).entity_id(), focused.entity_id());
        assert!(focused.focus_handle(cx).is_focused(window));
        window.simulate_next_frame(cx);
        assert_eq!(scroll.offset(), before);
    }).unwrap();
}

/// 启用集合收缩但当前有效 Agent 未变时保留合法滚入，不能把每次回执当作切换。
#[gpui_kit::test]
fn shell_textarea_reveal_survives_accepted_settings_with_same_effective_agent(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        let target = window.find("monitor-content-input-error").bounds();
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        shell.update(cx, |shell, cx| {
            shell.accept_agent_settings(crate::monitor::settings::MonitorSettings {
                enabled_ai_tools: vec![AiTool::Codex], ..Default::default()
            }, cx);
        });
        assert_eq!(shell.read(cx).monitor_tool(), Some(AiTool::Codex));
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
    }).unwrap();
}

/// 新焦点尚未 prepaint 时旧请求也不能移动页面。
#[gpui_kit::test]
fn shell_textarea_reveal_discards_focus_moved_before_next_frame(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let old = input(&shell, 3, cx);
        let next = input(&shell, 0, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        old.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        next.update(cx, |input, cx| input.focus(window, cx));
        window.simulate_next_frame(cx);
        assert_eq!(scroll.offset(), before);
        assert!(next.focus_handle(cx).is_focused(window));
    }).unwrap();
}

/// 焦点稳定后真实滚轮、编辑通知与重复绘帧都不能重新接管用户的滚动位置。
#[gpui_kit::test]
fn shell_textarea_reveal_does_not_fight_manual_scrolling_or_editing(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let pending = scroll.offset();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-80.))), cx);
        window.render_frame(cx);
        let manual = scroll.offset();
        assert_ne!(manual, pending, "真实滚轮必须改变页面位置");
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), manual, "用户滚轮优先于在途焦点请求");
        window.input("typing", cx);
        assert!(focused.read(cx).value().as_ref().ends_with("typing"));
        for _ in 0..4 {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
        }
        assert_eq!(scroll.offset(), manual, "编辑与光标动画不能持续滚入");
    }).unwrap();
}

/// 真实双轴滚动容器只用来覆盖 X 保留与边界，文本域和所属 Shell 输入仍为生产实现。
struct BothAxes {
    _shell: Entity<Shell>,
    scroll: PageScroll,
    input: Entity<TextareaState>,
}

impl Render for BothAxes {
    /// 固定大内容建立非零 X，避免在仅纵向页面上用恒为零的 X 虚通过。
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        div().id("reveal-both-axes").size_full().overflow_scroll().track_scroll(&self.scroll.handle)
            .child(div().w(px(1800.)).h(px(1600.)).flex().flex_col().flex_shrink_0()
                .child(div().h(px(1543.6)).flex_shrink_0())
                .child(super::MonitorTextarea::new(&self.input, false, "Native focus reveal")
                    .reveal_into(self.scroll.clone())))
    }
}

/// 自建窗口同样在 panic 时回收当前测试拥有的内存写入队列。
struct Finish(crate::preferences::Writer);

impl Drop for Finish {
    /// 只等待隔离偏好，不访问真实磁盘。
    fn drop(&mut self) { self.0.finish(); }
}

/// 最后字段贴近内容底边时不越过页面最大偏移，Y 变更保留实际非零横向滚动。
#[gpui_kit::test]
fn native_textarea_reveal_keeps_nonzero_x_and_clamps_page_edges(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let preferences = crate::preferences::Preferences::default();
    let writer = crate::preferences::Writer::in_memory(preferences.clone());
    let _finish = Finish(writer.clone());
    let mut scroll = None;
    let mut native = None;
    let (handle, _view) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(960.), px(640.))))),
        ..Default::default()
    }, cx, |window, cx| {
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(preferences, writer, window, cx);
            shell.agent_settings.value = Some(crate::monitor::settings::MonitorSettings {
                enabled_ai_tools: public_monitor_ai_tools().collect(), ..Default::default()
            });
            shell.page = Page::Monitor;
            shell.monitor.route = Route::Management;
            shell.monitor.catalog.accept_profiles(public_monitor_ai_tools().map(AiProfileDraft::default_for).collect());
            shell.sync_monitor_inputs(window, cx);
            shell
        });
        let page_scroll = shell.read(cx).page_scroll.clone();
        let input = shell.read(cx).monitor.catalog.editors[0].inputs.as_ref().unwrap()[0].clone();
        scroll = Some(page_scroll.handle.clone());
        native = Some(input.clone());
        cx.new(|_| BothAxes { _shell: shell, scroll: page_scroll, input })
    }).unwrap());
    let scroll = scroll.unwrap();
    let native = native.unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        scroll.set_offset(point(px(-140.), px(0.)));
        window.render_frame(cx);
        assert_eq!(scroll.offset().x, px(-140.));
        native.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset().x, px(-140.));
        near(window, scroll.offset().y, -scroll.max_offset().y);
        assert!(scroll.offset().y <= px(0.));
        assert!(scroll.offset().y >= -scroll.max_offset().y);
    }).unwrap();
}

/// 原生焦点往返不经中间绘帧，手动离屏后的再次进入仍须排队滚入。
#[gpui_kit::test]
fn shell_textarea_focus_round_trip_between_renders_reveals_manual_offscreen_input(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(10_000.))), cx);
        let before = scroll.offset();
        assert!(window.find("monitor-content-input-error").bounds().bottom() > scroll.bounds().bottom());
        assert!(focused.focus_handle(cx).is_focused(window));
        window.focus_next(cx);
        assert!(!focused.focus_handle(cx).is_focused(window));
        window.focus_prev(cx);
        assert!(focused.focus_handle(cx).is_focused(window));
        window.render_frame(cx);
        assert_eq!(scroll.offset(), before, "新焦点只在下一帧滚动");
        let target = window.find("monitor-content-input-error").bounds();
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let visible = window.find("monitor-content-input-error").bounds();
        assert!(visible.top() >= scroll.bounds().top() - px(0.51));
        assert!(visible.bottom() <= scroll.bounds().bottom() + px(0.51));
    }).unwrap();
}

/// 已有请求后发生原生焦点往返，旧回调立即过期，下一次 prepaint 才登记新请求。
#[gpui_kit::test]
fn shell_textarea_focus_round_trip_discards_pending_then_requeues_after_prepaint(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        window.focus_next(cx);
        assert!(!focused.focus_handle(cx).is_focused(window));
        window.focus_prev(cx);
        assert!(focused.focus_handle(cx).is_focused(window));
        window.simulate_next_frame(cx);
        assert_eq!(scroll.offset(), before, "旧代数的 callback 不能因同一输入再次获焦而复活");
        window.render_frame(cx);
        let target = window.find("monitor-content-input-error").bounds();
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        assert_eq!(scroll.offset(), before);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let visible = window.find("monitor-content-input-error").bounds();
        assert!(visible.top() >= scroll.bounds().top() - px(0.51));
        assert!(visible.bottom() <= scroll.bounds().bottom() + px(0.51));
    }).unwrap();
}

/// 图库是同一文档内嵌面板；展开后真实 Tab 到其下文本域仍需滚入。
#[gpui_kit::test]
fn shell_textarea_inline_picker_tab_reveals_still_mounted_input(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let idle = input(&shell, 0, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        idle.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.click("monitor-picker-trigger-idle", cx);
        assert!(shell.read(cx).monitor.catalog.picker == Some(HookBehavior::Idle));
        assert!(window.try_find("monitor-picker-panel-idle").is_some());
        tab_to(window, &idle, false, cx);
        assert!(shell.read(cx).monitor.catalog.picker == Some(HookBehavior::Idle));
        let before = scroll.offset();
        let target = window.find("monitor-content-input-idle").bounds();
        assert!(target.bottom() > scroll.bounds().bottom(), "内嵌图库必须真实把文本域推到视口下方");
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let visible = window.find("monitor-content-input-idle").bounds();
        assert!(visible.top() >= scroll.bounds().top() - px(0.51));
        assert!(visible.bottom() <= scroll.bounds().bottom() + px(0.51));
    }).unwrap();
}

/// 单独测试实体接收真实 bounds 事件，避免持有 Shell 可变借用时执行租约校验。
struct GeometryObserver {
    _bounds: gpui_kit::Subscription,
}

/// 记录真实 resize 回调中、测试框架自动绘制之前的可观察滚动结果。
struct GeometryDelivery {
    before: gpui_kit::Point<Pixels>,
    after_callback: gpui_kit::Point<Pixels>,
    after_wheel: gpui_kit::Point<Pixels>,
}

/// 由真实 bounds observer 投递 callbacks，保证 resize→callbacks→fresh draw 的生产顺序。
fn geometry_observer(
    window: &mut Window,
    cx: &mut gpui_kit::App,
    scroll: gpui_kit::ScrollHandle,
    seen: std::rc::Rc<std::cell::RefCell<Option<GeometryDelivery>>>,
    manual: bool,
) -> Entity<GeometryObserver> {
    let old_bounds = scroll.bounds();
    cx.new(|cx| GeometryObserver {
        _bounds: cx.observe_window_bounds(window, move |_, window, cx| {
            assert_eq!(scroll.bounds(), old_bounds, "原生 resize 后还不能提前更新 Div prepaint 几何");
            let before = scroll.offset();
            window.simulate_next_frame(cx);
            let after_callback = scroll.offset();
            if manual {
                window.dispatch_event(ScrollWheelEvent {
                    position: scroll.bounds().center(),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
                    ..Default::default()
                }.to_platform_input(), cx);
            }
            *seen.borrow_mut() = Some(GeometryDelivery {
                before, after_callback, after_wheel: scroll.offset(),
            });
        }),
    })
}

/// 原生几何变更先于下一 frame callback；旧几何请求须等待 fresh prepaint 后再滚动。
fn geometry_change(cx: &mut TestAppContext, scale_only: bool) {
    let (handle, shell, _writer) = setup(cx);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
    let _observer = cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        geometry_observer(window, cx, shell.read(cx).page_scroll.handle.clone(), seen.clone(), false)
    }).unwrap();
    if scale_only { cx.simulate_window_scale_factor_change(handle, 1.25); }
    else { cx.simulate_window_resize(handle, size(px(1440.), px(300.))); }
    let delivery = seen.borrow();
    let delivery = delivery.as_ref().expect("真实 native bounds observer 必须执行");
    assert_eq!(delivery.after_callback, delivery.before, "原生几何已变时不能按旧 bounds 执行请求");
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before = scroll.offset();
        assert_eq!(before, delivery.before, "测试框架 fresh draw 只能重新排队");
        let target = window.find("monitor-content-input-error").bounds();
        let expected = expected_y(target, scroll.bounds(), before.y, scroll.max_offset().y);
        assert_ne!(expected, before.y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let visible = window.find("monitor-content-input-error").bounds();
        assert!(visible.top() >= scroll.bounds().top() - px(0.51));
        assert!(visible.bottom() <= scroll.bounds().bottom() + px(0.51));
        let revealed = scroll.offset();
        for _ in 0..3 {
            window.render_frame(cx);
            window.simulate_next_frame(cx);
        }
        assert_eq!(scroll.offset(), revealed);
    }).unwrap();
}

/// 真正缩窗回调更新 viewport 后，下一 frame callback 不得消费旧大视口请求。
#[gpui_kit::test]
fn shell_textarea_native_resize_retries_after_fresh_prepaint_once(cx: &mut TestAppContext) {
    geometry_change(cx, false);
}

/// 真正显示倍率变化同样淘汰旧像素几何，不能拿比例值比较代替最终可见行为。
#[gpui_kit::test]
fn shell_textarea_native_scale_change_retries_after_fresh_prepaint_once(cx: &mut TestAppContext) {
    geometry_change(cx, true);
}

/// 放大窗口的原生夹紧不是手动滚动；位于上方的首次焦点仍须在新几何下滚入。
#[gpui_kit::test]
fn shell_textarea_native_resize_clamp_keeps_pending_focus_permission(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
    let _observer = cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-10000.))), cx);
        let focused = input(&shell, 0, cx);
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        assert!(window.find("monitor-content-input-idle").bounds().top() < scroll.bounds().top());
        geometry_observer(window, cx, scroll, seen.clone(), false)
    }).unwrap();
    cx.simulate_window_resize(handle, size(px(1440.), px(500.)));
    let delivery = seen.borrow();
    let delivery = delivery.as_ref().expect("真实 native bounds observer 必须执行");
    assert_eq!(delivery.after_callback, delivery.before);
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let clamped = scroll.offset();
        assert!(clamped.y > delivery.before.y, "fresh Div 必须真实夹紧旧底部偏移");
        let target = window.find("monitor-content-input-idle").bounds();
        assert!(target.top() < scroll.bounds().top(), "原生夹紧后新焦点仍须离屏");
        let expected = expected_y(target, scroll.bounds(), clamped.y, scroll.max_offset().y);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, scroll.offset().y, expected);
        let visible = window.find("monitor-content-input-idle").bounds();
        assert!(visible.top() >= scroll.bounds().top() - px(0.51));
        assert!(visible.bottom() <= scroll.bounds().bottom() + px(0.51));
    }).unwrap();
}

/// 几何漂移已经要求重排但尚未 fresh prepaint 时，真实滚轮接管必须取消原请求。
#[gpui_kit::test]
fn shell_textarea_geometry_retry_keeps_original_manual_scroll_lease(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
    let _observer = cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        geometry_observer(window, cx, shell.read(cx).page_scroll.handle.clone(), seen.clone(), true)
    }).unwrap();
    cx.simulate_window_resize(handle, size(px(1440.), px(300.)));
    let delivery = seen.borrow();
    let delivery = delivery.as_ref().expect("真实 native bounds observer 必须执行");
    assert_eq!(delivery.after_callback, delivery.before);
    assert_ne!(delivery.after_wheel, delivery.before, "fresh prepaint 前父容器必须收到真实 wheel 并自行移动");
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let manual = delivery.after_wheel;
        assert_eq!(scroll.offset(), manual, "自动 fresh prepaint 不能提前滚动");
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), manual, "fresh prepaint 不能重建并抢回已取消的租约");
        for _ in 0..3 {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        }
        assert_eq!(scroll.offset(), manual);
    }).unwrap();
}

/// 在途请求后的两次真实滚轮回到相同 offset，仍是用户接管，不能复活旧请求。
#[gpui_kit::test]
fn shell_textarea_real_wheel_round_trip_cancels_pending_reveal(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let before = scroll.offset();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-80.))), cx);
        assert_ne!(scroll.offset(), before);
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(80.))), cx);
        assert_eq!(scroll.offset(), before, "真实 wheel 必须返回原偏移才能检验 ABA");
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), before, "终值相同不能恢复旧滚动权限");
    }).unwrap();
}

/// 原生多行文本域消费自己的 wheel，父 page 回调不能阻断内层或再滚一次。
#[gpui_kit::test]
fn shell_textarea_inner_native_wheel_does_not_double_scroll_page(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx);
    cx.update_window(handle, |_, window, cx| {
        let focused = input(&shell, 3, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        focused.update(cx, |input, cx| {
            input.replace("one\ntwo\nthree\nfour\nfive\nsix", window, cx);
            input.focus(window, cx);
        });
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let page_before = scroll.offset();
        let inner_before = focused.read(cx).scroll_offset();
        let delta = if inner_before.y < px(0.) { px(10.) } else { px(-10.) };
        window.scroll("monitor-content-input-error", ScrollDelta::Pixels(point(px(0.), delta)), cx);
        let inner_after = focused.read(cx).scroll_offset();
        assert_ne!(inner_after, inner_before, "真实文本域 wheel 必须移动原生编辑 viewport");
        near(window, inner_after.y - inner_before.y, delta);
        assert_eq!(scroll.offset(), page_before, "内层已消费 wheel 时页面不能二次滚动");
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), page_before);
    }).unwrap();
}
