//! 授权与偏好回执使用隔离存储，不弹出真实系统权限，也不发送通知。

use super::*;
use crate::preferences::{Preferences, load_from};
use gpui_kit::TestAppContext;
use std::{cell::Cell, time::Duration};
use tokio::sync::oneshot;

/// 外部偏好线程回执必须在有界时间内完成，不能只推进虚拟时钟。
fn settle(cx: &mut TestAppContext, service: &Service) {
    for _ in 0..500 {
        cx.run_until_parked();
        if !service.state().busy {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("notification preference did not settle");
}

/// 初始值、等值点击与关闭均不请求权限，开启等待期间保持权威旧值并拒绝重复点击。
#[gpui_kit::test]
fn notification_authorizes_only_enable_and_uses_real_saved_receipt(cx: &mut TestAppContext) {
    let writer = Writer::in_memory(Preferences::default());
    let service = Service::new(false, writer.clone());
    let (mut key, initial) = service.state_if_changed(None).unwrap();
    assert!(!initial.enabled);
    assert_eq!(initial.progress.revision, 0);
    assert!(service.state_if_changed(Some(key)).is_none());
    cx.update(|cx| {
        service.set_using(
            false,
            |_| async { panic!("equal mutation must not authorize") },
            cx,
        )
    });
    let calls = Rc::new(Cell::new(0));
    let counted = calls.clone();
    let (sender, receiver) = oneshot::channel();
    cx.update(|cx| {
        service.set_using(
            true,
            move |_| {
                counted.set(counted.get() + 1);
                async { receiver.await.unwrap() }
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(calls.get(), 1);
    assert!(service.state().busy);
    assert_eq!(service.state().desired, Some(true));
    assert!(!service.state().enabled);
    let (busy_key, busy) = service.state_if_changed(Some(key)).unwrap();
    assert!(busy.busy);
    assert_eq!(busy.desired, Some(true));
    assert_eq!(busy.progress.revision, 0);
    key = busy_key;
    cx.update(|cx| {
        service.set_using(
            true,
            |_| async { panic!("busy operation must not authorize") },
            cx,
        )
    });
    assert!(service.state_if_changed(Some(key)).is_none());
    sender.send(Ok(())).unwrap();
    settle(cx, &service);
    let (saved_key, saved) = service.state_if_changed(Some(key)).unwrap();
    assert!(saved.enabled);
    assert!(!saved.busy);
    assert_eq!(saved.progress.revision, 0);
    key = saved_key;
    cx.update(|cx| {
        service.set_using(
            false,
            |_| async { panic!("disable must not authorize") },
            cx,
        )
    });
    let (closing_key, closing) = service.state_if_changed(Some(key)).unwrap();
    assert!(closing.busy);
    assert_eq!(closing.desired, Some(false));
    key = closing_key;
    settle(cx, &service);
    let (disabled_key, disabled) = service.state_if_changed(Some(key)).unwrap();
    assert!(!disabled.enabled);
    assert!(!disabled.busy);
    assert_eq!(disabled.progress.revision, 0);
    assert!(service.state_if_changed(Some(disabled_key)).is_none());
    service.stop();
    writer.finish();
}

/// 保存失败保持旧值；之后恢复存储，成功回执才可以确认本地开启。
#[gpui_kit::test]
fn notification_save_error_keeps_old_value_and_retry_can_recover(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());
    std::fs::create_dir(&file).unwrap();
    cx.update(|cx| service.set_using(true, |_| async { Ok(()) }, cx));
    settle(cx, &service);
    assert_eq!(service.state().error, Some(Error::Save));
    assert!(!service.state().enabled);
    cx.update(|cx| service.set_using(true, |_| async { Err(Error::Denied) }, cx));
    settle(cx, &service);
    assert_eq!(service.state().error, Some(Error::Save));
    assert!(!service.state().enabled);
    assert_eq!(
        service.state().permission,
        loki_metris_core::usage_milestones::NotificationPermission::Denied
    );
    std::fs::remove_dir(&file).unwrap();
    cx.update(|cx| service.set_using(true, |_| async { Ok(()) }, cx));
    settle(cx, &service);
    assert!(service.state().enabled);
    assert!(service.state().error.is_none());
    service.stop();
    writer.finish();
    assert!(
        std::fs::read_to_string(file)
            .unwrap()
            .contains("system_notification=true\n")
    );
}

/// 授权拒绝、未知和超时都保存明确开启意图；重启读取仍开启，权限不足不变成 Allowed。
#[gpui_kit::test]
fn notification_authorization_failures_preserve_saved_intent(cx: &mut TestAppContext) {
    use loki_metris_core::usage_milestones::NotificationPermission as Permission;
    for error in [Error::Denied, Error::Unavailable, Error::Timeout] {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("preferences.txt");
        let (_, writer) = load_from(Some(file.clone()));
        let service = Service::new(false, writer.clone());
        cx.update(|cx| service.set_using(true, move |_| async move { Err(error) }, cx));
        settle(cx, &service);
        let state = service.state();
        assert!(state.enabled);
        assert_eq!(state.error, Some(error));
        assert_eq!(
            state.permission,
            if error == Error::Denied {
                Permission::Denied
            } else {
                Permission::Unavailable
            }
        );
        assert_ne!(state.delivery_permission(), Permission::Allowed);
        service.stop();
        writer.finish();
        let (preferences, rereader) = load_from(Some(file));
        let restored = Service::new(preferences.system_notification, rereader.clone());
        assert!(restored.state().enabled);
        assert_eq!(restored.state().permission, Permission::Unavailable);
        restored.stop();
        rereader.finish();
    }
}

/// 缺少原生发送器仍呈现只读 OS 权限，但不能给领域发送器提供 Allowed。
#[gpui_kit::test]
fn notification_permission_does_not_hide_missing_host(cx: &mut TestAppContext) {
    use loki_metris_core::usage_milestones::NotificationPermission as Permission;
    let writer = Writer::in_memory(Preferences::default());
    let service = Service::new(true, writer.clone());
    cx.update(|cx| service.refresh_permission_using(async { Permission::Allowed }, cx));
    cx.run_until_parked();
    let state = service.state();
    assert!(state.enabled);
    assert_eq!(state.permission, Permission::Allowed);
    assert!(!state.host_available);
    assert_eq!(state.delivery_permission(), Permission::Unavailable);
    assert_eq!(
        menu_label("zh-CN", &state),
        "系统通知 · 服务不可用"
    );
    service.stop();
    writer.finish();
}

/// 启动查询迟到不能覆盖后续明确授权的拒绝结果；下一次只读查询恢复后不改变开启意图。
#[gpui_kit::test]
fn notification_late_read_cannot_overwrite_authorization(cx: &mut TestAppContext) {
    use loki_metris_core::usage_milestones::NotificationPermission as Permission;
    let writer = Writer::in_memory(Preferences::default());
    let service = Service::new(false, writer.clone());
    let (sender, receiver) = oneshot::channel();
    cx.update(|cx| service.refresh_permission_using(async { receiver.await.unwrap() }, cx));
    cx.run_until_parked();
    cx.update(|cx| service.set_using(true, |_| async { Err(Error::Denied) }, cx));
    settle(cx, &service);
    sender.send(Permission::NotDetermined).unwrap();
    cx.run_until_parked();
    assert_eq!(service.state().permission, Permission::Denied);
    assert_eq!(service.state().error, Some(Error::Denied));
    assert!(service.state().enabled);
    cx.update(|cx| service.refresh_permission_using(async { Permission::Allowed }, cx));
    cx.run_until_parked();
    assert!(service.state().enabled);
    assert_eq!(service.state().permission, Permission::Allowed);
    assert!(service.state().error.is_none());
    service.stop();
    writer.finish();
}

/// 退出取消权限等待，迟到回调不能开始写入，关闭后也不再接受新的开启。
#[gpui_kit::test]
fn notification_shutdown_rejects_late_authorization(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());
    let (sender, receiver) = oneshot::channel::<Result<(), Error>>();
    cx.update(|cx| service.set_using(true, |_| async { receiver.await.unwrap() }, cx));
    cx.run_until_parked();
    service.stop();
    cx.run_until_parked();
    assert!(sender.send(Ok(())).is_err());
    cx.update(|cx| service.set_using(true, |_| async { panic!("closed owner must reject") }, cx));
    writer.finish();
    assert!(
        !std::fs::read_to_string(file)
            .unwrap()
            .contains("system_notification=true")
    );
    assert!(!service.state().enabled);
}

/// 保存已接受但回执未消费时退出，真实写入仍完成，迟到回执不能改变关闭后的状态。
#[gpui_kit::test]
fn notification_shutdown_preserves_accepted_save(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());

    let pause = writer.pause_worker_for_test();
    cx.update(|cx| service.set_using(true, |_| async { Ok(()) }, cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(20));
    cx.run_until_parked();
    // 授权立即完成，worker 仍被门闩阻塞，因此 busy 证明已进入保存回执等待。
    let pending = service.state();

    service.stop();
    cx.run_until_parked();
    drop(pause);
    writer.finish();
    cx.executor().advance_clock(Duration::from_millis(20));
    cx.run_until_parked();

    // 先释放门闩并完成退出，再断言，避免失败路径留下阻塞线程。
    assert!(pending.busy);
    assert_eq!(pending.desired, Some(true));
    assert!(!pending.enabled);
    assert!(
        std::fs::read_to_string(file)
            .unwrap()
            .contains("system_notification=true\n")
    );
    let stopped = service.state();
    assert!(!stopped.enabled);
    assert!(!stopped.busy);
    assert_eq!(stopped.desired, None);
    assert!(stopped.error.is_none());
    assert!(
        writer
            .mutate(PreferencePatch::SystemNotification(false))
            .is_err()
    );
}

/// 授权总期限到达后不接受迟到成功，但独立的本地开启意图仍由真实保存回执确认。
#[gpui_kit::test]
fn notification_deadline_keeps_intent_without_claiming_permission(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());
    let (sender, receiver) = oneshot::channel::<Result<(), Error>>();
    cx.update(|cx| service.set_using(true, move |_| async { receiver.await.unwrap() }, cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(11));
    // 迟到成功不能伪装已允许；本地偏好不受 OS 超时撤销。
    let _ = sender.send(Ok(()));
    settle(cx, &service);
    assert_eq!(service.state().error, Some(Error::Timeout));
    assert!(service.state().enabled);
    assert_eq!(
        service.state().permission,
        loki_metris_core::usage_milestones::NotificationPermission::Unavailable
    );
    assert!(!service.state().busy);
    assert!(
        std::fs::read_to_string(file)
            .unwrap()
            .contains("system_notification=true\n")
    );
    service.stop();
    writer.finish();
}
