//! 使用真实业务 runtime、Monitor 与私有临时账本复现退出，零 OS 通知或用户数据。
use super::*;
use std::time::Instant;

/// 等到独立摘要已进入真实 store 的阻塞读取；锁仍由测试持有。
fn wait_for_summary_read(service: &UsageService) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while super::super::pet_background_task_count(service) == 0 {
        assert!(Instant::now() < deadline, "summary must start an owned settings read");
        std::thread::sleep(Duration::from_millis(1));
    }
}
/// 退出不能等待设置 mutex、磁盘或 OS 回调，已接受回执仍需实际写盘后 join。
#[test]
fn notification_shutdown_cancels_blocked_summary_and_persists_accepted_receipt() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("usage-notification-state.json");
    let now = current_epoch_ms();
    let mut value = TodayUsageSnapshot {
        day: UsageDay::capture(now, &jiff::tz::TimeZone::system()).unwrap(),
        source: "shutdown-fixture".into(), total_tokens: 90_000_000, observed_ms: now,
        availability: loki_metris_core::TodayAvailability::Available,
        completeness: loki_metris_core::Completeness::Complete,
        confidence: loki_metris_core::Confidence::Exact,
    };
    let mut ledger = Ledger::load(path.clone());
    let mut state = ledger.state.clone();
    state.set_enabled(true);
    state.observe(&value, now);
    value.total_tokens = 1_350_000_000;
    state.observe(&value, now);
    let request = state.reserve(&value, NotificationPermission::Allowed, now).unwrap();
    ledger.commit(state).unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    let blocked = super::super::block_pet_mutations(&service);
    service.configure_notifications(true, NotificationPermission::Unavailable);
    wait_for_summary_read(&service);
    // receipt 被有界通道接受后立即关闭，无需等待 UI 下一轮 poll。
    service.notification_receipt(request.id.clone(), MilestoneOutcome::Accepted);
    let before = Instant::now();
    service.runtime.lock().unwrap().as_ref().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(2), service.notifications.finish()).await
            .expect("shutdown must cancel the blocked read and drain accepted receipts");
    });
    assert!(before.elapsed() < Duration::from_secs(2));
    let restored = Ledger::load(path);
    assert_eq!(restored.state.active().unwrap().submitted, 1);
    assert!(restored.state.active().unwrap().reservation.is_none());
    assert_eq!(restored.state.active().unwrap().pending_count(), 0);
    assert!(service.take_notification().is_none(), "closing must never reserve another bucket");
    assert!(service.notifications.receipts.is_closed());
    blocked.release();
    service.finish();
}
/// 首次控制尚未初始化时也能关闭，重复退出不再 join 或发布新预约。
#[test]
fn notification_shutdown_before_initialization_and_repeated_finish() {
    let root = tempfile::tempdir().unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    service.finish();
    service.finish();
    assert!(service.notifications.receipts.is_closed());
    assert!(service.take_notification().is_none());
}
