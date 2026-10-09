//! 可控民用日、真实整数边界与纯 sender 回执，不发送系统通知。
use super::*;
use crate::{Completeness, Confidence, TodayAvailability, TodayUsageSnapshot, UsageDay};

/// 固定 UTC 日界让重试、跨日和回滚完全可重现。
fn snapshot(tokens: u64, day: i64) -> TodayUsageSnapshot {
    let start = 1_800_000_000_000_i64 / 86_400_000 * 86_400_000 + day * 86_400_000;
    TodayUsageSnapshot {
        day: UsageDay {
            date: format!("day{day}"),
            zone: "UTC".into(),
            start_ms: start,
            end_ms: start + 86_400_000,
        },
        source: "all-a".into(),
        total_tokens: tokens,
        observed_ms: start + 1000,
        availability: TodayAvailability::Available,
        completeness: Completeness::Complete,
        confidence: Confidence::Exact,
    }
}
/// 建基线后模拟同日消费，不在 fixture 中伪造已提交状态。
fn crossing(tokens: u64) -> (MilestoneState, TodayUsageSnapshot) {
    let mut state = MilestoneState::default();
    state.set_enabled(true);
    let initial = snapshot(90_000_000, 0);
    state.observe(&initial, initial.observed_ms);
    let value = snapshot(tokens, 0);
    state.observe(&value, value.observed_ms);
    (state, value)
}
/// 刷新观测时间与发送时间一致，避免测试不小心绕过90秒门禁。
fn reserve(
    state: &mut MilestoneState,
    value: &mut TodayUsageSnapshot,
    now: i64,
) -> MilestoneRequest {
    value.observed_ms = now;
    state
        .reserve(value, NotificationPermission::Allowed, now)
        .unwrap()
}
/// 90M→350M 三档分别提交且节流，下降或重复观测不重新排队。
#[test]
fn three_crossings_have_stable_distinct_ids_and_high_water() {
    let (mut state, mut value) = crossing(350_000_000);
    let start = value.observed_ms;
    assert_eq!(state.active().unwrap().pending_count(), 3);
    let mut ids = vec![];
    for bucket in 1..=3 {
        let now = start + (bucket - 1) as i64 * 10_000;
        let request = reserve(&mut state, &mut value, now);
        assert_eq!(request.bucket, bucket);
        assert_eq!(request.total_tokens, 350_000_000);
        assert!(
            state
                .reserve(&value, NotificationPermission::Allowed, now)
                .is_none()
        );
        state.complete(&request.id, MilestoneOutcome::Accepted, now);
        ids.push(request.id);
        assert!(
            state
                .reserve(&value, NotificationPermission::Allowed, now + 9999)
                .is_none()
        );
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3);
    value.total_tokens = 20;
    state.observe(&value, value.observed_ms);
    value.total_tokens = 350_000_000;
    state.observe(&value, value.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    assert_eq!(state.active().unwrap().submitted, 3);
    assert!(state.valid());
}
/// 最大整数跳跃压缩为一个范围，不按档位分配内存。
#[test]
fn huge_jump_stays_bounded_and_missing_history_is_baseline() {
    let (state, _) = crossing(u64::MAX);
    assert_eq!(state.active().unwrap().pending.len(), 1);
    assert!(serde_json::to_vec(&state).unwrap().len() < 4096);
    assert!(state.valid());
    let mut initial = MilestoneState::default();
    initial.set_enabled(true);
    let v = snapshot(350_000_000, 0);
    initial.observe(&v, v.observed_ms);
    assert_eq!(initial.active().unwrap().baseline, 3);
    assert_eq!(initial.active().unwrap().submitted, 0);
    assert_eq!(initial.active().unwrap().pending_count(), 0);
}
/// 权限不足、部分/派生/过期快照都暂停；恢复完整事实时补齐跨档。
#[test]
fn permission_and_evidence_pause_without_consuming_crossings() {
    let (mut state, mut v) = crossing(90_000_000);
    v.total_tokens = 350_000_000;
    v.completeness = Completeness::Partial;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().high, 0);
    v.completeness = Completeness::Complete;
    v.confidence = Confidence::Derived;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().high, 0);
    v.confidence = Confidence::Exact;
    state.observe(&v, v.observed_ms + 90_001);
    assert_eq!(state.active().unwrap().high, 0);
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 3);
    for p in [
        NotificationPermission::Denied,
        NotificationPermission::Unavailable,
        NotificationPermission::NotDetermined,
    ] {
        assert!(state.reserve(&v, p, v.observed_ms).is_none());
        assert!(state.enabled);
    }
    assert!(
        state
            .reserve(&v, NotificationPermission::Allowed, v.observed_ms)
            .is_some()
    );
}
/// 关闭、来源改变和原地重建都抑制历史，重新开启不追补关闭期间消费。
#[test]
fn disable_reenable_and_source_change_rebaseline() {
    let (mut state, mut v) = crossing(350_000_000);
    state.set_enabled(false);
    assert_eq!(state.active().unwrap().suppressed, 3);
    state.set_enabled(true);
    v.total_tokens = 650_000_000;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    v.total_tokens = 750_000_000;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 1);
    v.source = "all-b".into();
    v.total_tokens = 950_000_000;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    state.source_reset();
    v.total_tokens = 1_250_000_000;
    state.observe(&v, v.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    assert!(state.valid());
}
/// 普通新日从零跨档，首次新时区建基线，返回旧日复用高水位；仅保留32日。
#[test]
fn day_zone_return_and_retention() {
    let (mut state, _) = crossing(350_000_000);
    let next = snapshot(250_000_000, 1);
    state.observe(&next, next.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 2);
    assert_eq!(state.days[0].expired, 3);
    let mut other = next.clone();
    other.day.zone = "other".into();
    other.day.start_ms += 100;
    other.total_tokens = 650_000_000;
    state.observe(&other, other.observed_ms);
    assert_eq!(state.active().unwrap().baseline, 6);
    state.observe(&next, next.observed_ms);
    assert_eq!(state.active().unwrap().high, 2);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    for day in 2..40 {
        let v = snapshot(0, day);
        state.observe(&v, v.observed_ms);
    }
    assert_eq!(state.days.len(), 32);
    let old = snapshot(350_000_000, 0);
    state.observe(&old, old.observed_ms);
    assert_eq!(state.active().unwrap().pending_count(), 0);
    assert!(state.valid());
}
/// 三次自动重试保持 ID，第四次失败暂停；手动重试才能重启。
#[test]
fn rejected_receipts_backoff_and_exhaustion() {
    let (mut state, mut v) = crossing(100_000_000);
    let mut now = v.observed_ms;
    let mut id = None;
    for delay in [30_000, 120_000, 600_000, 600_000] {
        let request = reserve(&mut state, &mut v, now);
        assert_eq!(id.get_or_insert(request.id.clone()), &request.id);
        state.complete(&request.id, MilestoneOutcome::Rejected, now);
        now += delay;
        v.observed_ms = now;
        assert!(
            state
                .reserve(&v, NotificationPermission::Allowed, now - 1)
                .is_none()
        );
    }
    assert!(
        state
            .reserve(&v, NotificationPermission::Allowed, now)
            .is_none()
    );
    assert_eq!(state.issue, Some(MilestoneIssue::RetryExhausted));
    state.retry(now);
    assert_eq!(reserve(&mut state, &mut v, now).id, id.unwrap());
}
/// 预约重启或超时成为Unknown；找到系统相同 ID 才接受，否则显式重试。
#[test]
fn crash_unknown_reconciliation_and_explicit_retry() {
    let (mut state, mut v) = crossing(250_000_000);
    let now = v.observed_ms;
    let req = reserve(&mut state, &mut v, now);
    let bytes = serde_json::to_vec(&state).unwrap();
    let mut restored: MilestoneState = serde_json::from_slice(&bytes).unwrap();
    restored.recover();
    assert_eq!(restored.active().unwrap().unknown_count, 1);
    assert!(
        restored
            .reserve(&v, NotificationPermission::Allowed, now)
            .is_none()
    );
    restored.complete(&req.id, MilestoneOutcome::Accepted, now);
    assert_eq!(restored.active().unwrap().submitted, 1);
    state.complete(&req.id, MilestoneOutcome::Unknown, now);
    state.retry(now);
    assert_eq!(reserve(&mut state, &mut v, now).id, req.id);
}
/// 已提交第1档晚到失败只补第1档，不回退已提交第2档或重复添加重试。
#[test]
fn sparse_late_failure_retries_only_that_bucket() {
    let (mut state, mut v) = crossing(250_000_000);
    let now = v.observed_ms;
    let first = reserve(&mut state, &mut v, now);
    state.complete(&first.id, MilestoneOutcome::Accepted, now);
    let second = reserve(&mut state, &mut v, now + 10_000);
    state.complete(&second.id, MilestoneOutcome::Accepted, now + 10_000);
    state.late_failure(&first.id, now + 20_000);
    state.late_failure(&first.id, now + 20_000);
    assert_eq!(state.active().unwrap().retries.len(), 1);
    assert_eq!(state.active().unwrap().high, 2);
    assert_eq!(reserve(&mut state, &mut v, now + 50_000).id, first.id);
}
/// 非法 schema、重叠队列和重复日期都不能作为可信去重历史。
#[test]
fn state_validation_rejects_corrupt_history() {
    let (mut state, _) = crossing(350_000_000);
    state.schema_version = 2;
    assert!(!state.valid());
    state.schema_version = 1;
    state.days[0]
        .pending
        .push(BucketRange { first: 2, last: 3 });
    assert!(!state.valid());
}

/// 来源变更后的晚到失败不得重启已经抑制的旧来源事件。
#[test]
fn source_rebase_suppresses_late_callbacks_and_unsubmitted_unknown() {
    let (mut state, mut v) = crossing(250_000_000);
    let now = v.observed_ms;
    let first = reserve(&mut state, &mut v, now);
    state.complete(&first.id, MilestoneOutcome::Accepted, now);
    let second = reserve(&mut state, &mut v, now + 10_000);
    state.source_reset();
    v.total_tokens = 350_000_000;
    state.observe(&v, v.observed_ms);
    state.late_failure(&first.id, now + 20_000);
    state.complete(&second.id, MilestoneOutcome::Rejected, now + 20_000);
    assert!(state.active().unwrap().retries.is_empty());
    assert!(state.active().unwrap().reservation.is_none());
    assert_eq!(state.active().unwrap().high, 3);
    v.total_tokens = 450_000_000;
    state.observe(&v, v.observed_ms);
    assert_eq!(reserve(&mut state, &mut v, now + 20_000).bucket, 4);
}

/// 宿主确认尚未调用OS时可归还同档，不消费失败预算；已关闭意图不重新排队。
#[test]
fn unsubmitted_reservation_defers_without_failure_budget() {
    let (mut state, mut value) = crossing(250_000_000);
    let now = value.observed_ms;
    let request = reserve(&mut state, &mut value, now);
    state.defer_unsubmitted(&request.id, now);
    assert_eq!(state.active().unwrap().retries[0].failures, 0);
    assert_eq!(reserve(&mut state, &mut value, now).id, request.id);
    state.set_enabled(false);
    assert_eq!(state.active().unwrap().suppressed, 1);
    assert_eq!(state.active().unwrap().unknown_count, 1);
    state.defer_unsubmitted(&request.id, now);
    assert!(state.active().unwrap().reservation.is_none());
    assert!(state.active().unwrap().retries.is_empty());
    assert!(state.valid());
}
