//! 聚合累计与一致性摘要。

use std::collections::BTreeSet;

use super::{CanonicalUsageSet, lower_confidence};
use crate::{Confidence, LocalUsageAggregate, TokenUsage, TokenUsageError};

/// 按 canonical 调用集合计算可追溯聚合。
pub fn aggregate_canonical_usage(
    canonical: &CanonicalUsageSet,
) -> Result<LocalUsageAggregate, TokenUsageError> {
    let is_empty = canonical.calls.is_empty();
    let mut tokens = if is_empty {
        canonical.empty_tokens.clone()
    } else {
        TokenUsage::zero()
    };

    let mut cached_read_call_count = if is_empty {
        canonical.empty_tokens.cached_input_tokens.map(|_| 0_u64)
    } else {
        Some(0_u64)
    };

    let mut thread_keys = BTreeSet::new();
    let mut root_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut cross_root_duplicate_source_count = 0_u64;
    let mut confidence = Confidence::Exact;
    let mut accounted_total = 0_u64;

    for call in &canonical.calls {
        tokens = tokens.checked_add(&call.usage)?;
        accounted_total = accounted_total
            .checked_add(
                call.usage
                    .accounted_total_tokens(canonical.total_token_accounting),
            )
            .ok_or(TokenUsageError::Overflow)?;
        match call.usage.had_cache_read() {
            Some(true) => {
                cached_read_call_count = cached_read_call_count
                    .map(|count| count.checked_add(1).ok_or(TokenUsageError::Overflow))
                    .transpose()?;
            }
            Some(false) => {}
            None => cached_read_call_count = None,
        }

        thread_keys.insert(call.thread_key.clone());
        let mut source_counts_by_root = std::collections::BTreeMap::<&str, u64>::new();
        for provenance in &call.provenance {
            root_ids.insert(provenance.root_id.clone());
            source_ids.insert(provenance.source_id.clone());
            let root_source_count = source_counts_by_root
                .entry(provenance.root_id.as_str())
                .or_default();
            *root_source_count = root_source_count
                .checked_add(1)
                .ok_or(TokenUsageError::Overflow)?;
        }

        let call_source_count = source_counts_by_root
            .values()
            .try_fold(0_u64, |total, count| total.checked_add(*count))
            .ok_or(TokenUsageError::Overflow)?;
        let single_root_baseline_count = source_counts_by_root
            .values()
            .copied()
            .max()
            .unwrap_or_default();
        cross_root_duplicate_source_count = cross_root_duplicate_source_count
            .checked_add(call_source_count.saturating_sub(single_root_baseline_count))
            .ok_or(TokenUsageError::Overflow)?;
        confidence = lower_confidence(confidence, call.confidence);
    }

    let call_count = u64::try_from(canonical.calls.len()).map_err(|_| TokenUsageError::Overflow)?;
    let thread_count = u64::try_from(thread_keys.len()).map_err(|_| TokenUsageError::Overflow)?;
    let root_count = u64::try_from(root_ids.len()).map_err(|_| TokenUsageError::Overflow)?;
    let source_count = u64::try_from(source_ids.len()).map_err(|_| TokenUsageError::Overflow)?;
    tokens.total_tokens = accounted_total;
    let cache_read_basis_points = tokens.cache_read_basis_points();

    Ok(LocalUsageAggregate {
        tokens,
        call_count,
        cached_read_call_count,
        thread_count,
        root_count,
        source_count,
        duplicate_source_count: canonical.duplicate_source_count,
        cross_root_duplicate_source_count,
        cache_read_basis_points,
        confidence,
    })
}

#[cfg(test)]
#[path = "owned_segment_accounting.rs"]
mod owned_segment_accounting;

#[cfg(test)]
#[path = "utc_day_owned_accounting.rs"]
mod utc_day_owned_accounting;
