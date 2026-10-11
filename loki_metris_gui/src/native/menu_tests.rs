//! 原生菜单通过真实平台投影检查语言与已确认状态，不操作用户登录项或通知权限。

use super::*;
use gpui_kit::{Action, OwnedMenuItem, TestAppContext};

/// 检查实际菜单动作及其系统可见属性，避免仅断言构建中间值。
fn assert_action<A: Action>(
    item: &OwnedMenuItem,
    expected_name: &str,
    expected_checked: bool,
    expected_disabled: bool,
) {
    let OwnedMenuItem::Action {
        name,
        action,
        checked,
        disabled,
        ..
    } = item
    else {
        panic!("expected action")
    };
    assert_eq!(name, expected_name);
    assert!(action.as_any().is::<A>());
    assert_eq!(*checked, expected_checked);
    assert_eq!(*disabled, expected_disabled);
}

/// 语言经过异步刷新仍与显式选择一致，AppKit 的窗口菜单保留精确注册名。
#[gpui_kit::test]
fn selected_language_survives_state_refresh(cx: &mut TestAppContext) {
    cx.update(|cx| {
        install("zh-CN", cx);
        for (locale, title, autostart, notification, quit) in [
            ("zh-CN", "诡秘神谕", "开机自启", "系统通知", "退出诡秘神谕"),
            ("en-US", "LokiMetris", "Start at login", "System notifications", "Quit LokiMetris"),
            ("zh-CN", "诡秘神谕", "开机自启", "系统通知", "退出诡秘神谕"),
        ] {
            set_language(locale, cx);
            for _ in 0..2 {
                refresh(cx);
                let menus = cx.get_menus().unwrap();
                assert_eq!(menus[0].name.as_ref(), title);
                assert_eq!(menus[0].items.len(), 4);
                assert_action::<ToggleAutostart>(&menus[0].items[0], autostart, false, true);
                assert_action::<ToggleNotification>(&menus[0].items[1], notification, false, false);
                assert_action::<Quit>(&menus[0].items[3], quit, false, false);
                assert!(matches!(menus[0].items[2], OwnedMenuItem::Separator));
                #[cfg(target_os = "macos")]
                {
                    assert_eq!(menus[1].name.as_ref(), "Window");
                    assert_eq!(menus[1].items.len(), 1);
                    assert_action::<Minimize>(&menus[1].items[0],
                        if locale == "zh-CN" { "最小化" } else { "Minimize" }, false, true);
                    let bindings = cx.all_bindings_for_input(&[gpui_kit::Keystroke::parse("cmd-m").unwrap()]);
                    assert_eq!(bindings.len(), 1);
                    assert!(bindings[0].action().as_any().is::<Minimize>());
                    assert!(bindings[0].predicate().is_none());
                    for key in ["m", "ctrl-m"] {
                        assert!(cx.all_bindings_for_input(&[gpui_kit::Keystroke::parse(key).unwrap()])
                            .iter().all(|binding| !binding.action().as_any().is::<Minimize>()));
                    }
                }
            }
        }
    });
}

/// 真实菜单属性跟随主窗 key 状态，往返辅助窗口后恢复可用且不影响系统窗口菜单名。
#[cfg(target_os = "macos")]
#[gpui_kit::test]
fn minimize_menu_tracks_main_window_key_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        for chinese in [true, false] {
            for main_is_key in [true, false, false, true] {
                cx.set_menus(vec![window_menu(chinese, main_is_key)]);
                let menus = cx.get_menus().unwrap();
                assert_eq!(menus.len(), 1);
                assert_eq!(menus[0].name.as_ref(), "Window");
                assert_eq!(menus[0].items.len(), 1);
                assert_action::<Minimize>(&menus[0].items[0],
                    if chinese { "最小化" } else { "Minimize" }, false, !main_is_key);
            }
        }
    });
}

/// 第二个真实 App 不继承第一个 App 的中文菜单缓存，也不需要全局翻译库写入。
#[gpui_kit::test]
fn language_cache_is_scoped_to_app(first: &mut TestAppContext, second: &mut TestAppContext) {
    first.update(|cx| set_language("zh-CN", cx));
    second.update(|cx| {
        refresh(cx);
        assert_eq!(cx.get_menus().unwrap()[0].name.as_ref(), "LokiMetris");
        set_language("en-US", cx);
    });
    first.update(|cx| {
        refresh(cx);
        assert_eq!(cx.get_menus().unwrap()[0].name.as_ref(), "诡秘神谕");
    });
}

/// 已确认开关决定勾选，忙碌与未知禁用；写入错误仍保留重新读回的实际系统值。
#[gpui_kit::test]
fn autostart_menu_uses_confirmed_system_state(cx: &mut TestAppContext) {
    cx.update(|cx| {
        set_language("zh-CN", cx);
        for (enabled, busy, desired, error, checked, disabled) in [
            (Some(false), false, None, false, false, false),
            (Some(true), false, None, false, true, false),
            (Some(false), true, Some(true), false, false, true),
            (Some(true), true, Some(false), false, true, true),
            (None, false, None, true, false, true),
            (None, true, None, false, false, true),
            (Some(false), false, None, true, false, false),
            (Some(true), false, None, true, true, false),
        ] {
            let state = super::super::autostart::State {
                enabled,
                busy,
                desired,
                error,
                revision: 42,
            };
            apply_menus(&state, &super::super::notifications::State::default(), cx);
            assert_action::<ToggleAutostart>(
                &cx.get_menus().unwrap()[0].items[0],
                "开机自启",
                checked,
                disabled,
            );
        }
    });
}

/// 通知菜单仍使用已保存偏好，授权等待不提前勾选，禁用状态跟随 owner。
#[gpui_kit::test]
fn notification_menu_keeps_saved_preference(cx: &mut TestAppContext) {
    cx.update(|cx| {
        set_language("en-US", cx);
        for (enabled, busy, desired, label) in [
            (false, false, None, "System notifications"),
            (true, false, None, "System status unavailable"),
            (false, true, Some(true), "Saving notification preference"),
            (true, true, Some(false), "Saving notification preference"),
        ] {
            let state = super::super::notifications::State {
                enabled,
                busy,
                desired,
                ..Default::default()
            };
            apply_menus(&super::super::autostart::State::default(), &state, cx);
            let menus = cx.get_menus().unwrap();
            assert_action::<ToggleNotification>(&menus[0].items[1], label, enabled, busy);
        }
    });
}
