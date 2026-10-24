//! 原生确认等待使用真实 GPUI timer；保存和退出接管仍通过实际服务与临时设置文件验证。

use super::{tests::{fixture, settle}, *};
use gpui_kit::TestAppContext;

/// WM 尚未确认时 UI 继续处理其它事件，磁盘保持旧值；确认后只提交一次字段意图。
#[gpui_kit::test]
fn native_confirmation_is_async_and_precedes_preference_commit(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::delay_settle(3);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Locked(true), cx));
    cx.run_until_parked();
    f.owner.read_with(cx, |owner, _| {
        assert!(owner.busy && owner.native_mutating);
        assert!(owner.scheduled.is_some());
    });
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(loki_metris_service::usage_service::pet_background_task_count(&f.service), 0);
    let responsive = cx.new(|_| false);
    responsive.update(cx, |value, _| *value = true);
    assert!(responsive.read_with(cx, |value, _| *value));
    settle(cx, &f.owner);
    assert!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.locked && !owner.native_mutating));
    let stored = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert!(stored.pet_window.locked);
    assert_eq!(f.service.pet_settings_revision(), 1);
}

/// 退出发生于 WM 等待阶段时，原始字段意图仍由 shutdown plan 接管，不能随 UI task 取消丢失。
#[gpui_kit::test]
fn shutdown_takes_preference_waiting_for_native_confirmation(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    host::delay_settle(100);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Layout(PetLayout::Row3), cx));
    cx.run_until_parked();
    assert_eq!(loki_metris_service::usage_service::pet_background_task_count(&f.service), 0);
    let plan = cx.update(take_shutdown_plan);
    assert_eq!(plan.changes.len(), 1);
    assert!(matches!(plan.changes[0].0, PetPreferenceChange::Layout(PetLayout::Row3)));
    f.service.finish_with_pet(plan);
    let stored = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(stored.pet_window.layout, PetLayout::Row3);
    host::delay_settle(0);
}

/// 原生读回失败不能写文件；旧偏好重新应用且确认回滚后恢复操作入口。
#[gpui_kit::test]
fn failed_native_confirmation_rolls_back_before_releasing_queue(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::fail_next_settle();
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Locked(true), cx));
    settle(cx, &f.owner);
    f.owner.read_with(cx, |owner, _| {
        assert!(!owner.settings.pet_window.locked);
        assert!(!owner.native_mutating && !owner.busy);
        assert_eq!(owner.last_error.as_ref().unwrap().code, "error.monitor.petApplyFailed");
        assert_eq!(owner.mouse_state.confirmed(), Some(false));
    });
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert!(!loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.locked);
}

/// WM 长期不确认且回滚也未确认时，明确标记未知并禁止保存中间位置，不能假称恢复成功。
#[gpui_kit::test]
fn native_and_rollback_timeouts_preserve_unknown_geometry(cx: &mut TestAppContext) {
    let f = fixture(cx);
    host::delay_settle(1000);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::AlwaysOnTop(false), cx));
    settle(cx, &f.owner);
    f.owner.read_with(cx, |owner, _| {
        assert!(owner.settings.pet_window.always_on_top);
        assert!(!owner.native_mutating && !owner.busy);
        assert!(owner.display_geometry_unconfirmed);
        assert_eq!(owner.mouse_state.confirmed(), None);
        assert_eq!(owner.last_error.as_ref().unwrap().code, "error.monitor.petApplyFailed");
    });
    assert_eq!(f.service.pet_settings_revision(), 0);
    host::delay_settle(0);
}

/// 关闭穿透的真实后台保存被锁住时，用户仍可移动；退出接管最后位置且保留已经接受的穿透关闭。
#[gpui_kit::test]
fn mouse_only_save_keeps_user_drag_in_shutdown_plan(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let config = f.root.path().join("monitor");
    loki_metris_service::monitor::settings::update_monitor_settings(&config, |settings| settings.pet_window.mouse_passthrough = true).unwrap();
    f.owner.update(cx, |owner, cx| {
        owner.settings.pet_window.mouse_passthrough = true;
        owner.apply_mouse_passthrough(cx).unwrap();
    });
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    let held = loki_metris_service::usage_service::block_pet_mutations(&f.service);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::MousePassthrough(false), cx));
    cx.run_until_parked();
    assert_eq!(loki_metris_service::usage_service::pet_background_task_count(&f.service), 1);
    f.owner.read_with(cx, |owner, _| {
        assert!(owner.busy);
        assert!(!owner.native_mutating);
        assert_eq!(owner.mouse_state.confirmed(), Some(false));
    });
    let final_position = PetOverlayPosition { x: 110, y: 252 };
    f.owner.update(cx, |owner, cx| owner.moved(Bounds::new(point(px(110.), px(220.)), size(px(128.), px(160.))), 1., cx));
    let plan = cx.update(take_shutdown_plan);
    assert_eq!(plan.position, Some(final_position));
    held.release();
    f.service.finish_with_pet(plan);
    let stored = loki_metris_service::monitor::settings::load_monitor_settings(&config).unwrap();
    assert!(!stored.pet_window.mouse_passthrough);
    assert_eq!(stored.pet_overlay_position, Some(final_position));
}
