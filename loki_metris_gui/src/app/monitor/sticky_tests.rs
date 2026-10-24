//! 从真实 Shell 的绘制、焦点与命中边界验证粘滞层；不复制滚动或按钮业务状态。

use super::{Shell, state::{ImageEntry, Route}};
use crate::{app::Page, monitor_media::image_decode::MonitorImageSource};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, Pixels, Role, ScrollDelta,
    TestAppContext, Window, component::input::TextareaState, point, px, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, AiTool, ImageFormat, public_monitor_ai_tools};
use std::sync::Arc;

/// 内存 GIF 使真实清空按钮参与 Tab 顺序，所有偏好与保存失败均限制在测试内存。
pub(super) fn setup(cx: &mut TestAppContext, height: f32) -> (AnyWindowHandle, Entity<Shell>, crate::app::tests::FinishWriter) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., height);
    shell.update(cx, |shell, cx| {
        shell.page = Page::Monitor;
        shell.monitor.route = Route::Management;
        shell.monitor.catalog.accept_profiles(public_monitor_ai_tools().map(AiProfileDraft::default_for).collect());
        shell.monitor.catalog.editor_mut(AiTool::Codex).unwrap().draft.hooks[2].image = "sticky-gif".into();
        let source = MonitorImageSource {
            id: "sticky-gif".into(),
            bytes: Arc::from(include_bytes!("../../../tests/fixtures/monitor-loop.gif").as_slice()),
            format: ImageFormat::Gif,
        };
        shell.monitor.catalog.images = vec![ImageEntry {
            id: source.id.clone(), filename: "sticky.gif".into(), format: source.format,
            source: Ok(source), views: Default::default(), residency: Default::default(),
        }];
        shell.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
    (handle, shell, writer)
}

/// 读取真实编辑实体，不镜像其值或焦点状态。
pub(super) fn input(shell: &Entity<Shell>, index: usize, cx: &gpui_kit::App) -> Entity<TextareaState> {
    shell.read(cx).monitor.catalog.editors[0].inputs.as_ref().unwrap()[index].clone()
}

/// 每条吸附边容许一个设备像素，不能把几十像素的遮挡归为排版舍入。
pub(super) fn near(window: &Window, actual: Pixels, expected: Pixels) {
    assert!((actual - expected).abs() <= px(1. / window.scale_factor()), "{actual:?} vs {expected:?}");
}

/// 通过真实 Tab 绑定抵达原生按钮，并核对其辅助属性仍来自同一个控件。
pub(super) fn tab_to_button(window: &mut Window, id: &str, reverse: bool, cx: &mut gpui_kit::App) {
    for _ in 0..32 {
        window.press(if reverse { "shift-tab" } else { "tab" }, cx);
        let button = window.find(id.to_owned());
        assert_eq!(button.role(), Some(Role::Button));
        assert!(button.label().is_some());
        if button.focused() == Some(true) { return; }
    }
    panic!("原生 Tab 未抵达 {id}");
}

/// 焦点滚入后目标须完整处于实际 toolbar 和 savebar 之间，不把整视口当可见区。
fn unobscured(window: &Window, shell: &Entity<Shell>, target: Bounds<Pixels>, cx: &gpui_kit::App) {
    let viewport = shell.read(cx).page_scroll.handle.bounds();
    let toolbar = window.find("monitor-management-tab").bounds();
    let footer = window.find("monitor-profile-save-bar").bounds();
    assert!(target.top() >= viewport.top() - px(0.51));
    assert!(target.top() >= toolbar.bottom() - px(0.51), "目标被真实页头覆盖：{target:?} / {toolbar:?}");
    assert!(target.bottom() <= footer.top() + px(0.51), "目标被真实保存栏覆盖：{target:?} / {footer:?}");
}

/// 页头初始保留页面 padding，越过阈值后粘在 top0，且真实原生点击仍切换子页。
#[gpui_kit::test]
fn monitor_toolbar_sticks_after_threshold_and_keeps_native_click(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let initial = window.find("monitor-management-tab").bounds();
        assert!(initial.top() > scroll.bounds().top() + px(32.));
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-20.))), cx);
        near(window, window.find("monitor-management-tab").bounds().top(), initial.top() - px(20.));
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-100.))), cx);
        let pinned = window.find("monitor-management-tab").bounds();
        near(window, pinned.top(), initial.top() - px(32.));
        assert!(pinned.top() >= scroll.bounds().top());
        window.click("monitor-workbench-tab", cx);
        assert!(shell.read(cx).monitor.route == Route::Workbench);
    }).unwrap();
}

/// 保存栏保留正常文档占位：远离末尾 bottom12，滚到底回归正常流并留下页面32px内距。
#[gpui_kit::test]
fn monitor_savebar_sticky_returns_to_normal_flow_at_document_end(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let initial = window.find("monitor-profile-save-bar").bounds();
        near(window, initial.bottom(), scroll.bounds().bottom() - px(12.));
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-10000.))), cx);
        let bottom = window.find("monitor-profile-save-bar").bounds();
        near(window, scroll.offset().y, -scroll.max_offset().y);
        near(window, bottom.bottom(), scroll.bounds().bottom() - px(32.));
        let card = window.find("monitor-behavior-asking").bounds();
        assert!(card.bottom() < bottom.top(), "页尾栏必须在最后行为卡后占位，不能覆盖卡片");
        let save = window.find("monitor-profile-save");
        assert_eq!(save.role(), Some(Role::Button));
        assert_eq!(save.label(), Some(shell.read(cx).t("monitor.management.save").as_str()));
        window.click("monitor-profile-save", cx);
        assert!(shell.read(cx).monitor.catalog.feedback.as_ref().is_some_and(Result::is_err), "真实点击必须抵达保存处理器的隔离失败回执");
    }).unwrap();
}

/// 真实 Running→clear→image→textarea 顺序，image 和 textarea 聚焦后完整露出实际保存栏上方。
#[gpui_kit::test]
fn monitor_keyboard_trigger_and_textarea_reveal_above_actual_savebar(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let running = input(&shell, 1, cx);
        running.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        tab_to_button(window, "monitor-picker-clear-asking", false, cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        tab_to_button(window, "monitor-picker-trigger-asking", false, cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        unobscured(window, &shell, window.find("monitor-picker-trigger-asking").bounds(), cx);
        window.press("tab", cx);
        assert!(input(&shell, 2, cx).focus_handle(cx).is_focused(window));
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        unobscured(window, &shell, window.find("monitor-content-input-asking").bounds(), cx);
    }).unwrap();
}

/// Reference 低窗中离屏原生 caret 滚到完整视口中心，nearest 底对齐不足以复刻此行为。
#[gpui_kit::test]
fn monitor_offscreen_textarea_focus_centers_actual_native_caret(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 500.);
    cx.update_window(handle, |_, window, cx| {
        let idle = input(&shell, 0, cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        idle.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let caret = idle.read(cx).cursor_layout().expect("真实原生 active caret 必须已布局").0;
        let viewport = scroll.bounds();
        assert!(caret.top() > viewport.bottom(), "中心 oracle 需要真实完全离屏 caret");
        let wanted = scroll.offset().y + viewport.center().y - caret.center().y;
        assert!(wanted > -scroll.max_offset().y && wanted < px(0.), "足够滚动范围才能区分 center 与 clamp");
        let before_x = scroll.offset().x;
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        let visible_caret = idle.read(cx).cursor_layout().unwrap().0;
        near(window, visible_caret.center().y, viewport.center().y);
        assert_eq!(scroll.offset().x, before_x);
        unobscured(window, &shell, window.find("monitor-content-input-idle").bounds(), cx);
    }).unwrap();
}

/// 原版可见判定忽略 sticky 遮挡：image rect 在完整视口内时，Tab 不为保存栏额外滚动。
#[gpui_kit::test]
fn monitor_visible_image_focus_keeps_scroll_even_behind_sticky_savebar(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let running = input(&shell, 1, cx);
        running.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let trigger = window.find("monitor-picker-trigger-asking").bounds();
        let delta = scroll.bounds().bottom() - px(1.) - trigger.bottom();
        assert!(delta < px(0.));
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), delta)), cx);
        tab_to_button(window, "monitor-picker-trigger-asking", false, cx);
        let trigger = window.find("monitor-picker-trigger-asking").bounds();
        assert!(trigger.top() >= scroll.bounds().top() && trigger.bottom() <= scroll.bounds().bottom());
        assert!(trigger.bottom() > window.find("monitor-profile-save-bar").bounds().top(), "真实保存栏应覆盖图片底部");
        let before = scroll.offset();
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), before, "rect 可见时不能因 sticky reservation 增加滚动");
    }).unwrap();
}

/// 原版 caret 可见便保持偏移，允许 textarea 底边截断且 caret 正好位于 sticky 栏后方。
#[gpui_kit::test]
fn monitor_visible_native_caret_keeps_partial_textarea_behind_savebar(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let idle = input(&shell, 0, cx);
        idle.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let caret = idle.read(cx).cursor_layout().unwrap().0;
        let desired = scroll.bounds().bottom() - px(20.);
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), desired - caret.center().y)), cx);
        window.press("shift-tab", cx);
        assert_eq!(window.find("monitor-picker-trigger-idle").focused(), Some(true));
        window.press("tab", cx);
        assert!(idle.focus_handle(cx).is_focused(window));
        let caret = idle.read(cx).cursor_layout().unwrap().0;
        let box_bounds = window.find("monitor-content-input-idle").bounds();
        let footer = window.find("monitor-profile-save-bar").bounds();
        assert!(caret.top() >= scroll.bounds().top() && caret.bottom() <= scroll.bounds().bottom());
        assert!(caret.center().y > footer.top() && caret.center().y < footer.bottom());
        assert!(box_bounds.bottom() > scroll.bounds().bottom(), "完整 textbox 须真实被截断");
        let before = scroll.offset();
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(scroll.offset(), before, "active caret 可见不能为整框或 sticky 遮挡额外滚动");
    }).unwrap();
}

/// 保存栏保留父 dispatch 路径，真实 wheel 仍由原生 page-content 滚一次。
#[gpui_kit::test]
fn monitor_sticky_savebar_wheel_bubbles_once_to_native_page(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let scroll = shell.read(cx).page_scroll.handle.clone();
        let before = scroll.offset();
        window.scroll("monitor-profile-save-bar", ScrollDelta::Pixels(point(px(0.), px(-80.))), cx);
        near(window, scroll.offset().y - before.y, px(-80.));
        assert_eq!(scroll.offset().x, before.x);
    }).unwrap();
}

/// deferred 绘制顺序不能改写 DOM 顺序：最后页头 Tab 后仍进入正文的首个原生 Agent Tab。
#[gpui_kit::test]
fn monitor_sticky_paint_order_preserves_native_header_to_body_tab_order(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let running = input(&shell, 1, cx);
        running.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        tab_to_button(window, "monitor-hooks-tab", false, cx);
        window.press("tab", cx);
        let first = format!("monitor-agent-{}", loki_metris_core::ai_tool_name(AiTool::Codex));
        let tab = window.find(first);
        assert_eq!(tab.role(), Some(Role::Tab));
        assert_eq!(tab.focused(), Some(true), "原生正文首项必须紧随页头，不能因 deferred 留到文档尾部");
    }).unwrap();
}

/// 原生 Dialog 的 deferred 层须在 sticky 之上，点击背景页头不能穿透切换路由。
#[gpui_kit::test]
fn monitor_sticky_toolbar_does_not_cover_or_pierce_native_modal(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    cx.update_window(handle, |_, window, cx| {
        let initial = window.find("monitor-workbench-tab").bounds();
        let viewport = shell.read(cx).page_scroll.handle.bounds();
        let pinned_click = point(initial.center().x - viewport.left(), initial.center().y - px(32.) - viewport.top());
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-120.))), cx);
        shell.update(cx, |shell, cx| shell.open_release_notes(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("release-notes-surface").is_some());
        window.click_at("page-content", pinned_click, cx);
        assert!(shell.read(cx).monitor.route == Route::Management, "modal 覆盖期间不能命中背景 sticky tab");
    }).unwrap();
}

/// AX 默认回退连续分发按下与抬起，粘滞层不能要求中间绘帧才能导航。
#[gpui_kit::test]
fn monitor_sticky_sidebar_navigation_without_intermediate_frame(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    let draft = "未保存的侧栏命中检查\n返回监控后应保留";
    cx.update_window(handle, |_, window, cx| {
        input(&shell, 0, cx).update(cx, |input, cx| input.replace_all(draft, window, cx));
        window.render_frame(cx);
    }).unwrap();
    for (scale, height) in [(1., 700.), (2., 700.), (1., 929.), (2., 929.)] {
        cx.simulate_window_scale_factor_change(handle, scale);
        cx.simulate_window_resize(handle, gpui_kit::size(px(1440.), px(height)));
        cx.update_window(handle, |_, window, cx| {
            for delta in [0., -120., -10000.] {
                window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(delta))), cx);
                window.render_frame(cx);
                let settings = window.find("nav-settings");
                assert_eq!(settings.role(), Some(Role::Button));
                assert!(settings.label().is_some());
                let sidebar = window.find("sidebar").bounds();
                let main = window.find("main-content").bounds();
                let footer = window.find("monitor-profile-save-bar").bounds();
                let at = settings.bounds().center();
                assert!(sidebar.contains(&at));
                assert!(at.x < main.left());
                assert!(!footer.contains(&at));
                // 与默认 AX Click 同样连续派发，不用 click helper 的移入和中间绘帧。
                for event in [
                    gpui_kit::PlatformInput::MouseDown(gpui_kit::MouseDownEvent {
                        button: gpui_kit::MouseButton::Left, position: at,
                        modifiers: Default::default(), click_count: 1, first_mouse: false,
                    }),
                    gpui_kit::PlatformInput::MouseUp(gpui_kit::MouseUpEvent {
                        button: gpui_kit::MouseButton::Left, position: at,
                        modifiers: Default::default(), click_count: 1,
                    }),
                ] { window.dispatch_event(event, cx); }
                assert_eq!(shell.read(cx).page, Page::Settings,
                    "scale={scale}, height={height}, scroll={delta}, center={at:?}");
                window.render_frame(cx);
                window.click("nav-monitor", cx);
                assert_eq!(shell.read(cx).page, Page::Monitor);
                assert!(shell.read(cx).monitor.route == Route::Management);
                assert_eq!(input(&shell, 0, cx).read(cx).value().as_ref(), draft);
                assert_eq!(shell.read(cx).monitor.catalog.editors[0].draft.hooks[2].image, "sticky-gif");
            }
        }).unwrap();
    }
}
