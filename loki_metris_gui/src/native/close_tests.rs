//! 真实GPUI关闭回调已经借用Window；仅替换X11能力/恢复入口事实，不创建GTK托盘或发送通知。

use super::*;
use crate::preferences::{Preferences, Writer};
use gpui_kit::{AppContext, Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, px, size};
use std::{cell::Cell, rc::Rc};

/// 服务与单实例均归属本次临时目录；偏好使用内存writer，登录项owner只执行启动时只读查询。
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
    cx.update(gpui_kit::init);
    let preferences = Preferences::default();
    let writer = Writer::in_memory(preferences.clone());
    let shell_writer = writer.clone();
    let (handle, _) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
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
    cx.update(|cx| cx.set_global(Services {
        main: handle,
        main_is_x11,
        window_action_generation: 0,
        main_hidden: false,
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
    assert_eq!(crate::pet::host::take_visibility_events(f.handle), vec![false]);
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
    assert_eq!(crate::pet::host::take_visibility_events(main), vec![false]);
    finish(f, cx);
}

/// 纯Wayland或实际恢复入口缺失均允许正常关闭，不能把能力缓存当成托盘成功事实。
#[gpui_kit::test]
fn close_without_x11_or_restore_entry_remains_normal_close(cx: &mut TestAppContext) {
    for (main_is_x11, available) in [(false, true), (true, false), (false, false)] {
        let f = fixture(cx, main_is_x11, available);
        assert!(VisualTestContext::from_window(f.handle, cx).simulate_close());
        cx.run_until_parked();
        assert!(crate::pet::host::take_visibility_events(f.handle).is_empty());
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
    assert!(crate::pet::host::take_visibility_events(f.handle).is_empty());
    assert!(!cx.read(|cx| cx.global::<Services>().main_hidden));
    finish(f, cx);
}
