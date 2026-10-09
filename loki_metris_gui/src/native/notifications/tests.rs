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
    assert!(!service.state().enabled);
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
    cx.update(|cx| {
        service.set_using(
            true,
            |_| async { panic!("busy operation must not authorize") },
            cx,
        )
    });
    sender.send(Ok(())).unwrap();
    settle(cx, &service);
    assert!(service.state().enabled);
    cx.update(|cx| {
        service.set_using(
            false,
            |_| async { panic!("disable must not authorize") },
            cx,
        )
    });
    settle(cx, &service);
    assert!(!service.state().enabled);
    service.stop();
    writer.finish();
}

/// 拒绝、超时和保存失败保持旧值；之后重新申请且成功回执才可以开启。
#[gpui_kit::test]
fn notification_errors_keep_old_value_and_retry_can_recover(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());
    for error in [Error::Denied, Error::Unavailable, Error::Timeout] {
        cx.update(|cx| service.set_using(true, move |_| async move { Err(error) }, cx));
        settle(cx, &service);
        assert_eq!(service.state().error, Some(error));
        assert!(!service.state().enabled);
        assert!(!file.exists());
    }
    std::fs::create_dir(&file).unwrap();
    cx.update(|cx| service.set_using(true, |_| async { Ok(()) }, cx));
    settle(cx, &service);
    assert_eq!(service.state().error, Some(Error::Save));
    assert!(!service.state().enabled);
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

/// 入口总期限覆盖授权和保存准入，迟到成功不能产生持久写入。
#[gpui_kit::test]
fn notification_deadline_rejects_late_authorization_before_save(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let service = Service::new(false, writer.clone());
    let (sender, receiver) = oneshot::channel::<Result<(), Error>>();
    cx.update(|cx| service.set_using(true, move |_| async { receiver.await.unwrap() }, cx));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(11));
    // 迟到授权回调不能把已经到期的请求推进到持久写入。
    let _ = sender.send(Ok(()));
    cx.run_until_parked();
    assert_eq!(service.state().error, Some(Error::Timeout));
    assert!(!service.state().enabled);
    assert!(!service.state().busy);
    assert!(!file.exists());
    service.stop();
    writer.finish();
}
