//! 粘滞实表面须阻挡后方控件的鼠标命中，但保留同一原生滚动父路径。

use super::{Shell, state::Route, sticky_tests::setup};
use gpui_kit::{App, AppContext, Entity, Pixels, Point, ScrollDelta, Window, point, px, test::TestWindowExt};

/// 用真实滚轮将正文图片与粘滞区域重叠，避免仅证明无后方控件的空白点击。
fn overlap(window: &mut Window, shell: &Entity<Shell>, footer: bool, cx: &mut App) -> (Point<Pixels>, &'static str) {
    let id = if footer { "monitor-picker-trigger-asking" } else { "monitor-picker-trigger-idle" };
    let area = if footer {
        window.find("monitor-profile-save-bar").bounds()
    } else {
        shell.read(cx).page_scroll.regions.borrow().header.unwrap()
    };
    let trigger = window.find(id).bounds();
    window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), area.center().y - trigger.center().y)), cx);
    let trigger = window.find(id).bounds();
    let area = if footer {
        window.find("monitor-profile-save-bar").bounds()
    } else {
        shell.read(cx).page_scroll.regions.borrow().header.unwrap()
    };
    let click = point(if footer { trigger.left() + px(12.) } else { trigger.right() - px(12.) }, area.center().y);
    assert!(trigger.contains(&click), "点击位置必须确实覆盖真实图片 trigger：{trigger:?}/{area:?}/{click:?}");
    assert!(area.contains(&click));
    assert!(shell.read(cx).monitor.catalog.picker.is_none());
    (click, id)
}

/// 保存栏的说明/空白区域不能代理被遮住的图片选择操作。
#[gpui_kit::test]
fn monitor_sticky_footer_background_blocks_overlapped_native_trigger(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let (click, _) = overlap(window, &shell, true, cx);
        let before = shell.read(cx).monitor.catalog.editors[0].draft.hooks.clone();
        let viewport = shell.read(cx).page_scroll.handle.bounds();
        window.click_at("page-content", click - viewport.origin, cx);
        assert!(shell.read(cx).monitor.catalog.picker.is_none(), "粘滞保存栏背景不能点击穿透图片 trigger");
        assert_eq!(shell.read(cx).monitor.catalog.editors[0].draft.hooks, before);
    }).unwrap();
}

/// 保存按钮仍只运行自身保存，不能同时打开其后方图片选择器。
#[gpui_kit::test]
fn monitor_sticky_save_button_does_not_also_activate_underlying_trigger(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        overlap(window, &shell, true, cx);
        let click = window.find("monitor-profile-save").bounds().center();
        assert!(window.find("monitor-picker-trigger-error").bounds().contains(&click), "真实save重叠：{:?}/{click:?}", window.find("monitor-picker-trigger-error").bounds());
        window.click("monitor-profile-save", cx);
        assert!(shell.read(cx).monitor.catalog.feedback.as_ref().is_some_and(Result::is_err));
        assert!(shell.read(cx).monitor.catalog.picker.is_none(), "保存点击不能同时穿透触发图库");
    }).unwrap();
}

/// 页头按钮之后的背景空白也必须隔离正文命中。
#[gpui_kit::test]
fn monitor_sticky_header_background_blocks_overlapped_native_trigger(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let (click, _) = overlap(window, &shell, false, cx);
        let viewport = shell.read(cx).page_scroll.handle.bounds();
        window.click_at("page-content", click - viewport.origin, cx);
        assert!(shell.read(cx).monitor.catalog.picker.is_none(), "粘滞页头背景不能点击穿透图片 trigger");
        assert!(shell.read(cx).monitor.route == Route::Management);
    }).unwrap();
}

/// 原生页头按钮只执行自身导航，不能在同次点击打开后方图片选择器。
#[gpui_kit::test]
fn monitor_sticky_header_button_does_not_also_activate_underlying_trigger(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let (_, trigger) = overlap(window, &shell, false, cx);
        let click = window.find("monitor-management-tab").bounds().center();
        assert!(window.find(trigger).bounds().contains(&click), "真实header重叠：{:?}/{click:?}", window.find(trigger).bounds());
        window.click("monitor-management-tab", cx);
        assert!(shell.read(cx).monitor.route == Route::Management);
        assert!(shell.read(cx).monitor.catalog.picker.is_none(), "页头按钮不能同时穿透触发图库");
    }).unwrap();
}

/// 页头的鼠标阻挡只能影响非滚轮命中，滚轮仍通过同一 dispatch 父节点滚一次。
#[gpui_kit::test]
fn monitor_sticky_header_wheel_bubbles_once_to_native_page(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-120.))), cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before = scroll.offset();
        window.scroll("monitor-management-tab", ScrollDelta::Pixels(point(px(0.), px(-80.))), cx);
        super::sticky_tests::near(window, scroll.offset().y - before.y, px(-80.));
        assert_eq!(scroll.offset().x, before.x);
    }).unwrap();
}
