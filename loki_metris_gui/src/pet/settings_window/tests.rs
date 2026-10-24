//! 真实虚拟窗口与 defer 时序覆盖配置窗显示租约，不调用实际用户目录或 AppKit。

use super::super::tests::{fixture, settle};
use gpui_kit::{AppContext, TestAppContext, point, px, test::TestWindowExt};

/// 新建设置窗的原生属性失败必须呈现错误且销毁窗口；下一次重试能建立可用入口。
#[gpui_kit::test]
fn failed_native_settings_creation_is_visible_and_retryable(cx: &mut TestAppContext) {
    let f = fixture(cx);
    super::super::host::fail_next_apply();
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    f.owner.read_with(cx, |owner, _| {
        assert!(owner.settings_window.is_none());
        assert_eq!(owner.last_error.as_ref().unwrap().code, "error.monitor.petApplyFailed");
    });
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    f.owner.read_with(cx, |owner, _| {
        assert!(owner.settings_window.is_some());
        assert!(owner.last_error.is_none());
    });
    assert_eq!(f.service.pet_settings_revision(), 0);
}

/// 真实轨道点击跨过缩进后的指示器两端，仍提交完整尺寸范围并持久化。
#[gpui_kit::test]
fn size_track_endpoints_save_full_range(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    let range = cx.update(|cx| {
        let layout = f.owner.read(cx).settings.pet_window.layout;
        super::super::range_for(Some(f.overlay), layout, cx)
    });
    for (right_end, expected) in [(true, range.max), (false, range.min)] {
        cx.update_window(settings, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("slider-bar-container").bounds();
            let x = if right_end { bounds.size.width - px(1.) } else { px(1.) };
            window.click_at("slider-bar-container", point(x, bounds.size.height / 2.), cx);
        }).unwrap();
        settle(cx, &f.owner);
        assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size), expected);
        let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
        assert_eq!(saved.pet_window.pet_size, expected);
    }
}

/// 保存失败后恢复已确认尺寸；后续键盘输入从回滚值继续，而非失败的指针值。
#[gpui_kit::test]
fn failed_size_click_restores_keyboard_start_value(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size);
    let directory = f.root.path().join("monitor");
    std::fs::create_dir_all(&directory).unwrap();
    let path = loki_metris_service::monitor::settings::settings_path(&directory);
    std::fs::write(&path, b"{broken").unwrap();
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("slider-bar-container").bounds();
        window.click_at("slider-bar-container", point(bounds.size.width - px(1.), bounds.size.height / 2.), cx);
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size), before);
    assert!(f.owner.read_with(cx, |owner, _| owner.last_error.is_some()));
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    std::fs::remove_file(&path).unwrap();
    cx.simulate_keystrokes(settings, "right");
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size), before + 1);
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&directory).unwrap();
    assert_eq!(saved.pet_window.pet_size, before + 1);
}

/// 边界方向键保持原生 range 的夹紧语义，反向第一步立即生效。
#[gpui_kit::test]
fn size_keyboard_reverses_immediately_after_range_boundary(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    let range = cx.update(|cx| {
        let layout = f.owner.read(cx).settings.pet_window.layout;
        super::super::range_for(Some(f.overlay), layout, cx)
    });
    cx.update_window(settings, |_, window, cx| window.click("pet-size-control", cx)).unwrap();
    settle(cx, &f.owner);
    for (keys, expected) in [("end right left", range.max - 1), ("home left right", range.min + 1)] {
        for key in keys.split_whitespace() {
            cx.simulate_keystrokes(settings, key);
            settle(cx, &f.owner);
        }
        assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size), expected);
        let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
        assert_eq!(saved.pet_window.pet_size, expected);
    }
}

/// 尚未创建时关闭也必须撤销显示，不能在下一个回合迟到出现。
#[gpui_kit::test]
fn close_before_defer_does_not_create_settings_window(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let before = cx.update(|cx| cx.windows().len());
    f.owner.update(cx, |owner, cx| {
        owner.open_settings(cx);
        owner.close_settings(cx);
    });
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_none()));
    assert_eq!(cx.update(|cx| cx.windows().len()), before);
}

/// 隐藏桌宠撤销整个配置入口，而不只关闭当前已经创建的窗。
#[gpui_kit::test]
fn hide_before_defer_does_not_create_settings_window(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let before = cx.update(|cx| cx.windows().len());
    f.owner.update(cx, |owner, cx| {
        owner.open_settings(cx);
        owner.hide(cx);
    });
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_none()));
    assert_eq!(cx.update(|cx| cx.windows().len()), before);
}

/// 同回合多个打开动作与取消只留下最新一窗；随后打开复用现有窗。
#[gpui_kit::test]
fn repeated_open_and_close_keep_only_latest_window(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let before = cx.update(|cx| cx.windows().len());
    f.owner.update(cx, |owner, cx| {
        owner.open_settings(cx);
        owner.open_settings(cx);
        owner.close_settings(cx);
        owner.open_settings(cx);
    });
    cx.run_until_parked();
    let handle = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert_eq!(cx.update(|cx| cx.windows().len()), before + 1);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings_window), Some(handle));
    assert_eq!(cx.update(|cx| cx.windows().len()), before + 1);
}

/// 退出排空开始后，之前排队或新发出的打开意图都不能建立辅助窗。
#[gpui_kit::test]
fn shutdown_rejects_pending_and_new_settings_open(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let before = cx.update(|cx| cx.windows().len());
    cx.update(|cx| {
        cx.set_global(super::super::Manager(f.owner.clone()));
        f.owner.update(cx, |owner, cx| owner.open_settings(cx));
        let _ = super::super::take_shutdown_plan(cx);
        f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    });
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_none()));
    assert_eq!(cx.update(|cx| cx.windows().len()), before);
}

/// 测试宿主分两步完成原生关闭；旧窗的迟到回调不得清除重建后的窗口。
#[gpui_kit::test]
fn late_native_close_keeps_reopened_window(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let old = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert!(gpui_kit::VisualTestContext::from_window(old, cx).simulate_close());
    assert!(f.owner.read_with(cx, |owner, _| owner.settings_window.is_none()));
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let reopened = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert_ne!(old, reopened);
    assert!(gpui_kit::VisualTestContext::from_window(old, cx).simulate_close());
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings_window), Some(reopened));
    cx.update_window(old, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(cx.update_window(old, |_, _, _| ()).is_err());
    assert!(cx.update_window(reopened, |_, _, _| ()).is_ok());
}
