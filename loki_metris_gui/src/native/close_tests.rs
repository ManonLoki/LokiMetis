//! 真实GPUI关闭回调已经借用Window；仅替换X11能力/恢复入口事实，不创建GTK托盘或发送通知。

use super::*;
use crate::preferences::{Preferences, Writer};
use gpui_kit::{AppContext, Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, px, size};
use std::{cell::Cell, rc::Rc};

/// 服务与单实例均归属本次临时目录；偏好使用内存writer，通知查询保持停止，登录项只读。
struct Fixture {
    /// 主窗句柄。
    handle: AnyWindowHandle,
    /// 内存偏好写入器。
    writer: Writer,
    /// 测试临时目录。
    _root: tempfile::TempDir,
}

/// 真实Shell::new负责注册生产关闭回调；虚拟窗口没有raw X11句柄，能力在服务缓存边界明确注入。
fn fixture(cx: &mut TestAppContext, main_is_x11: bool, restore_available: bool) -> Fixture {
    fixture_with_visibility(cx, main_is_x11, restore_available, true)
}

/// 窗口可见性通过真实 WindowOptions 进入 GPUI 创建路径，其余资源仍完全隔离。
fn fixture_with_visibility(cx: &mut TestAppContext, main_is_x11: bool, restore_available: bool, show: bool) -> Fixture {
    cx.update(gpui_kit::init);
    let preferences = Preferences::default();
    let writer = Writer::in_memory(preferences.clone());
    let shell_writer = writer.clone();
    let (handle, _) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
        show,
        focus: show,
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(20.), px(30.)), size(px(1200.), px(800.))))),
        ..Default::default()
    }, cx, |window, cx| cx.new(|cx| crate::app::Shell::new(preferences, shell_writer, window, cx))).unwrap());
    let root = tempfile::tempdir().unwrap();
    let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
    let instance = match instance::Instance::acquire(&root.path().join("instance"), sender, false).unwrap() {
        instance::Launch::Primary(instance) => instance,
        instance::Launch::Activated => panic!("isolated instance must be primary"),
    };
    let notifications = notifications::Service::new(false, writer.clone());
    // 唤醒会刷新通知权限；本组只验证窗口生命周期，不能访问真实会话 D-Bus。
    notifications.stop();
    cx.update(|cx| cx.set_global(Services {
        main: handle,
        main_is_x11,
        window_action_generation: 0,
        main_hidden: false,
        main_taskbar_request: linux::TaskbarRequest::default(),
        _poll: Task::ready(()),
        tray: None,
        tray_restore_available_for_test: Some(restore_available),
        autostart: autostart::Service::new(),
        notifications,
        _instance: instance,
        shortcuts: shortcuts::Service::new(),
    }));
    Fixture { handle, writer, _root: root }
}

/// 先释放后台owner，再清理临时IPC和内存偏好线程。
fn finish(fixture: Fixture, cx: &mut TestAppContext) {
    cx.update(|cx| drop(cx.remove_global::<Services>()));
    fixture.writer.finish();
}

/// 生产Shell关闭回调使用启动缓存，延迟原生隐藏成功后才记录主窗已隐藏。
#[gpui_kit::test]
fn shell_close_uses_cached_x11_capability_and_defers_hide(cx: &mut TestAppContext) {
    let f = fixture(cx, true, true);
    cx.update_window(f.handle, |_, window, _| {
        assert!(linux::x11_window_id(window).is_err(), "虚拟平台不能凭raw句柄伪装X11");
    }).unwrap();
    assert!(!VisualTestContext::from_window(f.handle, cx).simulate_close(), "有恢复入口的X11主窗应阻止销毁");
    cx.run_until_parked();
    assert_eq!(super::window_visibility::take_visibility_events(f.handle), vec![false]);
    assert!(cx.read(|cx| cx.global::<Services>().main_hidden));
    assert!(cx.update_window(f.handle, |_, _, _| ()).is_ok(), "同一个主窗口仍可恢复");
    finish(f, cx);
}

/// 由平台实际触发on_window_should_close，证实嵌套update失败而生产隐藏决策仍能成立。
#[gpui_kit::test]
fn platform_close_callback_does_not_reborrow_main_window(cx: &mut TestAppContext) {
    let f = fixture(cx, true, true);
    let nested_update_failed = Rc::new(Cell::new(false));
    let observed = nested_update_failed.clone();
    let main = f.handle;
    cx.update_window(main, |_, window, cx| {
        window.on_window_should_close(cx, move |_, cx| {
            observed.set(main.update(cx, |_, _, _| ()).is_err());
            !hide_if_available(cx)
        });
    }).unwrap();
    assert!(!VisualTestContext::from_window(main, cx).simulate_close());
    assert!(nested_update_failed.get(), "本回调确实处于GPUI窗口借用中");
    cx.run_until_parked();
    assert_eq!(super::window_visibility::take_visibility_events(main), vec![false]);
    finish(f, cx);
}

/// 纯Wayland或实际恢复入口缺失均允许正常关闭，不能把能力缓存当成托盘成功事实。
#[gpui_kit::test]
fn close_without_x11_or_restore_entry_remains_normal_close(cx: &mut TestAppContext) {
    for (main_is_x11, available) in [(false, true), (true, false), (false, false)] {
        let f = fixture(cx, main_is_x11, available);
        assert!(VisualTestContext::from_window(f.handle, cx).simulate_close());
        cx.run_until_parked();
        assert!(super::window_visibility::take_visibility_events(f.handle).is_empty());
        assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
        finish(f, cx);
    }
}

/// 关闭回合后的恢复入口失效仍撤销待执行隐藏，缓存能力不会跳过第二次门禁。
#[gpui_kit::test]
fn deferred_close_hide_rechecks_the_restore_entry(cx: &mut TestAppContext) {
    let f = fixture(cx, true, true);
    cx.update_window(f.handle, |_, _, cx| {
        assert!(hide_if_available(cx));
        cx.global_mut::<Services>().tray_restore_available_for_test = Some(false);
    }).unwrap();
    cx.run_until_parked();
    assert!(super::window_visibility::take_visibility_events(f.handle).is_empty());
    assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
    finish(f, cx);
}

/// 启动收尾只计数回退显示，不驱动真实宿主显窗。
fn complete_counting_shows(cx: &mut TestAppContext, hidden: bool) -> Rc<Cell<usize>> {
    let shows = Rc::new(Cell::new(0));
    let observed = shows.clone();
    cx.update(|cx| startup::complete(hidden, cx, move |_| observed.set(observed.get() + 1)));
    shows
}

/// 隐藏创建不向 Linux 平台发出首次 map，普通创建仍映射同一真实窗口。
#[gpui_kit::test]
fn startup_hidden_window_does_not_request_initial_map(cx: &mut TestAppContext) {
    for show in [false, true] {
        let f = fixture_with_visibility(cx, true, false, show);
        assert_eq!(cx.test_window(f.handle).map_request_count(), usize::from(show));
        finish(f, cx);
    }
}

/// 托盘异步就绪前保持隐藏，成功确认后登记隐藏状态，后续不会触发超时显示。
#[gpui_kit::test]
fn startup_waits_for_tray_confirmation_without_showing(cx: &mut TestAppContext) {
    let f = fixture_with_visibility(cx, true, false, false);
    let shows = complete_counting_shows(cx, true);
    cx.run_until_parked();
    assert_eq!(shows.get(), 0);
    cx.executor().advance_clock(Duration::from_millis(500));
    cx.run_until_parked();
    assert_eq!(shows.get(), 0);
    cx.update(|cx| cx.global_mut::<Services>().tray_restore_available_for_test = Some(true));
    cx.executor().advance_clock(Duration::from_millis(40));
    cx.run_until_parked();
    assert!(cx.read(|cx| cx.global::<Services>().main_hidden));
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(shows.get(), 0);
    assert_eq!(cx.test_window(f.handle).map_request_count(), 0);
    finish(f, cx);
}

/// 没有实际托盘、纯 Wayland 或普通启动立即显示，不遗留不可恢复的主窗。
#[gpui_kit::test]
fn startup_manual_and_missing_restore_paths_show_immediately(cx: &mut TestAppContext) {
    for (hidden, x11, installed) in [(false, true, true), (true, false, true), (true, true, false)] {
        let f = fixture_with_visibility(cx, x11, false, false);
        if !installed { cx.update(|cx| cx.global_mut::<Services>().tray_restore_available_for_test = None); }
        let shows = complete_counting_shows(cx, hidden);
        assert_eq!(shows.get(), 1);
        assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
        finish(f, cx);
    }
}

/// 探测始终失败时三秒内只回退显示一次，不把 unknown 当成可恢复入口。
#[gpui_kit::test]
fn startup_unconfirmed_tray_times_out_to_visible_window(cx: &mut TestAppContext) {
    let f = fixture_with_visibility(cx, true, false, false);
    let shows = complete_counting_shows(cx, true);
    cx.run_until_parked();
    for tick in 1..=75 {
        cx.executor().advance_clock(Duration::from_millis(40));
        cx.run_until_parked();
        assert_eq!(shows.get(), usize::from(tick == 75));
    }
    assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
    finish(f, cx);
}

/// 事件线程迟到执行时按真实截止时间回退，不重新累计轮询次数延长隐藏等待。
#[gpui_kit::test]
fn startup_delayed_poll_still_observes_the_total_deadline(cx: &mut TestAppContext) {
    let f = fixture_with_visibility(cx, true, false, false);
    let shows = complete_counting_shows(cx, true);
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert_eq!(shows.get(), 1);
    finish(f, cx);
}

/// 等待期间手动显示会撤销旧意图，迟到托盘成功不能重新隐藏主窗。
#[gpui_kit::test]
fn startup_manual_show_cancels_pending_hidden_launch(cx: &mut TestAppContext) {
    let f = fixture_with_visibility(cx, true, false, false);
    cx.update(|cx| startup::complete(true, cx, |_| panic!("旧启动意图不得再次显示或隐藏")));
    cx.run_until_parked();
    cx.update(|cx| {
        show(f.handle, cx);
        cx.global_mut::<Services>().tray_restore_available_for_test = Some(true);
    });
    cx.executor().advance_clock(Duration::from_millis(40));
    cx.run_until_parked();
    assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
    finish(f, cx);
}

/// 退出移除原生服务后，在途启动等待保持零窗口副作用。
#[gpui_kit::test]
fn startup_shutdown_cancels_pending_tray_wait(cx: &mut TestAppContext) {
    let f = fixture_with_visibility(cx, true, false, false);
    cx.update(|cx| startup::complete(true, cx, |_| panic!("退出后不能显示")));
    cx.run_until_parked();
    finish(f, cx);
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
}

/// 隐藏启动和关闭隐藏的显窗失败均保留恢复意图；无托盘下一轮成功后才清除。
#[gpui_kit::test]
fn failed_main_show_retries_after_restore_entry_is_lost(cx: &mut TestAppContext) {
    for previously_hidden in [false, true] {
        let f = fixture_with_visibility(cx, true, false, false);
        cx.update(|cx| {
            cx.global_mut::<Services>().main_hidden = previously_hidden;
            super::window_visibility::fail_next_visibility();
            show(f.handle, cx);
        });
        cx.run_until_parked();
        assert!(cx.read(|cx| cx.global::<Services>().main_hidden), "失败不能丢弃仍需显窗的事实");
        assert!(super::window_visibility::take_visibility_events(f.handle).is_empty());
        cx.update(|cx| sync_linux_main(false, cx));
        cx.run_until_parked();
        assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
        assert_eq!(super::window_visibility::take_visibility_events(f.handle), [true]);
        cx.update(|cx| sync_linux_main(false, cx));
        cx.run_until_parked();
        assert!(super::window_visibility::take_visibility_events(f.handle).is_empty(), "成功后不得重复显窗");
        finish(f, cx);
    }
}
