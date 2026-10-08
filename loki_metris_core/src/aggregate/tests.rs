//! aggregate 模块级测试。

use super::*;
use crate::{
    Completeness, Confidence, Freshness, MetricFact, MetricScope, ProviderKind, SourceProvenance,
    TokenUsage, UsageCall,
};

/// 构造带稳定来源标识的聚合测试调用。
fn call(id: &str, source_id: &str, input: u64) -> UsageCall {
    UsageCall {
        logical_call_id: id.to_owned(),
        occurred_at_epoch_ms: 1,
        model: Some("synthetic-model".to_owned()),
        reasoning_effort: None,
        project_key: Some("project-a".to_owned()),
        thread_key: "thread-a".to_owned(),
        project_label: None,
        thread_label: None,
        usage: TokenUsage::new(input, input / 2, None, 10, 2, Some(input + 10))
            .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![SourceProvenance {
            source_id: source_id.to_owned(),
            root_id: "root-a".to_owned(),
            relative_label: "sessions/example.jsonl".to_owned(),
            archived: false,
        }],
    }
}

#[test]
/// 验证同一范围内重复观察优先采用更新事实。
fn prefers_the_newer_observation_within_the_same_scope() {
    let older = MetricFact::new(
        10_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        10,
        Freshness::Stale,
        Completeness::Partial,
        Confidence::Derived,
        Some("parser-v1".to_owned()),
    );
    let newer = MetricFact::new(
        12_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        20,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        Some("parser-v1".to_owned()),
    );

    let selected = prefer_metric_within_scope(older, newer).expect("scope matches");
    assert_eq!(selected.observed_at_epoch_ms, 20);
    assert_eq!(selected.value, 12);
}

#[test]
/// 验证不同聚合范围的事实不会被错误融合。
fn rejects_cross_scope_fusion() {
    let root = MetricFact::new(
        10_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::RootObserved,
        1,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        None,
    );
    let device = MetricFact::new(
        10_u64,
        ProviderKind::RolloutJsonl,
        MetricScope::DeviceObserved,
        1,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        None,
    );

    assert_eq!(
        prefer_metric_within_scope(root, device),
        Err(MetricFusionError::ScopeMismatch)
    );
}

#[test]
/// 验证调用去重后仍保留全部物理来源证据。
fn deduplicates_calls_and_keeps_all_provenance() {
    let first = call("call-a", "active", 100);
    let mut archived = call("call-a", "archived", 100);
    archived.provenance[0].archived = true;

    let canonical = canonicalize_usage_calls(vec![first, archived]);
    let aggregate = aggregate_canonical_usage(&canonical).expect("aggregate is valid");

    assert_eq!(canonical.calls.len(), 1);
    assert_eq!(canonical.calls[0].provenance.len(), 2);
    assert_eq!(canonical.duplicate_counts_by_call["call-a"], 1);
    assert_eq!(aggregate.call_count, 1);
    assert_eq!(aggregate.root_count, 1);
    assert_eq!(aggregate.duplicate_source_count, 1);
    assert_eq!(aggregate.cross_root_duplicate_source_count, 0);
    assert_eq!(aggregate.tokens.total_tokens, 110);
}

/// 高缓存调用的聚合总量仍必须覆盖全部输入与输出，缓存不得被扣除。
#[test]
fn high_cache_aggregate_keeps_total_above_input_plus_output() {
    let first = call("call-a", "source-a", 100);
    let second = call("call-b", "source-b", 200);
    let canonical = canonicalize_usage_calls(vec![first, second]);
    let aggregate = aggregate_canonical_usage(&canonical).expect("aggregate is valid");

    assert_eq!(aggregate.tokens.input_tokens, 300);
    assert_eq!(aggregate.tokens.cached_input_tokens, Some(150));
    assert_eq!(aggregate.tokens.output_tokens, 20);
    assert_eq!(aggregate.tokens.total_tokens, 320);
    assert!(
        aggregate.tokens.total_tokens
            >= aggregate.tokens.input_tokens + aggregate.tokens.output_tokens
    );
}

#[test]
/// 验证不同数据根数量与跨根重复观察分别计数。
fn counts_distinct_roots_and_cross_root_duplicate_observations() {
    let first = call("call-a", "source-a", 100);
    let mut copied = call("call-a", "source-b", 100);
    copied.provenance[0].root_id = "root-b".to_owned();

    let canonical = canonicalize_usage_calls(vec![first, copied]);
    let aggregate = aggregate_canonical_usage(&canonical).expect("aggregate is valid");

    assert_eq!(aggregate.call_count, 1);
    assert_eq!(aggregate.root_count, 2);
    assert_eq!(aggregate.source_count, 2);
    assert_eq!(aggregate.duplicate_source_count, 1);
    assert_eq!(aggregate.cross_root_duplicate_source_count, 1);
}

#[test]
/// 验证跨根重复量以最完整的单根集合为保守基线。
fn cross_root_duplicate_count_uses_most_complete_single_root_baseline() {
    let first = call("call-a", "source-a", 100);
    let mut copied_once = call("call-a", "source-b-1", 100);
    copied_once.provenance[0].root_id = "root-b".to_owned();
    let mut copied_twice = call("call-a", "source-b-2", 100);
    copied_twice.provenance[0].root_id = "root-b".to_owned();

    let canonical = canonicalize_usage_calls(vec![first, copied_once, copied_twice]);
    let aggregate = aggregate_canonical_usage(&canonical).expect("aggregate is valid");

    assert_eq!(aggregate.root_count, 2);
    assert_eq!(aggregate.duplicate_source_count, 2);
    assert_eq!(aggregate.cross_root_duplicate_source_count, 1);
}

#[test]
/// 验证筛选后的规范集合保留所选调用对应的数据质量事实。
fn filtered_canonical_usage_preserves_selected_quality_facts() {
    let kept = call("kept", "active", 100);
    let mut kept_duplicate = call("kept", "archived", 120);
    kept_duplicate.confidence = Confidence::Derived;
    let omitted = call("omitted", "other", 50);
    let canonical = canonicalize_usage_calls(vec![kept, kept_duplicate, omitted]);

    let filtered = filter_canonical_usage(&canonical, |call| call.logical_call_id == "kept");
    let aggregate = aggregate_canonical_usage(&filtered).expect("subset is valid");

    assert_eq!(filtered.calls.len(), 1);
    assert_eq!(filtered.duplicate_source_count, 1);
    assert_eq!(filtered.duplicate_counts_by_call["kept"], 1);
    assert_eq!(filtered.warnings.len(), 1);
    assert_eq!(aggregate.duplicate_source_count, 1);
}

#[test]
/// 验证冲突重复项采用最高置信度事实并保留冲突信号。
fn conflicting_duplicates_keep_the_highest_confidence_selected_fact() {
    let exact = call("same", "exact", 100);
    let mut first_derived = call("same", "derived-a", 200);
    first_derived.confidence = Confidence::Derived;
    let mut second_derived = call("same", "derived-b", 300);
    second_derived.confidence = Confidence::Derived;

    let canonical = canonicalize_usage_calls(vec![exact, first_derived, second_derived]);

    assert_eq!(canonical.calls.len(), 1);
    assert_eq!(canonical.calls[0].usage.input_tokens, 100);
    assert_eq!(canonical.calls[0].confidence, Confidence::Suspected);
    assert_eq!(canonical.calls[0].provenance.len(), 3);
    assert_eq!(canonical.duplicate_counts_by_call["same"], 2);
    assert_eq!(canonical.warnings.len(), 2);
}

#[test]
/// 验证非空子集按自身调用重新计算 Token 分项可用性。
fn non_empty_filtered_subset_uses_its_own_component_availability() {
    let mut provided = call("provided", "source-a", 100);
    provided.usage =
        TokenUsage::new(100, 25, Some(5), 10, 2, None).expect("provided fixture is valid");
    let mut missing = call("missing", "source-b", 50);
    missing.usage = TokenUsage::new_with_availability(50, None, None, 5, None, None)
        .expect("missing fixture is valid");
    let canonical = canonicalize_usage_calls(vec![provided, missing]);
    let filtered = filter_canonical_usage(&canonical, |call| call.logical_call_id == "provided");
    let aggregate = aggregate_canonical_usage(&filtered).expect("subset aggregate is valid");

    assert_eq!(aggregate.tokens.cached_input_tokens, Some(25));
    assert_eq!(aggregate.tokens.cache_write_input_tokens, Some(5));
    assert_eq!(aggregate.tokens.reasoning_output_tokens, Some(2));
    assert_eq!(aggregate.cached_read_call_count, Some(1));
}
