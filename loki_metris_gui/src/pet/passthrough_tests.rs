//! 穿透控制沿用真实服务回执；测试适配器只验证映射，不冒充OS投递。

use super::{passthrough::toggle_mouse_passthrough, tests::{fixture, settle}, *};
use gpui_kit::{TestAppContext, test::TestWindowExt};

/// 公开bounds观察独立于被测方法，防止穿透误走几何路径。
fn watch_bounds(handle: AnyWindowHandle, cx: &mut TestAppContext) -> (Entity<usize>, gpui_kit::Subscription) {
    let count = cx.new(|_| 0usize);
    let subscription = cx.update_window(handle, |_, window, cx| {
        count.update(cx, |_, cx| cx.observe_window_bounds(window, |count, _, _| *count += 1))
    }).unwrap();
    (count, subscription)
}

/// 实际配置控件提交独立穿透，关闭重开仍可恢复；成功和失败都不触碰bounds。
#[gpui_kit::test]
fn mouse_passthrough_control_persists_and_corrupt_store_rolls_back_without_geometry(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    let (bounds_events, _observe) = watch_bounds(f.overlay, cx);
    let bounds = cx.update_window(f.overlay, |_, window, _| window.bounds()).unwrap();
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.click("pet-mouse-passthrough", cx);
    }).unwrap();
    settle(cx, &f.owner);
    assert!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(true));
    let path = f.root.path().join("monitor/monitor-settings.json");
    let saved = std::fs::read(&path).unwrap();
    let stored = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert!(stored.pet_window.mouse_passthrough);
    assert!(!stored.pet_window.locked);
    f.owner.update(cx, |owner, cx| { owner.close_settings(cx); owner.open_settings(cx); });
    cx.run_until_parked();
    let reopened = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert_ne!(settings, reopened);
    std::fs::write(&path, b"{broken").unwrap();
    cx.update_window(reopened, |_, window, cx| { window.render_frame(cx); window.click("pet-mouse-passthrough", cx); }).unwrap();
    settle(cx, &f.owner);
    assert!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(true));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)), Some("error.monitor.settingsInvalid"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    std::fs::write(&path, saved).unwrap();
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    cx.update(toggle_mouse_passthrough);
    settle(cx, &f.owner);
    assert!(!f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(bounds_events.read_with(cx, |count, _| *count), 0);
    assert_eq!(cx.update_window(f.overlay, |_, window, _| window.bounds()).unwrap(), bounds);
}

/// 原生拒绝不得保存，后续意图继续执行；原生未知时允许相同false值恢复。
#[gpui_kit::test]
fn mouse_passthrough_native_failure_and_same_value_recovery(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let (bounds_events, _observe) = watch_bounds(f.overlay, cx);
    f.owner.update(cx, |owner, cx| {
        owner.mouse_state.fail_next = true;
        owner.change(PetPreferenceChange::MousePassthrough(true), cx);
        owner.change(PetPreferenceChange::TurnPage(1), cx);
    });
    settle(cx, &f.owner);
    assert!(!f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 4);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(false));
    assert_eq!(f.service.pet_settings_revision(), 1, "only paging may be persisted");
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    f.owner.update(cx, |owner, _| owner.mouse_state.confirmed = None);
    let calls = f.owner.read_with(cx, |owner, _| owner.mouse_state.calls);
    cx.update(toggle_mouse_passthrough);
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(false));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.calls), calls + 1);
    assert_eq!(f.service.pet_settings_revision(), 1);
    assert_eq!(bounds_events.read_with(cx, |count, _| *count), 0);
}

/// 实际store锁阻塞在途保存，关闭配置窗后已接受的关闭仍最终落盘。
#[gpui_kit::test]
fn mouse_passthrough_busy_queue_survives_settings_close(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let held = loki_metris_service::usage_service::block_pet_mutations(&f.service);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(true), cx));
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.busy));
    f.owner.update(cx, |owner, cx| {
        owner.change(PetPreferenceChange::MousePassthrough(false), cx);
        owner.close_settings(cx);
        owner.open_settings(cx);
    });
    cx.run_until_parked();
    held.release();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(false));
    assert!(!loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.mouse_passthrough);
    assert_eq!(f.service.pet_settings_revision(), 2);
}

/// 退出收尾覆盖尚未defer和已经接受两条路径，最终关闭不能被在途开启覆盖。
#[gpui_kit::test]
fn mouse_passthrough_shutdown_drains_final_off(cx: &mut TestAppContext) {
    for accepted in [false, true] {
        let f = fixture(cx);
        cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
        let held = loki_metris_service::usage_service::block_pet_mutations(&f.service);
        if accepted {
            f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(true), cx));
            cx.run_until_parked();
            assert_eq!(loki_metris_service::usage_service::pet_background_task_count(&f.service), 1);
        }
        let plan = cx.update(|cx| {
            f.owner.update(cx, |owner, cx| {
                if !accepted { owner.change(PetPreferenceChange::MousePassthrough(true), cx); }
                owner.change(PetPreferenceChange::MousePassthrough(false), cx);
            });
            take_shutdown_plan(cx)
        });
        assert_eq!(plan.changes.len(), if accepted { 1 } else { 2 });
        held.release();
        f.service.finish_with_pet(plan);
        assert!(!loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.mouse_passthrough);
    }
}

/// 启动读取旧true尚未回执时显式关闭，迟到读取不得覆盖已接受的false。
#[gpui_kit::test]
fn mouse_passthrough_initial_read_cannot_overwrite_explicit_off(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.service.change_pet_preferences(PetPreferenceChange::MousePassthrough(true), PetSizeRange { min: 64, max: 135 }).unwrap().blocking_recv().unwrap().unwrap();
    let held = loki_metris_service::usage_service::block_pet_mutations(&f.service);
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    assert!(f.owner.read_with(cx, |owner, _| owner.loading));
    cx.update(toggle_mouse_passthrough);
    cx.run_until_parked();
    held.release();
    settle(cx, &f.owner);
    for _ in 0..500 {
        cx.run_until_parked();
        if f.owner.read_with(cx, |owner, _| !owner.loading) { break; }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!f.owner.read_with(cx, |owner, _| owner.loading));
    assert!(!f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(false));
    assert!(!loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.mouse_passthrough);
}

/// 缺失浮窗时拒收原生属性并继续分页，不把测试假平台成功当作可用窗口。
#[gpui_kit::test]
fn mouse_passthrough_missing_overlay_rejects_and_continues_queue(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.overlay = None;
        owner.change(PetPreferenceChange::MousePassthrough(true), cx);
        owner.change(PetPreferenceChange::TurnPage(1), cx);
    });
    settle(cx, &f.owner);
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert!(!saved.pet_window.mouse_passthrough);
    assert_eq!(saved.pet_window.focused_slot, 4);
    assert_eq!(f.service.pet_settings_revision(), 1);
}

/// 真正保存失败后原生回滚也失败，未知状态必须披露且可通过托盘同值关闭恢复。
#[gpui_kit::test]
fn mouse_passthrough_failed_save_and_failed_rollback_remain_recoverable(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let path = f.root.path().join("monitor/monitor-settings.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"{broken").unwrap();
    let (events, _observe) = watch_bounds(f.overlay, cx);
    let held = loki_metris_service::usage_service::block_pet_mutations(&f.service);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(true), cx));
    cx.run_until_parked();
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(true));
    f.owner.update(cx, |owner, _| owner.mouse_state.fail_next = true);
    held.release();
    settle(cx, &f.owner);
    assert!(!f.owner.read_with(cx, |owner, _| owner.settings.pet_window.mouse_passthrough));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), None);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)), Some("error.monitor.petApplyFailed"));
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    assert_eq!(f.service.pet_settings_revision(), 0);
    loki_metris_service::monitor::settings::save_monitor_settings(&f.root.path().join("monitor"), &Default::default()).unwrap();
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::TurnPage(1), cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), None);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)), None, "unrelated successful paging clears only its own feedback");
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f.owner.read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| { window.render_frame(cx); assert!(window.try_find("pet-mouse-recover").is_some(), "native unknown must remain visible after unrelated success"); }).unwrap();
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    cx.update(toggle_mouse_passthrough);
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.mouse_state.confirmed()), Some(false));
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)), None);
    assert_eq!(f.service.pet_settings_revision(), 1, "only paging changes store; same authoritative false remains a no-op");
    cx.update_window(settings, |_, window, cx| { window.render_frame(cx); assert!(window.try_find("pet-mouse-recover").is_none()); }).unwrap();
    assert_eq!(events.read_with(cx, |count, _| *count), 0);
}
