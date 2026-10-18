//! 虚拟宿主只断言显隐命令和真实偏好文件；多屏数值映射另以 core 输入检查。

use super::*;
use crate::pet::{tests::fixture, geometry};
use gpui_kit::{AppContext, TestAppContext, Bounds, point, px, size};
use loki_metris_core::{PetOverlayPosition, PetWindowPreferences, window_placement::*};

/// WM 尚未确认显示时保留 reveal 意图，超时重试不能误把只接受的 map 请求当作已显示。
#[gpui_kit::test]
fn display_timeout_preserves_unconfirmed_reveal_for_retry(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, _| owner.settings_ready = true);
    host::delay_settle(1000);
    f.owner.update(cx, |owner, cx| owner.request_display(cx));
    settle_query(&f.owner, cx);
    f.owner.read_with(cx, |owner, _| {
        assert!(!owner.display_applying);
        assert!(owner.display_geometry_unconfirmed);
        assert!(owner.display_request.as_ref().unwrap().reveal);
    });
    host::delay_settle(0);
}

/// 查询使用真实服务，假时钟与有界墙钟让步不触碰用户配置。
fn settle_query(owner: &Entity<Controller>, cx: &mut TestAppContext) {
    for _ in 0..500 {
        cx.run_until_parked();
        if owner.read_with(cx, |owner, _| !owner.loading && !owner.display_applying) { return; }
        cx.executor().advance_clock(std::time::Duration::from_millis(10));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("pet display/read receipt did not settle");
}

/// 默认展示不能把坏文件变为成功恢复，也不能保存默认几何覆盖原字节。
#[gpui_kit::test]
fn startup_read_failure_shows_defaults_without_saving(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::take_visibility_events(f.overlay);
    let directory = f.root.path().join("monitor");
    std::fs::create_dir_all(&directory).unwrap();
    let path = crate::monitor::settings::settings_path(&directory);
    std::fs::write(&path, b"{broken").unwrap();
    f.owner.update(cx, |owner, cx| {
        owner.geometry_restored = false;
        owner.request_display(cx);
    });
    cx.run_until_parked();
    assert_eq!(host::take_visibility_events(f.overlay), [true]);
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_query(&f.owner, cx);
    assert!(f.owner.read_with(cx, |owner, _| owner.visible && !owner.geometry_restored));
    f.service.finish();
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(std::fs::read(path).unwrap(), b"{broken");
}

/// 读取在途时隐藏，成功回执只能接受偏好，不能发出迟到显示命令。
#[gpui_kit::test]
fn initial_settings_receipt_after_hide_never_reveals_pet(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::take_visibility_events(f.overlay);
    f.owner.update(cx, |owner, cx| {
        owner.geometry_restored = false;
        owner.request_display(cx);
    });
    cx.run_until_parked();
    assert_eq!(host::take_visibility_events(f.overlay), [true]);
    f.owner.update(cx, |owner, cx| {
        owner.refresh(cx);
        assert!(owner.loading);
        owner.hide(cx);
    });
    settle_query(&f.owner, cx);
    let events = host::take_visibility_events(f.overlay);
    assert!(!events.is_empty() && events.iter().all(|visible| !visible));
    assert!(f.owner.read_with(cx, |owner, _| !owner.visible && owner.geometry_restored));
}

/// 默认浮窗已经显示后迟到的真实偏好只回填几何，不能再次发出聚焦显示命令。
#[gpui_kit::test]
fn successful_initial_settings_do_not_reveal_default_pet_twice(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::take_visibility_events(f.overlay);
    f.owner.update(cx, |owner, cx| {
        owner.geometry_restored = false;
        owner.request_display(cx);
    });
    cx.run_until_parked();
    assert_eq!(host::take_visibility_events(f.overlay), [true]);
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_query(&f.owner, cx);
    assert!(host::take_visibility_events(f.overlay).is_empty());
    assert!(f.owner.read_with(cx, |owner, _| owner.geometry_restored));
}

/// 创建失败没有活窗时设置回执不能锁死迁屏标志，后续分页仍走真实偏好服务。
#[gpui_kit::test]
fn missing_overlay_does_not_starve_preference_queue(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.overlay = None;
        owner.geometry_restored = false;
        owner.request_display(cx);
    });
    assert!(!f.owner.read_with(cx, |owner, _| owner.display_applying));
    cx.executor().advance_clock(std::time::Duration::from_secs(2));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_query(&f.owner, cx);
    f.owner.update(cx, |owner, cx| owner.change(loki_metris_core::PetPreferenceChange::TurnPage(1), cx));
    crate::pet::tests::settle(cx, &f.owner);
    assert_eq!(crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.focused_slot, 4);
}

/// 同一回合交错显隐仅执行最后显示；退出撤销尚未运行的显示和隐藏。
#[gpui_kit::test]
fn latest_visibility_intent_and_shutdown_cancel_stale_native_commands(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::take_visibility_events(f.overlay);
    f.owner.update(cx, |owner, cx| {
        owner.hide(cx);
        owner.show(cx);
        owner.hide(cx);
        owner.show(cx);
    });
    cx.run_until_parked();
    assert_eq!(host::take_visibility_events(f.overlay), [true]);
    cx.update(|cx| cx.set_global(crate::pet::Manager(f.owner.clone())));
    cx.update(|cx| {
        f.owner.update(cx, |owner, cx| { owner.hide(cx); owner.show(cx); });
        crate::pet::take_shutdown_plan(cx);
    });
    cx.run_until_parked();
    assert!(host::take_visibility_events(f.overlay).is_empty());
}

/// 写入回执在迁屏等待期间推进队列，不能把队头重新排到尾部改变最后意图。
#[gpui_kit::test]
fn pending_preferences_keep_fifo_while_display_waits(cx: &mut TestAppContext) {
    use loki_metris_core::PetPreferenceChange;
    let f = fixture(cx);
    let (bounds, scale) = cx.update_window(f.overlay, |_, window, _| (host::bounds(window), window.scale_factor())).unwrap();
    let baseline = f.service.save_pet_position(PetOverlayPosition {
        x: (f32::from(bounds.origin.x) * scale).round() as i32,
        y: ((f32::from(bounds.origin.y) + geometry::HEADER_HEIGHT as f32) * scale).round() as i32,
    }).unwrap().blocking_recv().unwrap().unwrap();
    let revision = f.service.pet_settings_revision();
    f.owner.update(cx, |owner, cx| {
        owner.settings = baseline;
        owner.display_applying = true;
        owner.change(PetPreferenceChange::Locked(true), cx);
        owner.change(PetPreferenceChange::Locked(false), cx);
        // 对应较早 mutation 已结束、但原生读回仍等待的推进入口。
        owner.next_pending(cx);
        owner.display_applying = false;
        owner.next_pending(cx);
    });
    crate::pet::tests::settle(cx, &f.owner);
    let saved = crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert!(!saved.pet_window.locked);
    assert_eq!(f.service.pet_settings_revision() - revision, 2, "both opposite field intents must execute in FIFO order");
}

/// 原生迁屏被隐藏取消后迟到的 bounds 仍不可信，不能取消屏蔽并保存中间矩形。
#[gpui_kit::test]
fn cancelled_display_keeps_late_geometry_feedback_blocked(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update(|cx| f.owner.update(cx, |owner, cx| {
        owner.settings_ready = true;
        owner.request_display(cx);
        owner.hide(cx);
        owner.moved(Bounds::new(point(px(-1000.), px(200.)), size(px(128.), px(160.))), 2., cx);
    }));
    cx.run_until_parked();
    cx.executor().advance_clock(std::time::Duration::from_millis(250));
    cx.run_until_parked();
    f.service.finish();
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert!(crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_overlay_position.is_none());
}

/// 迁屏中间反馈不保存；事务结束通过正式 bounds 回调只保存最终画布位置。
#[gpui_kit::test]
fn intermediate_display_bounds_do_not_replace_final_position(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.settings_ready = true;
        owner.request_display(cx);
        for x in [-1024., -512., 0.] {
            owner.moved(Bounds::new(point(px(x), px(120.)), size(px(256.), px(288.))), 2., cx);
        }
    });
    cx.run_until_parked();
    let (bounds, scale) = cx.update_window(f.overlay, |_, window, _| (host::bounds(window), window.scale_factor())).unwrap();
    cx.executor().advance_clock(std::time::Duration::from_millis(220));
    cx.run_until_parked();
    f.service.finish();
    let saved = crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_overlay_position, Some(PetOverlayPosition {
        x: (f32::from(bounds.origin.x) * scale).round() as i32,
        y: ((f32::from(bounds.origin.y) + geometry::HEADER_HEIGHT as f32) * scale).round() as i32,
    }));
    assert_eq!(f.service.pet_settings_revision(), 1);
}

/// 两屏真实单位保持独立：外屏负全局原点，工作区在各自屏内 DIP。
fn desktop() -> crate::native::windowing::DesktopSnapshot {
    use crate::native::windowing::{DesktopSnapshot, NativeDisplay};
    let make = |key: &str, id, x, width, scale, primary| NativeDisplay {
        id: gpui_kit::DisplayId::new(id), global_origin: WindowDipPoint { x, y: 0. },
        area: WindowDisplayArea { key: key.into(), scale, primary,
            work_area: WindowDipRect { origin: WindowDipPoint { x: 0., y: 24. }, size: WindowDipSize { width, height: 1000. } } },
    };
    DesktopSnapshot { pointer: Some("external".into()), #[cfg(target_os = "macos")] global_top: 1200.,
        displays: vec![make("builtin", 1, 0., 1500., 2., true), make("external", 2, -1920., 1920., 1., false)] }
}

/// 显示动作的冻结屏优先于保存屏，跨屏映射保留锚点且顶栏只计入一次。
#[test]
fn frozen_pointer_maps_pet_anchor_without_scaling_negative_desktop_origin() {
    let desktop = desktop();
    let prefs = PetWindowPreferences::default();
    let saved = capture_window_state(&desktop.displays[0].area, WindowDipPoint { x: 1000., y: 500. },
        WindowDipSize { width: 128., height: 160. }, WindowFrameInsets::default(), false).unwrap();
    let (plan, _) = super::plan::placement(&desktop, prefs, Some(&saved), Some("builtin"), WindowFrameInsets::default()).unwrap();
    assert_eq!(plan.display_key, "external");
    assert_eq!(plan.content_size, WindowDipSize { width: 128., height: 160. });
    assert!((plan.origin.x - (1920. - 128.) * saved.anchor.x).abs() < 0.0001);
    assert!(!plan.maximized);
    assert!(super::plan::matches(&capture_window_state(&desktop.displays[1].area, plan.origin, plan.content_size, WindowFrameInsets::default(), false).unwrap(), &plan));
}

/// 旧物理位置转换为唯一外屏屏内 DIP，跨 scale 的另一候选不能按猜测接受。
#[test]
fn legacy_pet_position_requires_unique_screen_and_removes_header_once() {
    let desktop = desktop();
    let saved = super::plan::legacy_state(&desktop, PetWindowPreferences::default(),
        Some(PetOverlayPosition { x: -1000, y: 300 }), 1, WindowFrameInsets::default()).unwrap();
    assert_eq!(saved.display_key, "external");
    assert_eq!(saved.origin, WindowDipPoint { x: 920., y: 268. });
    let mut ambiguous = desktop.clone();
    ambiguous.displays.push(ambiguous.displays[1].clone());
    assert!(super::plan::legacy_state(&ambiguous, PetWindowPreferences::default(),
        Some(PetOverlayPosition { x: -1000, y: 300 }), 1, WindowFrameInsets::default()).is_none());
}

/// Windows 历史外框位置与新客户区位置恢复到同一画布，重复启动不能累积边框偏移。
#[cfg(windows)]
#[test]
fn legacy_and_client_position_versions_resolve_to_the_same_outer_origin() {
    let desktop = desktop();
    let insets = WindowFrameInsets { left: 8., top: 8., right: 8., bottom: 8. };
    let old = super::plan::legacy_state(&desktop, PetWindowPreferences::default(),
        Some(PetOverlayPosition { x: -1000, y: 300 }), 1, insets).unwrap();
    let new = super::plan::legacy_state(&desktop, PetWindowPreferences::default(),
        Some(PetOverlayPosition { x: -992, y: 308 }), 2, insets).unwrap();
    assert_eq!(old, new);
}
