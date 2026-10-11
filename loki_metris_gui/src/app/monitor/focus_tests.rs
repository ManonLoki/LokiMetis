//! 原生图片按钮与文本域共享焦点租约；回归从真实键盘和生产页面事实观察取消行为。

use super::{state::Route, sticky_tests::{input, near, setup, tab_to_button}};
use crate::app::Page;
use gpui_kit::{AppContext, ScrollDelta, TestAppContext, point, px, test::TestWindowExt};
use loki_metris_core::{AiTool, HookBehavior};

/// 页面或 Agent 往返、clear 卸载及 trigger 禁用均在尚未 render 时终止旧按钮请求。
#[gpui_kit::test]
fn monitor_picker_focus_discards_navigation_and_unmounted_field_before_render(cx: &mut TestAppContext) {
    for change in 0..5 {
        let (handle, shell, _writer) = setup(cx, 500.);
        cx.update_window(handle, |_, window, cx| {
            let running = input(&shell, 1, cx);
            running.update(cx, |input, cx| input.focus(window, cx));
            window.render_frame(cx);
            let id = if change == 3 { "monitor-picker-clear-asking" } else { "monitor-picker-trigger-asking" };
            tab_to_button(window, id, false, cx);
            let scroll = shell.read(cx).page_scroll.handle.clone();
            assert!(window.find(id).bounds().top() > scroll.bounds().bottom());
            let before = scroll.offset();
            let focused = window.focused(cx).unwrap();
            shell.update(cx, |shell, cx| match change {
                0 => { shell.navigate(Page::Settings, cx); shell.navigate(Page::Monitor, cx); },
                1 => { shell.select_monitor_route(Route::Hooks, cx); shell.select_monitor_route(Route::Management, cx); },
                2 => { shell.select_monitor_tool(AiTool::ClaudeCode, cx); shell.select_monitor_tool(AiTool::Codex, cx); },
                3 => shell.select_monitor_image(AiTool::Codex, HookBehavior::Asking, String::new(), cx),
                4 => { shell.monitor.catalog.images_loading = true; cx.notify(); },
                _ => unreachable!(),
            });
            assert!(focused.is_focused(window), "取消须由页面/字段事实证明，不能靠预先移走焦点");
            window.simulate_next_frame(cx);
            assert_eq!(scroll.offset(), before, "旧原生按钮请求类型 {change} 不能复活");
        }).unwrap();
    }
}

/// 同一按钮在两次 render 间真实 focus_next/prev，旧请求过期，新 prepaint 才能中心滚入。
#[gpui_kit::test]
fn monitor_picker_focus_round_trip_cancels_old_request_and_requeues_native_button(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 500.);
    cx.update_window(handle, |_, window, cx| {
        let running = input(&shell, 1, cx);
        running.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        tab_to_button(window, "monitor-picker-trigger-asking", false, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before = scroll.offset();
        let focused = window.focused(cx).unwrap();
        window.focus_next(cx);
        assert!(!focused.is_focused(window));
        window.focus_prev(cx);
        assert!(focused.is_focused(window));
        window.simulate_next_frame(cx);
        assert_eq!(scroll.offset(), before, "同 handle 返回不能复活旧 generation 请求");
        window.render_frame(cx);
        let target = window.find("monitor-picker-trigger-asking").bounds();
        let wanted = before.y + scroll.bounds().center().y - target.center().y;
        assert!(wanted > -scroll.max_offset().y && wanted < px(0.));
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        near(window, window.find("monitor-picker-trigger-asking").bounds().center().y, scroll.bounds().center().y);
        assert_eq!(scroll.offset().x, before.x);
        window.press("enter", cx);
        assert!(shell.read(cx).monitor.catalog.picker == Some(HookBehavior::Asking), "同一原生按钮的 Enter 仍必须打开真实内嵌图库");
    }).unwrap();
}

/// 实际两次父滚轮返回相同偏移，仍撤销原生按钮请求；不能只看最终 offset。
#[gpui_kit::test]
fn monitor_picker_focus_real_wheel_round_trip_keeps_user_scroll(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 500.);
    cx.update_window(handle, |_, window, cx| {
        let running = input(&shell, 1, cx);
        running.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        tab_to_button(window, "monitor-picker-trigger-asking", false, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before = scroll.offset();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-80.))), cx);
        assert_ne!(scroll.offset(), before);
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(80.))), cx);
        assert_eq!(scroll.offset(), before);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), before, "真实用户接管不能因 offset ABA 被重新取得权限");
    }).unwrap();
}
