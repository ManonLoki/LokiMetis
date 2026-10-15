//! 固定民用日与纯 sender 回执，验证每日唯一事件，不发送系统通知。
use super::*;
use crate::{Completeness, Confidence, TodayAvailability, TodayUsageSnapshot, UsageDay};

/// 固定 UTC 日界，使跨日、重试和回落不依赖真实时钟。
fn snapshot(tokens: u64, day: i64) -> TodayUsageSnapshot {
    let start = 1_800_000_000_000_i64 / 86_400_000 * 86_400_000 + day * 86_400_000;
    TodayUsageSnapshot {
        day: UsageDay { date: format!("day{day}"), zone: "UTC".into(), start_ms: start, end_ms: start + 86_400_000 },
        source: "all-a".into(), total_tokens: tokens, observed_ms: start + 1000,
        availability: TodayAvailability::Available, completeness: Completeness::Complete, confidence: Confidence::Exact,
    }
}
/// 先观察阈值以下的可信总量，再模拟同日首次突破。
fn crossing() -> (MilestoneState, TodayUsageSnapshot) {
    let mut state = MilestoneState::default();
    state.set_enabled(true);
    let initial = snapshot(900_000_000, 0);
    state.observe(&initial, initial.observed_ms);
    let value = snapshot(1_350_000_000, 0);
    state.observe(&value, value.observed_ms);
    (state, value)
}
/// 观测时间随发送更新，所有测试真实经过90秒资格门禁。
fn reserve(state: &mut MilestoneState, value: &mut TodayUsageSnapshot, now: i64) -> MilestoneRequest {
    value.observed_ms = now;
    state.reserve(value, NotificationPermission::Allowed, now).unwrap()
}
/// 严格突破才入队；极大跳跃、重复观测及下降后反弹都不产生第二条。
#[test]
fn daily_first_billion_crossing_is_strict_and_unique() {
    let mut state = MilestoneState::default();
    state.set_enabled(true);
    let mut value = snapshot(TOKEN_MILESTONE, 0);
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    value.total_tokens += 1;
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 1);
    let now = value.observed_ms;
    let request = reserve(&mut state, &mut value, now);
    assert_eq!(request.total_tokens, TOKEN_MILESTONE + 1);
    assert!(request.id.starts_with("m2-"));
    state.complete(&request.id, MilestoneOutcome::Accepted, now);
    for total in [2_000_000_000, u64::MAX, 0, 1_500_000_000] {
        value.total_tokens = total;
        state.observe(&value, now);
        assert_eq!(state.active().unwrap().pending_count(), 0);
    }
    assert_eq!(state.active().unwrap().submitted, 1);
    assert!(state.valid());
}
/// 首次可信观测已突破也只产生当日一个事件，不展开历史百兆档位。
#[test]
fn first_observation_above_threshold_notifies_once() {
    let mut state = MilestoneState::default();
    state.set_enabled(true);
    let value = snapshot(u64::MAX, 0);
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 1);
    assert!(serde_json::to_vec(&state).unwrap().len() < 4096);
    assert!(state.valid());
}
/// 部分、推算和过期快照不能消费阈值；权限恢复后仍是同一事件。
#[test]
fn evidence_and_permission_gate_daily_event() {
    let mut state = MilestoneState::default();
    state.set_enabled(true);
    let mut value = snapshot(1_350_000_000, 0);
    for (completeness, confidence, age) in [
        (Completeness::Partial, Confidence::Exact, 0),
        (Completeness::Complete, Confidence::Derived, 0),
        (Completeness::Complete, Confidence::Exact, 90_001),
    ] {
        value.completeness = completeness;
        value.confidence = confidence;
        state.observe(&value, value.observed_ms + age);
        assert!(state.active().is_none());
    }
    value.completeness = Completeness::Complete;
    value.confidence = Confidence::Exact;
    state.observe(&value, value.observed_ms);
    for permission in [NotificationPermission::Denied, NotificationPermission::Unavailable, NotificationPermission::NotDetermined] {
        assert!(state.reserve(&value, permission, value.observed_ms).is_none());
    }
    assert_eq!(state.active().unwrap().pending_count(), 1);
    assert!(state.reserve(&value, NotificationPermission::Allowed, value.observed_ms).is_some());
}
/// 关闭重开与来源重建抑制已有事件，不因消费继续增加而重新通知。
#[test]
fn reenable_and_source_reset_do_not_duplicate_daily_event() {
    let (mut state, mut value) = crossing();
    state.set_enabled(false);
    assert_eq!(state.active().unwrap().suppressed, 1);
    state.set_enabled(true);
    value.total_tokens = 2_500_000_000;
    state.observe(&value, value.observed_ms);
    value.source = "all-b".into();
    state.observe(&value, value.observed_ms);
    state.source_reset();
    state.observe(&value, value.observed_ms);
    value.total_tokens = u64::MAX;
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    assert_eq!(state.active().unwrap().high, 1);
    assert!(state.valid());
}
/// 新日重新拥有一次事件；旧队列过期，返回旧日不重报且仅保留32日。
#[test]
fn rollover_return_and_retention() {
    let (mut state, _) = crossing();
    let value = snapshot(2_500_000_000, 1);
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 1);
    assert_eq!(state.days[0].expired, 1);
    let old = snapshot(2_500_000_000, 0);
    state.observe(&old, old.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    for day in 2..40 { let value = snapshot(0, day); state.observe(&value, value.observed_ms); }
    assert_eq!(state.days.len(), 32);
    state.observe(&old, old.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    assert!(state.valid());
}
/// 拒绝保持稳定ID和三次重试预算，耗尽后须明确重试。
#[test]
fn rejected_receipts_backoff_and_exhaustion() {
    let (mut state, mut value) = crossing();
    let mut now = value.observed_ms;
    let mut id = None;
    for delay in [30_000, 120_000, 600_000, 600_000] {
        let request = reserve(&mut state, &mut value, now);
        assert_eq!(id.get_or_insert(request.id.clone()), &request.id);
        state.complete(&request.id, MilestoneOutcome::Rejected, now);
        now += delay;
        value.observed_ms = now;
        assert!(state.reserve(&value, NotificationPermission::Allowed, now - 1).is_none());
    }
    assert!(state.reserve(&value, NotificationPermission::Allowed, now).is_none());
    assert_eq!(state.issue, Some(MilestoneIssue::RetryExhausted));
    state.retry(now);
    assert_eq!(reserve(&mut state, &mut value, now).id, id.unwrap());
}
/// 重启已预约请求保持Unknown，核对成功或主动重试才改变结果。
#[test]
fn restart_preserves_unknown_and_daily_deduplication() {
    let (mut state, mut value) = crossing();
    let now = value.observed_ms;
    let request = reserve(&mut state, &mut value, now);
    let mut restored: MilestoneState = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    restored.recover();
    assert_eq!(restored.active().unwrap().unknown_count, 1);
    assert!(restored.reserve(&value, NotificationPermission::Allowed, now).is_none());
    restored.complete(&request.id, MilestoneOutcome::Accepted, now);
    value.total_tokens = u64::MAX;
    restored.observe(&value, now);
    assert_eq!(restored.active().unwrap().pending_count(), 0);
    state.complete(&request.id, MilestoneOutcome::Unknown, now);
    state.retry(now);
    assert_eq!(reserve(&mut state, &mut value, now).id, request.id);
}
/// 晚到失败只重试匹配日事件，重复回调与旧策略ID不能添加新事件。
#[test]
fn late_failure_retries_only_matching_daily_event() {
    let (mut state, mut value) = crossing();
    let now = value.observed_ms;
    let request = reserve(&mut state, &mut value, now);
    state.complete(&request.id, MilestoneOutcome::Accepted, now);
    state.late_failure(&request.id.replacen("m2-", "m1-", 1), now);
    assert!(state.active().unwrap().retries.is_empty());
    state.late_failure(&request.id, now);
    state.late_failure(&request.id, now);
    assert_eq!(state.active().unwrap().retries.len(), 1);
    assert_eq!(reserve(&mut state, &mut value, now + 30_000).id, request.id);
}
/// 旧百兆队列显式取消，已见1B的日界保守建基线而不误发旧请求。
#[test]
fn old_policy_migrates_without_replaying_100m_notifications() {
    let (mut state, _) = crossing();
    state.policy_version = 1;
    state.days[0].high = 35;
    state.days[0].pending = vec![BucketRange { first: 1, last: 35 }];
    assert!(!state.valid());
    assert!(state.upgrade_policy());
    assert_eq!(state.policy_version, 2);
    assert_eq!(state.days[0].high, 1);
    assert_eq!(state.days[0].baseline, 1);
    assert_eq!(state.days[0].pending_count(), 0);
    assert!(state.upgrade_policy());
    let value = snapshot(5_000_000_000, 0);
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
}
/// 旧第10档可能恰好1B，迁移不能吞掉随后严格超过阈值的首次事件。
#[test]
fn old_policy_at_exact_billion_can_cross_after_migration() {
    let (mut state, _) = crossing();
    state.policy_version = 1;
    state.days[0].high = 10;
    state.days[0].pending = vec![BucketRange { first: 1, last: 10 }];
    assert!(state.upgrade_policy());
    let mut value = snapshot(TOKEN_MILESTONE, 0);
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().high, 0);
    value.total_tokens += 1;
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 1);
}
/// 未知策略、超出唯一事件的高水位与重叠队列均拒绝。
#[test]
fn validation_rejects_corrupt_history() {
    let (mut state, _) = crossing();
    state.policy_version = 3;
    assert!(!state.upgrade_policy());
    state.policy_version = 2;
    state.days[0].high = 2;
    assert!(!state.valid());
    state.days[0].high = 1;
    state.days[0].pending.push(BucketRange { first: 1, last: 1 });
    assert!(!state.valid());
}
/// 尚未调用OS的预约可无损归还；关闭后的旧预约不得重新排队。
#[test]
fn unsubmitted_reservation_defers_without_failure_budget() {
    let (mut state, mut value) = crossing();
    let now = value.observed_ms;
    let request = reserve(&mut state, &mut value, now);
    state.defer_unsubmitted(&request.id, now);
    assert_eq!(state.active().unwrap().retries[0].failures, 0);
    assert_eq!(reserve(&mut state, &mut value, now).id, request.id);
    state.set_enabled(false);
    state.defer_unsubmitted(&request.id, now);
    assert!(state.active().unwrap().reservation.is_none());
    assert!(state.active().unwrap().retries.is_empty());
    assert!(state.valid());
}
