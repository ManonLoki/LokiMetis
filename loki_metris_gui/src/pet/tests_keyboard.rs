// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：指针在桌宠内时切换键盘输入模式不能伪装为离开并清空分页栏。；设置滑块键盘步长为1；观察值更新会保存辅助增减而程序同步相同值不重复提交。

/// 完整原生按键保留松开激活和逐帧焦点遍历，不使用只派发KeyDown的快捷模拟。
fn settings_press(handle: gpui_kit::AnyWindowHandle, name: &'static str, cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, KeyDownEvent, KeyUpEvent, Keystroke};
    for down in [true, false] {
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
    cx.run_until_parked();
}

/// 独立设置窗首次键盘进入不依赖鼠标，正向第二步只移动一个原生焦点。
#[gpui_kit::test]
fn settings_first_tab_enters_controls_without_mouse(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        assert!(window.focused(cx).is_none(), "创建设置窗不应自动激活按钮");
    }).unwrap();
    settings_press(settings, "tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pet-settings-close").focused(), Some(true),
            "首次Tab应进入关闭按钮，不能要求先用鼠标建立焦点");
    }).unwrap();
    settings_press(settings, "tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pet-mouse-recover").focused(), Some(true),
            "未知穿透状态的恢复按钮必须保留在原生Tab顺序中");
    }).unwrap();
    for id in ["pet-layout-Single", "pet-layout-Row", "pet-layout-Column",
        "pet-layout-Row3", "pet-layout-Column3", "pet-layout-Grid", "pet-size-control"] {
        settings_press(settings, "tab", cx);
        cx.update_window(settings, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(id).focused(), Some(true), "Tab必须依次到达{id}");
        }).unwrap();
    }
}

/// 反向首次进入最后按钮，后续正向原生遍历绕回关闭按钮。
#[gpui_kit::test]
fn settings_first_shift_tab_enters_last_control(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        assert!(window.focused(cx).is_none());
    }).unwrap();
    settings_press(settings, "shift-tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pet-hide").focused(), Some(true));
    }).unwrap();
    settings_press(settings, "tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pet-settings-close").focused(), Some(true));
    }).unwrap();
}

/// 关闭释放旧设置视图；新窗口不继承旧焦点，仍可用首次Tab进入。
#[gpui_kit::test]
fn settings_reopened_window_keeps_first_tab_entry(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let mut previous = None;
    for _ in 0..2 {
        f.owner.update(cx, |owner, cx| owner.open_settings(cx));
        cx.run_until_parked();
        let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
        assert_ne!(Some(settings), previous);
        cx.update_window(settings, |_, window, cx| {
            window.activate_window();
            window.render_frame(cx);
            assert!(window.focused(cx).is_none());
        }).unwrap();
        settings_press(settings, "tab", cx);
        cx.update_window(settings, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("pet-settings-close").focused(), Some(true));
        }).unwrap();
        settings_press(settings, "enter", cx);
        cx.run_until_parked();
        assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_none()));
        assert!(cx.update_window(settings, |_, _, _| ()).is_err());
        previous = Some(settings);
    }
}

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
    settings_press(settings, "right", cx);
    settle(cx, &f.owner);
    assert_eq!(
        f.owner
            .read_with(cx, |owner, _| owner.settings.pet_window.pet_size),
        before + 1
    );
    let slider_focus = cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.focused(cx).expect("滑块应仍持有实际键盘焦点")
    }).unwrap();
    settings_press(settings, "tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pet-top").focused(), Some(true));
    }).unwrap();
    settings_press(settings, "shift-tab", cx);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.focused(cx), Some(slider_focus));
    }).unwrap();
}
