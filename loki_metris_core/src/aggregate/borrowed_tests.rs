//! 借用聚合对照旧 owned 筛选，核对完整事实及错误，不依赖宿主或当前时刻。

use super::*;
use crate::*;
use crate::aggregate::copy_matching_snapshots;
use jiff::tz::TimeZone;

/// 构造带多来源与展示字符串的可追溯调用。
fn call(id: &str, when: i64, input: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(), occurred_at_epoch_ms: when,
        model: Some("model".into()), reasoning_effort: Some("high".into()),
        project_key: Some("project".into()), thread_key: format!("thread-{id}"),
        project_label: Some("project label".into()), thread_label: Some("thread label".into()),
        usage: TokenUsage::new(input, input / 2, Some(0), 0, 0, Some(input)).unwrap(),
        adapter_consistency_key: Some("adapter".into()), confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: format!("source-{id}"), root_id: "root-a".into(),
            relative_label: "session/example.jsonl".into(), archived: false,
        }],
    }
}

/// 对任意输入比较借用筛选与原 owned 子集聚合的完整结果。
fn assert_subset_equal(canonical: &CanonicalUsageSet, selected: &[&str], empty: &TokenUsage) {
    let mut owned = filter_canonical_usage(canonical, |call| selected.contains(&call.logical_call_id.as_str()));
    owned.empty_tokens = empty.clone();
    owned.total_token_accounting = TotalTokenAccounting::Observed;
    assert_eq!(
        aggregate_filtered_canonical_usage(canonical,
            |call| selected.contains(&call.logical_call_id.as_str()), empty, TotalTokenAccounting::Observed),
        aggregate_canonical_usage(&owned),
    );
}

/// 未知分项、冲突、跨根来源和空筛选都维持原聚合的全字段结果。
#[test]
fn borrowed_subset_matches_owned_quality_counts_and_empty_shapes() {
    let first = call("a", 1, 100);
    let mut duplicate = first.clone();
    duplicate.provenance[0].root_id = "root-b".into();
    duplicate.provenance[0].source_id = "copied-a".into();
    duplicate.usage = TokenUsage::new(120, 60, Some(0), 0, 0, Some(120)).unwrap();
    let mut second = call("b", 2, 200);
    second.usage.cached_input_tokens = None;
    second.usage.reasoning_output_tokens = None;
    second.confidence = Confidence::Derived;
    let canonical = canonicalize_usage_calls(vec![first, duplicate, second]);
    for provider in [ProviderKind::RolloutJsonl, ProviderKind::ClaudeTranscriptJsonl,
        ProviderKind::GrokSessionJsonl, ProviderKind::CombinedLocalAgents] {
        let empty = empty_token_usage_for_provider(provider);
        for selected in [&[][..], &["a"][..], &["b"][..], &["a", "b"][..]] {
            assert_subset_equal(&canonical, selected, &empty);
        }
    }
}

/// 重复 ID 只按唯一 ID 取重复 map；孤立项排除，重复计数继续饱和求和。
#[test]
fn borrowed_subset_preserves_unique_id_duplicate_saturation() {
    let mut canonical = canonicalize_usage_calls(vec![call("a", 1, 1), call("b", 2, 1)]);
    canonical.calls.push(canonical.calls[0].clone());
    canonical.duplicate_counts_by_call.insert("a".into(), u64::MAX);
    canonical.duplicate_counts_by_call.insert("b".into(), 5);
    canonical.duplicate_counts_by_call.insert("omitted".into(), u64::MAX);
    canonical.duplicate_source_count = 999;
    for selected in [&[][..], &["a"][..], &["b"][..], &["a", "b"][..]] {
        assert_subset_equal(&canonical, selected, &TokenUsage::zero());
    }
    let aggregate = aggregate_filtered_canonical_usage(&canonical, |_| true,
        &TokenUsage::zero(), TotalTokenAccounting::Observed).unwrap();
    assert_eq!(aggregate.duplicate_source_count, u64::MAX);
    assert_eq!(aggregate.call_count, 3);
}

/// 可选缓存写入的先溢出后未知与先未知后累加不同，借用路径不得重排。
#[test]
fn borrowed_subset_keeps_order_sensitive_optional_overflow() {
    let mut first = call("a", 1, 1); first.usage.cache_write_input_tokens = Some(u64::MAX);
    let mut second = call("b", 1, 1); second.usage.cache_write_input_tokens = Some(1);
    let mut third = call("c", 1, 1); third.usage.cache_write_input_tokens = None;
    let mut canonical = canonicalize_usage_calls(vec![first, second, third]);
    assert_subset_equal(&canonical, &["a", "b", "c"], &TokenUsage::zero());
    assert_eq!(aggregate_filtered_canonical_usage(&canonical, |_| true,
        &TokenUsage::zero(), TotalTokenAccounting::Observed), Err(TokenUsageError::Overflow));
    canonical.calls.rotate_right(1);
    assert_subset_equal(&canonical, &["a", "b", "c"], &TokenUsage::zero());
    assert!(aggregate_filtered_canonical_usage(&canonical, |_| true,
        &TokenUsage::zero(), TotalTokenAccounting::Observed).is_ok());
}

/// 把固定 RFC3339 夹具转换为毫秒，测试不读取当前系统时区。
fn epoch(value: &str) -> i64 {
    value.parse::<jiff::Timestamp>().unwrap().as_millisecond()
}

/// 冻结旧六窗口的 owned 筛选、快照拷贝及质量装配，作为语义对照。
fn owned_windows(
    canonical: &CanonicalUsageSet, coverage: &CoverageReport, index_state: LocalIndexState,
    now: i64, provider: ProviderKind, standard: TimeStandard, zone: &TimeZone,
) -> Result<LocalRecordsSummary, String> {
    let error = || "本机窗口无法聚合。".to_owned();
    let today = civil_date_for_timestamp(now, &standard, zone).ok_or_else(error)?;
    let empty = if provider == ProviderKind::CombinedLocalAgents {
        canonical.empty_tokens.clone()
    } else { empty_token_usage_for_provider(provider) };
    let windows = LocalUsageWindow::OVERVIEW_WINDOWS.into_iter().map(|window| {
        let (from, to) = inclusive_calendar_range(window, today).map_err(|_| error())?;
        let contains = |timestamp| civil_date_for_timestamp(timestamp, &standard, zone)
            .is_some_and(|date| date >= from && date <= to);
        let filtered = filter_canonical_usage(canonical, |call| contains(call.occurred_at_epoch_ms));
        let mut subset = copy_matching_snapshots(canonical, filtered,
            |snapshot| contains(snapshot.occurred_at_epoch_ms));
        subset.empty_tokens = empty.clone();
        subset.total_token_accounting = provider.local_total_token_accounting();
        let aggregate = aggregate_canonical_usage(&subset).map_err(|_| error())?;
        let (freshness, completeness, confidence) = local_fact_quality(index_state,
            coverage.state, aggregate.confidence);
        Ok(WindowUsage { window, fact: MetricFact::new(aggregate, provider,
            MetricScope::DeviceObserved, now, freshness, completeness, confidence,
            Some("fixture-parser".into())) })
    }).collect::<Result<Vec<_>, String>>()?;
    Ok(LocalRecordsSummary { index_state, windows })
}

/// 六窗口完整 Result 必须与旧路径一致，包括时区/DST、未来今日、无效时间和质量状态。
#[test]
fn borrowed_six_windows_match_owned_across_calendar_and_quality_states() {
    let mut canonical = canonicalize_usage_calls(vec![
        call("previous-month", epoch("2026-10-31T23:59:59Z"), 1),
        call("month-start", epoch("2026-11-01T00:00:00Z"), 2),
        call("dst-before", epoch("2026-11-01T05:30:00Z"), 3),
        call("dst-after", epoch("2026-11-01T06:30:00Z"), 4),
        call("future-hour", epoch("2026-11-01T23:59:59Z"), 5),
        call("next-day", epoch("2026-11-02T00:00:00Z"), 6),
        call("invalid-time", i64::MAX, 7),
    ]);
    canonical.empty_tokens.cached_input_tokens = None;
    canonical.duplicate_counts_by_call.insert("dst-before".into(), 3);
    canonical.snapshots.push(SessionTokenSnapshot {
        thread_key: "history-only".into(), occurred_at_epoch_ms: epoch("2026-11-01T06:30:00Z"),
        cumulative_total_tokens: u64::MAX, logical_call_id: "snapshot".into(),
        model: Some("diagnostic".into()), reasoning_effort: None, project_key: None,
        provenance: vec![],
    });
    for zone_name in ["UTC", "Asia/Shanghai", "America/New_York"] {
        let zone = TimeZone::get(zone_name).unwrap();
        for standard in [TimeStandard::Local, TimeStandard::utc()] {
            for now in [epoch("2026-11-01T06:00:00Z"), epoch("2026-11-02T00:00:00Z"), i64::MAX] {
                for index_state in [LocalIndexState::NotScanned, LocalIndexState::NeedsRescan,
                    LocalIndexState::ReadyNoCalls, LocalIndexState::Ready] {
                    for state in [CoverageState::Complete, CoverageState::Partial,
                        CoverageState::Cancelled, CoverageState::Failed] {
                        let coverage = CoverageReport { state, roots_scanned: 1, roots_discovered: 1,
                            permission_denied_count: 0, skipped_count: 0, warning_count: 0 };
                        for provider in [ProviderKind::RolloutJsonl, ProviderKind::ClaudeTranscriptJsonl,
                            ProviderKind::GrokSessionJsonl, ProviderKind::CombinedLocalAgents] {
                            assert_eq!(build_local_windows_with_standard(&canonical, &coverage, index_state,
                                now, provider, Some("fixture-parser"), standard.clone(), &zone),
                                owned_windows(&canonical, &coverage, index_state, now, provider,
                                    standard.clone(), &zone), "{zone_name} {standard:?} {index_state:?} {state:?}");
                        }
                    }
                }
            }
        }
    }
}

/// 即使今日正常，只有上月发生的总量溢出仍让整个六窗口构建失败。
#[test]
fn borrowed_six_windows_keep_failure_from_a_non_today_window() {
    let canonical = canonicalize_usage_calls(vec![
        call("old-a", epoch("2026-10-15T00:00:00Z"), u64::MAX),
        call("old-b", epoch("2026-10-16T00:00:00Z"), 1),
        call("today", epoch("2026-11-01T00:00:00Z"), 1),
    ]);
    let coverage = CoverageReport { state: CoverageState::Complete, roots_scanned: 1,
        roots_discovered: 1, permission_denied_count: 0, skipped_count: 0, warning_count: 0 };
    let now = epoch("2026-11-01T12:00:00Z");
    let result = build_local_windows_with_standard(&canonical, &coverage, LocalIndexState::Ready,
        now, ProviderKind::CombinedLocalAgents, Some("fixture-parser"), TimeStandard::Local, &TimeZone::UTC);
    assert_eq!(result, owned_windows(&canonical, &coverage, LocalIndexState::Ready, now,
        ProviderKind::CombinedLocalAgents, TimeStandard::Local, &TimeZone::UTC));
    assert!(result.is_err());
}
