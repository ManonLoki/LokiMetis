use super::*;
/// DST 日界来自日历，23/25小时均保持一天，且时区往返身份稳定。
#[test]
fn civil_day_uses_dst_calendar_boundaries() {
    let zone = TimeZone::get("America/New_York").unwrap();
    for (time, hours) in [("2026-03-08T12:00:00Z", 23), ("2026-11-01T12:00:00Z", 25)] {
        let now = time.parse::<jiff::Timestamp>().unwrap().as_millisecond();
        let day = UsageDay::capture(now, &zone).unwrap();
        assert_eq!(day.end_ms - day.start_ms, hours * 3_600_000);
        assert!(day.contains(now));
        assert!(!day.contains(day.end_ms));
        assert_eq!(day.key(), UsageDay::capture(now, &zone).unwrap().key());
    }
}
/// 可信零量可触发资格，部分/推算/过期/旧日/旧来源均不能推进通知。
#[test]
fn eligibility_requires_complete_exact_current_snapshot() {
    let now = 1_790_000_000_000;
    let day = UsageDay::capture(now, &TimeZone::UTC).unwrap();
    let mut snapshot = TodayUsageSnapshot {
        day: day.clone(),
        source: "source".into(),
        total_tokens: 0,
        observed_ms: now,
        availability: TodayAvailability::Available,
        completeness: Completeness::Complete,
        confidence: Confidence::Exact,
    };
    assert!(snapshot.eligible(now, &day, "source"));
    assert!(!snapshot.eligible(now + 90_001, &day, "source"));
    assert!(!snapshot.eligible(now, &day, "other"));
    snapshot.confidence = Confidence::Derived;
    assert!(!snapshot.eligible(now, &day, "source"));
    snapshot.confidence = Confidence::Exact;
    snapshot.completeness = Completeness::Partial;
    assert!(!snapshot.eligible(now, &day, "source"));
    snapshot.completeness = Completeness::Complete;
    snapshot.availability = TodayAvailability::NoRecords;
    assert!(!snapshot.eligible(now, &day, "source"));
}
