//! 排序折叠版 canonical 去重与重构前逐条插入版必须逐字段一致，含冲突、置信度升级与警告顺序。

use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// 重构前的逐条插入 BTreeMap 实现，仅作为等价性回归的参照。
fn canonicalize_reference(calls: Vec<UsageCall>) -> CanonicalUsageSet {
    let empty_tokens = empty_tokens_for_calls(&calls);
    let mut canonical = BTreeMap::<String, UsageCall>::new();
    let mut duplicate_source_count = 0_u64;
    let mut duplicate_counts_by_call = BTreeMap::<String, u64>::new();
    let mut selected_confidence_by_call = BTreeMap::<String, Confidence>::new();
    let mut conflicted_calls = BTreeSet::<String>::new();
    let mut warnings = Vec::new();

    for call in calls {
        let logical_call_id = call.logical_call_id.clone();
        let incoming_confidence = call.confidence;

        match canonical.get_mut(&logical_call_id) {
            None => {
                selected_confidence_by_call.insert(logical_call_id.clone(), incoming_confidence);
                canonical.insert(logical_call_id, call);
            }
            Some(existing) => {
                duplicate_source_count = duplicate_source_count.saturating_add(1);
                let call_duplicate_count = duplicate_counts_by_call
                    .entry(logical_call_id.clone())
                    .or_default();
                *call_duplicate_count = call_duplicate_count.saturating_add(1);

                let selected_confidence = selected_confidence_by_call
                    .get(&logical_call_id)
                    .copied()
                    .unwrap_or(existing.confidence);
                let conflict = existing.usage != call.usage;
                if conflict {
                    conflicted_calls.insert(logical_call_id.clone());
                    warnings.push(CanonicalizationWarning {
                        logical_call_id: logical_call_id.clone(),
                        kind: CanonicalizationWarningKind::ConflictingDuplicate,
                    });
                    if confidence_rank(incoming_confidence) > confidence_rank(selected_confidence) {
                        let mut replacement = call.clone();
                        replacement.provenance = existing.provenance.clone();
                        *existing = replacement;
                        selected_confidence_by_call
                            .insert(logical_call_id.clone(), incoming_confidence);
                    }
                    existing.confidence = Confidence::Suspected;
                } else if confidence_rank(incoming_confidence)
                    > confidence_rank(selected_confidence)
                {
                    selected_confidence_by_call
                        .insert(logical_call_id.clone(), incoming_confidence);
                    if !conflicted_calls.contains(&logical_call_id) {
                        existing.confidence = incoming_confidence;
                    }
                }

                merge_provenance(&mut existing.provenance, call.provenance);
            }
        }
    }

    for call in canonical.values_mut() {
        call.provenance.sort_by(|left, right| {
            left.root_id
                .cmp(&right.root_id)
                .then_with(|| left.source_id.cmp(&right.source_id))
                .then_with(|| left.relative_label.cmp(&right.relative_label))
                .then_with(|| left.archived.cmp(&right.archived))
        });
    }

    CanonicalUsageSet {
        total_token_accounting: TotalTokenAccounting::Observed,
        empty_tokens,
        calls: canonical.into_values().collect(),
        duplicate_source_count,
        duplicate_counts_by_call,
        warnings,
        snapshots: Vec::new(),
    }
}


/// 小型确定性伪随机数，避免引入测试依赖。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }
}

/// 构造含重复 ID、同值重复、冲突重复与不同置信度的输入。
fn random_calls(seed: u64, count: usize) -> Vec<UsageCall> {
    let mut rng = Lcg(seed);
    (0..count)
        .map(|position| {
            let id = rng.next(12);
            let tokens = 100 + rng.next(3) * 10;
            let confidence = match rng.next(3) {
                0 => Confidence::Suspected,
                1 => Confidence::Derived,
                _ => Confidence::Exact,
            };
            UsageCall {
                logical_call_id: format!("call-{id:02}"),
                occurred_at_epoch_ms: 1_000 + position as i64,
                model: Some(format!("model-{}", rng.next(2))),
                reasoning_effort: None,
                project_key: None,
                project_label: None,
                thread_key: format!("thread-{}", rng.next(3)),
                thread_label: None,
                usage: TokenUsage::new(tokens, 0, None, 10, 0, Some(tokens + 10)).unwrap(),
                adapter_consistency_key: None,
                confidence,
                provenance: vec![SourceProvenance {
                    source_id: format!("source-{}", rng.next(5)),
                    root_id: format!("root-{}", rng.next(2)),
                    relative_label: format!("file-{position}"),
                    archived: rng.next(2) == 0,
                }],
            }
        })
        .collect()
}

/// 两种实现对大量随机输入给出完全相同的集合、计数与警告顺序。
#[test]
fn sorted_fold_matches_the_per_call_map_reference() {
    for seed in 0..300_u64 {
        let count = (seed % 40) as usize;
        let calls = random_calls(seed, count);
        let expected = canonicalize_reference(calls.clone());
        let actual = canonicalize_usage_calls(calls);
        assert_eq!(actual, expected, "seed {seed}");
    }
}

/// 空输入与全部唯一输入也保持一致。
#[test]
fn sorted_fold_handles_empty_and_unique_inputs() {
    assert_eq!(canonicalize_usage_calls(Vec::new()), canonicalize_reference(Vec::new()));
    let unique: Vec<_> = random_calls(7, 1).into_iter().collect();
    assert_eq!(canonicalize_usage_calls(unique.clone()), canonicalize_reference(unique));
}
