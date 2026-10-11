// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：指针在桌宠内时切换键盘输入模式不能伪装为离开并清空分页栏。；设置滑块键盘步长为1；观察值更新会保存辅助增减而程序同步相同值不重复提交。

/// 指针在桌宠内时切换键盘输入模式不能伪装为离开并清空分页栏。
#[gpui_kit::test]
fn keyboard_navigation_preserves_pointer_hover_after_deferred_paint(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, KeyDownEvent, Keystroke, MouseExitEvent, MouseMoveEvent};
    let f = fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        window.click("pet-overlay", cx);
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        let outside = point(px(-1.), px(-1.));
        window.dispatch_event(MouseMoveEvent { position: outside, ..Default::default() }.to_platform_input(), cx);
        window.dispatch_event(MouseExitEvent { position: outside, ..Default::default() }.to_platform_input(), cx);
        window.render_frame(cx);
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.is_window_active());
        assert!(window.focused(cx).is_some());
        // 稳定焦点本身不显示分页栏；接下来实际移入才建立 hover 来源。
        assert!(window.try_find("pet-pager").is_none());
        window.dispatch_event(MouseMoveEvent { position: point(px(20.), px(52.)), ..Default::default() }.to_platform_input(), cx);
        window.render_frame(cx);
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("pet-pager").is_some());
        let result = window.dispatch_event(KeyDownEvent {
            keystroke: Keystroke::parse("right").unwrap(),
            is_held: false,
            prefer_character_input: false,
        }.to_platform_input(), cx);
        assert!(!result.propagate);
        window.render_frame(cx);
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 1);
    cx.update_window(f.overlay, |_, window, cx| {
        // 排除键盘焦点作为分页栏来源，必须由仍在窗口内的指针独立维持。
        window.blur(cx);
        window.render_frame(cx);
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.is_window_active());
        assert!(window.focused(cx).is_none());
        assert!(window.try_find("pet-pager").is_some(), "keyboard modality must not dispatch a fake pointer exit");
    }).unwrap();
}

/// 设置滑块键盘步长为1；观察值更新会保存辅助增减而程序同步相同值不重复提交。
#[gpui_kit::test]
fn settings_size_keyboard_step_persists(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f
        .owner
        .read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.click("pet-size-control", cx);
    })
    .unwrap();
    settle(cx, &f.owner);
    let before = f
        .owner
        .read_with(cx, |owner, _| owner.settings.pet_window.pet_size);
    cx.simulate_keystrokes(settings, "right");
    settle(cx, &f.owner);
    assert_eq!(
        f.owner
            .read_with(cx, |owner, _| owner.settings.pet_window.pet_size),
        before + 1
    );
}
