//! 展示键过滤不漏掉偏好/权限变化，未变化的展示也保留最新业务时间。

use super::*;
use crate::preferences::Preferences;
use loki_metris_core::{
    Completeness, Confidence, TodayAvailability, TodayUsageSnapshot, UsageDay,
    usage_milestones::NotificationPermission,
};

/// 每个展示字段独立变化都必须交付一次新快照，业务 revision 不代替宿主状态。
#[test]
fn refresh_key_observes_each_native_field_independently() {
    for field in 0..7 {
        let writer = Writer::in_memory(Preferences::default());
        let service = Service::new(false, writer.clone());
        let (key, _) = service.state_if_changed(None).unwrap();
        assert!(service.state_if_changed(Some(key)).is_none());
        {
            let mut owner = service.owner.borrow_mut();
            match field {
                0 => owner.state.enabled = true,
                1 => owner.state.busy = true,
                2 => owner.state.desired = Some(true),
                3 => owner.state.error = Some(Error::Save),
                4 => owner.state.permission = NotificationPermission::Denied,
                5 => owner.state.host_available = true,
                6 => owner.state.progress.revision += 1,
                _ => unreachable!(),
            }
        }
        let (next, state) = service.state_if_changed(Some(key))
            .unwrap_or_else(|| panic!("native field {field} did not refresh"));
        assert_ne!(key, next, "missing native field {field}");
        assert_eq!(state, service.state());
        assert!(service.state_if_changed(Some(next)).is_none());
        service.stop();
        writer.finish();
    }
}

/// 时间单独推进不重绘；下次宿主变化取到的仍是最新时间，派发资格不错误过期。
#[test]
fn unchanged_display_keeps_latest_age_for_state_and_next_refresh() {
    let writer = Writer::in_memory(Preferences::default());
    let service = Service::new(false, writer.clone());
    let now = "2026-10-10T12:00:00Z"
        .parse::<jiff::Timestamp>().unwrap().as_millisecond();
    let value = TodayUsageSnapshot {
        day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
        source: "all".into(),
        total_tokens: 350_000_000,
        observed_ms: now,
        availability: TodayAvailability::Available,
        completeness: Completeness::Complete,
        confidence: Confidence::Exact,
    };
    service.owner.borrow_mut().state.progress.snapshot = Some(value.clone());
    let (key, old) = service.state_if_changed(None).unwrap();
    let fresh_ms = now + 120_000;
    service.owner.borrow_mut().state.progress.snapshot.as_mut().unwrap().observed_ms = fresh_ms;
    assert!(service.state_if_changed(Some(key)).is_none());
    let current = service.state();
    assert_eq!(current.progress.snapshot.as_ref().unwrap().observed_ms, fresh_ms);
    assert!(!old.progress.snapshot.as_ref().unwrap().eligible(fresh_ms, &value.day, "all"));
    assert!(current.progress.snapshot.as_ref().unwrap().eligible(fresh_ms, &value.day, "all"));
    service.owner.borrow_mut().state.permission = NotificationPermission::Allowed;
    let (_, next) = service.state_if_changed(Some(key)).unwrap();
    assert_eq!(next.progress.snapshot.as_ref().unwrap().observed_ms, fresh_ms);
    service.stop();
    writer.finish();
}
