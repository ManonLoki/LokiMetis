//! 真实主窗从无焦点接收完整键盘事件，入口修复不能接管已聚焦控件或辅助窗口。

use super::{Page, tests::open_shell};
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Focusable, InputEvent, IntoElement,
    KeyDownEvent, KeyUpEvent, Keystroke, Render, Subscription, TestAppContext, Window,
    WindowOptions, base::Button, div, prelude::*, test::TestWindowExt,
};
use std::{cell::Cell, rc::Rc};

/// 两个原生按钮使窗口隔离与订阅释放都可通过真实焦点和动作观察。
struct BareButtons {
    /// 按钮点击次数。
    clicks: Rc<Cell<usize>>,
}

impl Render for BareButtons {
    /// 不安装Root的Tab绑定，避免其行为掩盖主窗专属入口是否错误介入。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .size_full()
            .flex()
            .child(Button::new("keyboard-bare-first").child("First").on_click(move |_, _, _| {
                clicks.set(clicks.get() + 1);
            }))
            .child(Button::new("keyboard-bare-last").child("Last"))
    }
}

/// 辅助窗有真实tab stop，但不装配主窗Shell或独立键盘策略。
fn open_bare(cx: &mut TestAppContext) -> AnyWindowHandle {
    cx.update(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| BareButtons { clicks: Rc::new(Cell::new(0)) })
        })
        .unwrap()
        .into()
    })
}

/// 分别发送按下和松开，让焦点变化及原生按钮释放激活保留真实时序。
fn key(handle: AnyWindowHandle, name: &str, down: bool, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(name).unwrap();
    cx.update_window(handle, |_, window, cx| {
        if down {
            window.dispatch_event(KeyDownEvent {
                keystroke, is_held: false, prefer_character_input: false,
            }.to_platform_input(), cx);
        } else {
            window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
        }
        window.render_frame(cx);
    }).unwrap();
}

/// 完整按键用于遍历；激活时需要逐帧断言的场景直接使用key。
fn press(handle: AnyWindowHandle, name: &str, cx: &mut TestAppContext) {
    key(handle, name, true, cx);
    key(handle, name, false, cx);
}

/// 原生末端观察器证明未处理的Tab透传，已由入口消费的首次Tab不再下发。
fn observe_tabs(cx: &mut TestAppContext) -> (Rc<Cell<usize>>, Subscription) {
    let received = Rc::new(Cell::new(0));
    let count = received.clone();
    let subscription = cx.update(|cx| cx.observe_keystrokes(move |event, _, _| {
        if event.keystroke.key == "tab" {
            count.set(count.get() + 1);
        }
    }));
    (received, subscription)
}

/// 生产Shell最初不抢焦点；首个Tab进入侧栏，下一次只移一格，Enter仍在松开时导航。
#[gpui_kit::test]
fn first_tab_enters_main_sidebar_once_and_keeps_native_enter(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    let (received, _observer) = observe_tabs(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.focused(cx).is_none(), "创建主窗不能自动聚焦按钮");
    }).unwrap();
    key(handle, "tab", true, cx);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find("nav-dashboard").focused(), Some(true));
        assert_eq!(shell.read(cx).page, Page::Dashboard);
        assert_eq!(received.get(), 0, "首次Tab已消费，不能再交给Root遍历");
    }).unwrap();
    key(handle, "tab", false, cx);
    press(handle, "tab", cx);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find("nav-monitor").focused(), Some(true));
        assert_eq!(shell.read(cx).page, Page::Dashboard);
        assert_eq!(received.get(), 1, "已有焦点的第二次Tab必须经过原生Root");
    }).unwrap();
    key(handle, "enter", true, cx);
    assert_eq!(shell.read_with(cx, |shell, _| shell.page), Page::Dashboard);
    key(handle, "enter", false, cx);
    assert_eq!(shell.read_with(cx, |shell, _| shell.page), Page::Monitor);
    shell.update(cx, |shell, cx| { shell.navigate(Page::Dashboard, cx); });
    key(handle, "enter", false, cx);
    assert_eq!(shell.read_with(cx, |shell, _| shell.page), Page::Dashboard,
        "重复松开不能重新激活已完成的按键");
}

/// 反向首次遍历选择生产主窗最后一个范围按钮，再由Root正向绕回第一个侧栏按钮。
#[gpui_kit::test]
fn first_shift_tab_uses_last_main_window_tab_stop(cx: &mut TestAppContext) {
    let (handle, _, _finish) = open_shell(cx, 1440., 900.);
    let (received, _observer) = observe_tabs(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.focused(cx).is_none());
    }).unwrap();
    press(handle, "shift-tab", cx);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find(("usage-window", 5usize)).focused(), Some(true));
        assert_eq!(received.get(), 0);
    }).unwrap();
    press(handle, "tab", cx);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("nav-dashboard").focused(), Some(true));
        assert_eq!(received.get(), 1);
    }).unwrap();
}

/// 应用和系统组合键不进入无焦点回退，也不能在主窗吞掉原始事件。
#[gpui_kit::test]
fn modified_tab_keeps_main_window_unfocused_and_propagates(cx: &mut TestAppContext) {
    let (handle, _, _finish) = open_shell(cx, 1440., 900.);
    let (received, _observer) = observe_tabs(cx);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    for (index, name) in ["ctrl-tab", "alt-tab", "cmd-tab", "fn-tab",
        "ctrl-shift-tab", "alt-shift-tab", "cmd-shift-tab", "fn-shift-tab"].into_iter().enumerate() {
        press(handle, name, cx);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.focused(cx).is_none(), "{name}不能建立主窗焦点");
            assert_eq!(received.get(), index + 1, "{name}必须交还原生分发");
        }).unwrap();
    }
}

/// 主窗订阅不能按平台active_window猜测目标，辅助窗的正反Tab均透传且不改变主窗。
#[gpui_kit::test]
fn main_keyboard_entry_does_not_intercept_another_window(cx: &mut TestAppContext) {
    let (main, _, _finish) = open_shell(cx, 1440., 900.);
    let other = open_bare(cx);
    let (received, _observer) = observe_tabs(cx);
    for handle in [main, other] {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    }
    for name in ["tab", "shift-tab"] {
        press(other, name, cx);
        cx.update_window(other, |_, window, cx| {
            assert!(window.focused(cx).is_none());
            assert_eq!(window.find("keyboard-bare-first").focused(), Some(false));
            assert_eq!(window.find("keyboard-bare-last").focused(), Some(false));
        }).unwrap();
        cx.update_window(main, |_, window, cx| assert!(window.focused(cx).is_none())).unwrap();
    }
    assert_eq!(received.get(), 2, "辅助窗口事件不能被主窗订阅消费");
}

/// 原生禁用焦点时不能宣称已处理Tab；没有获得焦点必须继续向末端观察器传播。
#[gpui_kit::test]
fn keyboard_entry_preserves_tab_when_native_focus_is_disabled(cx: &mut TestAppContext) {
    let (handle, _, _finish) = open_shell(cx, 1440., 900.);
    let (received, _observer) = observe_tabs(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.disable_focus(cx);
    }).unwrap();
    press(handle, "tab", cx);
    press(handle, "shift-tab", cx);
    cx.update_window(handle, |_, window, cx| assert!(window.focused(cx).is_none())).unwrap();
    assert_eq!(received.get(), 2);
}

/// 真实用量数字输入已拥有焦点时，Tab仍由Input与Root处理，草稿和反向焦点都保留。
#[gpui_kit::test]
fn focused_production_input_keeps_native_tab_and_text(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    shell.update(cx, |shell, cx| {
        shell.dashboard.client = 1;
        shell.dashboard.route = super::dashboard::Route::Settings;
        shell.dashboard.usage_settings.saved = Some(loki_metris_core::UsageSettings::default());
        cx.notify();
    });
    let input = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let input = shell.read(cx).dashboard.usage_settings.inputs.as_ref().unwrap()[0].clone();
        input.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        input
    }).unwrap();
    cx.simulate_keystrokes(handle, "7");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(input.read(cx).value().as_ref(), "7");
        assert_eq!(window.focused(cx), Some(input.focus_handle(cx)));
    }).unwrap();
    press(handle, "tab", cx);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find(("usage-settings-save", 0usize)).focused(), Some(true));
        assert_eq!(input.read(cx).value().as_ref(), "7");
    }).unwrap();
    press(handle, "shift-tab", cx);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.focused(cx), Some(input.focus_handle(cx)));
        assert_eq!(input.read(cx).value().as_ref(), "7");
    }).unwrap();
}

/// 生产模态已有稳定焦点，重复正反Tab保持其原生陷阱，Escape恢复打开前实际控件。
#[gpui_kit::test]
fn focused_production_modal_keeps_its_native_focus_trap(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    let previous = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-settings", cx);
        window.render_frame(cx);
        let previous = window.focused(cx).unwrap();
        window.click("settings-release-notes", cx);
        window.render_frame(cx);
        assert!(shell.read(cx).release_notes_modal.is_some());
        previous
    }).unwrap();
    for name in ["tab", "shift-tab"] {
        for _ in 0..6 {
            press(handle, name, cx);
            cx.update_window(handle, |_, window, cx| {
                let trap = gpui_kit::base::active_focus_trap(window, cx).expect("原生模态必须保有焦点陷阱");
                assert!(trap.contains_focused(window, cx));
                assert_eq!(window.find("release-notes-close").focused(), Some(true));
                assert_eq!(window.find("nav-dashboard").focused(), Some(false));
            }).unwrap();
        }
    }
    press(handle, "escape", cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(shell.read(cx).release_notes_modal.is_none());
        assert_eq!(window.focused(cx), Some(previous));
    }).unwrap();
}

/// 同一仍存活窗口先证明订阅获焦和一次原生激活，再真正释放Shell；窗口ID不能掩盖泄漏。
#[gpui_kit::test]
fn releasing_shell_removes_keyboard_entry_from_the_same_window(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    let weak = shell.downgrade();
    let clicks = Rc::new(Cell::new(0));
    cx.update_window(handle, |_, window, cx| {
        let clicks = clicks.clone();
        window.replace_root(cx, |_, _| BareButtons { clicks });
        window.blur(cx);
        window.render_frame(cx);
    }).unwrap();
    press(handle, "tab", cx);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("keyboard-bare-first").focused(), Some(true));
    }).unwrap();
    key(handle, "enter", true, cx);
    assert_eq!(clicks.get(), 0);
    key(handle, "enter", false, cx);
    assert_eq!(clicks.get(), 1);
    key(handle, "enter", false, cx);
    assert_eq!(clicks.get(), 1, "原生Button只能激活一次");
    drop(shell);
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
        cx.run_until_parked();
    }
    assert!(weak.upgrade().is_none(), "测试必须真正释放拥有Subscription的Shell");
    let (received, _observer) = observe_tabs(cx);
    cx.update_window(handle, |_, window, cx| { window.blur(cx); window.render_frame(cx); }).unwrap();
    press(handle, "tab", cx);
    press(handle, "shift-tab", cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.focused(cx).is_none(), "卸载Shell后不能继续应用旧主窗入口");
    }).unwrap();
    assert_eq!(received.get(), 2, "已回收订阅不能消费仍存活窗口的事件");
}
