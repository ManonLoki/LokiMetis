// 本文件是 reveal_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：已有请求后发生原生焦点往返，旧回调立即过期，下一次 prepaint 才登记新请求。；图库是同一文档内嵌面板；展开后真实 Tab 到其下文本域仍需滚入。；单独测试实体接收真实 bounds 事件，避免持有 Shell 可变借用时执行租约校验。

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
        let target = input(&shell, 3, cx).read(cx).cursor_layout().unwrap().0;
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
        // 新中心策略让此低窗中的 caret 原本仍在视口内；真实 wheel 先建立离屏前提。
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(120.))), cx);
        tab_to(window, &idle, false, cx);
        assert!(shell.read(cx).monitor.catalog.picker == Some(HookBehavior::Idle));
        let before = scroll.offset();
        let target = input(&shell, 0, cx).read(cx).cursor_layout().unwrap().0;
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
    /// bounds 变化订阅。
    _bounds: gpui_kit::Subscription,
}

/// 记录真实 resize 回调中、测试框架自动绘制之前的可观察滚动结果。
struct GeometryDelivery {
    /// 回调前的滚动偏移。
    before: gpui_kit::Point<Pixels>,
    /// resize 回调后的滚动偏移。
    after_callback: gpui_kit::Point<Pixels>,
    /// 随后滚轮后的滚动偏移。
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
        let target = input(&shell, 3, cx).read(cx).cursor_layout().unwrap().0;
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
        let target = input(&shell, 0, cx).read(cx).cursor_layout().unwrap().0;
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
