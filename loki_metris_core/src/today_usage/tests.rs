//! 本文件的测试覆盖：用真实窗口事实形状验证可展示性与通知资格，不能依赖总量是否大于零；保持生产聚合的质量字段及整数总量，隔离索引状态与来源就绪状态；DST 日界来自日历，23/25小时均保持一天，且时区往返身份稳定。

use super::*;
use crate::{
    Freshness, MetricFact, MetricScope, ProviderKind, WindowUsage,
    empty_local_usage_aggregate_for_provider,
};

/// 用真实窗口事实形状验证可展示性与通知资格，不能依赖总量是否大于零。
#[test]
fn from_records_displays_indexed_partial_usage_without_notification_eligibility() {
    let now = 1_790_000_000_000;
    let day = UsageDay::capture(now, &TimeZone::UTC).unwrap();
    for tokens in [0, 110] {
        for state in [LocalIndexState::Ready, LocalIndexState::ReadyNoCalls] {
            for quality in [
                Completeness::Complete,
                Completeness::Partial,
                Completeness::Unknown,
            ] {
                for confidence in [
                    Confidence::Exact,
                    Confidence::Derived,
                    Confidence::Suspected,
                ] {
                    let records = today_records(state, tokens, quality, confidence, now);
                    for all_ready in [false, true] {
                        let snapshot = TodayUsageSnapshot::from_records(
                            day.clone(),
                            "source".into(),
                            &records,
                            now,
                            all_ready,
                            false,
                        )
                        .unwrap();
                        assert_eq!(snapshot.availability, TodayAvailability::Available);
                        assert_eq!(snapshot.total_tokens, tokens);
                        assert_eq!(
                            snapshot.completeness,
                            if !all_ready && quality == Completeness::Complete {
                                Completeness::Partial
                            } else {
                                quality
                            }
                        );
                        assert_eq!(snapshot.confidence, confidence);
                        assert_eq!(
                            snapshot.eligible(now, &day, "source"),
                            all_ready
                                && quality == Completeness::Complete
                                && confidence == Confidence::Exact
                        );
                    }
                }
            }
        }
        for state in [LocalIndexState::NotScanned, LocalIndexState::NeedsRescan] {
            let records = today_records(
                state,
                tokens,
                Completeness::Complete,
                Confidence::Exact,
                now,
            );
            let snapshot = TodayUsageSnapshot::from_records(
                day.clone(),
                "source".into(),
                &records,
                now,
                true,
                false,
            )
            .unwrap();
            assert_eq!(snapshot.availability, TodayAvailability::NoRecords);
            assert!(!snapshot.eligible(now, &day, "source"));
        }
    }
    let mut records = today_records(
        LocalIndexState::ReadyNoCalls,
        0,
        Completeness::Complete,
        Confidence::Exact,
        now,
    );
    let snapshot =
        TodayUsageSnapshot::from_records(day.clone(), "source".into(), &records, now, true, false)
            .unwrap();
    assert_eq!(snapshot.availability, TodayAvailability::Available);
    assert!(snapshot.eligible(now, &day, "source"));
    for state in [LocalIndexState::Ready, LocalIndexState::NotScanned] {
        records.index_state = state;
        let snapshot = TodayUsageSnapshot::from_records(
            day.clone(),
            "source".into(),
            &records,
            now,
            false,
            true,
        )
        .unwrap();
        assert_eq!(snapshot.availability, TodayAvailability::Unavailable);
    }
    records.windows.clear();
    assert!(
        TodayUsageSnapshot::from_records(day, "source".into(), &records, now, true, false,)
            .is_none()
    );
}

/// 保持生产聚合的质量字段及整数总量，隔离索引状态与来源就绪状态。
fn today_records(
    state: LocalIndexState,
    tokens: u64,
    quality: Completeness,
    confidence: Confidence,
    now: i64,
) -> LocalRecordsSummary {
    let mut aggregate = empty_local_usage_aggregate_for_provider(ProviderKind::CombinedLocalAgents);
    aggregate.tokens.total_tokens = tokens;
    LocalRecordsSummary {
        index_state: state,
        windows: vec![WindowUsage {
            window: LocalUsageWindow::Today,
            fact: MetricFact::new(
                aggregate,
                ProviderKind::CombinedLocalAgents,
                MetricScope::DeviceObserved,
                now,
                Freshness::Fresh,
                quality,
                confidence,
                None,
            ),
        }],
    }
}

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
