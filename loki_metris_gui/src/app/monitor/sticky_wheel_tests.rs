//! 滚轮通过真实重叠命中路径，区分粘滞栏父滚动与可见原生编辑器自身滚动。

use super::{Shell, sticky_tests::{input, near, setup}};
use gpui_kit::{App, AppContext, Entity, InputEvent as _, Point, Pixels, ScrollDelta, ScrollWheelEvent, Window, point, px, test::TestWindowExt};
use gpui_kit::component::input::TextareaState;

/// 多行文字只经真实原生输入事件写入隔离草稿，不直接设滚动偏移或读取剪贴板。
fn multiline(window: &mut Window, shell: &Entity<Shell>, cx: &mut App) -> Entity<TextareaState> {
    let editor = input(shell, 0, cx);
    editor.update(cx, |input, cx| input.focus(window, cx));
    window.render_frame(cx);
    let text = "a\n".repeat(12);
    window.input(&text, cx);
    window.render_frame(cx);
    assert!(editor.read(cx).scroll_offset().y < px(-80.), "真实多行编辑器必须确实有向上滚动空间");
    editor
}

/// 在实际像素位置发送同一原生事件，不调用页面或输入滚动回调。
fn wheel(window: &mut Window, position: Point<Pixels>, delta: f32, cx: &mut App) {
    window.simulate_mouse_move(position, cx);
    window.dispatch_event(ScrollWheelEvent { position, delta: ScrollDelta::Pixels(point(px(0.), px(delta))), ..Default::default() }.to_platform_input(), cx);
    window.render_frame(cx);
}

/// 保存栏背景是实际鼠标目标时，滚轮应走它的页面父路径，不交给被覆盖的编辑器。
#[gpui_kit::test]
fn monitor_sticky_footer_wheel_does_not_scroll_covered_native_textarea(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let editor = multiline(window, &shell, cx);
        let footer = window.find("monitor-profile-save-bar").bounds();
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), footer.center().y - box_bounds.center().y)), cx);
        let position = window.find("monitor-content-input-idle").bounds().center();
        assert!(window.find("monitor-profile-save-bar").bounds().contains(&position), "事件坐标必须真实被保存栏覆盖");
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before_page = scroll.offset();
        let before_editor = editor.read(cx).scroll_offset();
        assert!(before_page.y < px(-1.), "页面也必须确实有向上滚动空间：{before_page:?}");
        let delta: f32 = ((-before_page.y).min(px(40.)) / 2.).into();
        wheel(window, position, delta, cx);
        assert_eq!(editor.read(cx).scroll_offset(), before_editor, "粘滞栏的滚轮不能交给其后方原生编辑器");
        near(window, scroll.offset().y - before_page.y, px(delta));
        assert_eq!(scroll.offset().x, before_page.x);
    }).unwrap();
}

/// 直接可见编辑器仍优先消费自己的滚轮，不能因粘滞栏修复而强制滚动整页。
#[gpui_kit::test]
fn monitor_visible_native_textarea_retains_own_wheel_before_parent(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let editor = multiline(window, &shell, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), scroll.bounds().center().y - box_bounds.center().y)), cx);
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        assert!(box_bounds.top() > shell.read(cx).page_scroll.regions.borrow().header.unwrap().bottom());
        assert!(box_bounds.bottom() < window.find("monitor-profile-save-bar").bounds().top());
        let before_page = scroll.offset();
        let before_editor = editor.read(cx).scroll_offset();
        wheel(window, box_bounds.center(), 40., cx);
        near(window, editor.read(cx).scroll_offset().y - before_editor.y, px(40.));
        assert_eq!(editor.read(cx).scroll_offset().x, before_editor.x);
        assert_eq!(scroll.offset(), before_page, "可见原生输入自身有滚动余量时页面不能同时滚动");
    }).unwrap();
}

/// 保存栏下方12px仍是正文可交互区域，不能用单一上半视口矩形近似命中。
#[gpui_kit::test]
fn monitor_sticky_footer_bottom_gap_keeps_native_textarea_wheel(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let editor = multiline(window, &shell, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let footer = window.find("monitor-profile-save-bar").bounds();
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        let position = point(box_bounds.center().x, footer.bottom() + px(2.));
        assert!(box_bounds.contains(&position) && scroll.bounds().contains(&position));
        assert!(!footer.contains(&position), "真实露出缝隙不能误作保存栏覆盖区");
        let before_page = scroll.offset();
        let before_editor = editor.read(cx).scroll_offset();
        wheel(window, position, 40., cx);
        near(window, editor.read(cx).scroll_offset().y - before_editor.y, px(40.));
        assert_eq!(scroll.offset(), before_page);
    }).unwrap();
}

/// 页头覆盖区域同样只向真实页面父路径滚动，正文输入不参与该点的命中。
#[gpui_kit::test]
fn monitor_sticky_header_wheel_does_not_scroll_covered_native_textarea(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 500.);
    cx.update_window(handle, |_, window, cx| {
        let editor = multiline(window, &shell, cx);
        let header = shell.read(cx).page_scroll.regions.borrow().header.unwrap();
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), header.center().y - box_bounds.center().y)), cx);
        let header = shell.read(cx).page_scroll.regions.borrow().header.unwrap();
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        let position = point(box_bounds.center().x, (header.bottom().min(box_bounds.bottom()) + header.top().max(box_bounds.top())) / 2.);
        assert!(header.contains(&position) && box_bounds.contains(&position));
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before_page = scroll.offset();
        let before_editor = editor.read(cx).scroll_offset();
        wheel(window, position, 40., cx);
        assert_eq!(editor.read(cx).scroll_offset(), before_editor);
        near(window, scroll.offset().y - before_page.y, px(40.));
        assert_eq!(scroll.offset().x, before_page.x);
    }).unwrap();
}

/// 命中排除不裁剪原生光标或选择几何，键盘选择仍属于同一输入实体。
#[gpui_kit::test]
fn monitor_sticky_covered_textarea_retains_native_cursor_and_selection(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let editor = multiline(window, &shell, cx);
        let caret = editor.read(cx).cursor_layout().expect("真实原生光标布局必须保留").0;
        let footer = window.find("monitor-profile-save-bar").bounds();
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), footer.center().y - caret.center().y)), cx);
        let before = editor.read(cx).cursor_layout().expect("真实原生光标布局必须保留");
        assert!(window.find("monitor-profile-save-bar").bounds().contains(&before.0.center()));
        window.press("cmd-a", cx);
        assert_eq!(editor.read(cx).selected_range(), 0..editor.read(cx).value().len());
        assert_eq!(editor.read(cx).selected_value(), editor.read(cx).value());
        assert_eq!(editor.read(cx).cursor_layout(), Some(before), "仅排除命中不能改变原生光标的实际位置/行高");
    }).unwrap();
}
