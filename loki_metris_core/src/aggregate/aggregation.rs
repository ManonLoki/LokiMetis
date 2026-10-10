//! 聚合累计与一致性摘要。

use std::collections::BTreeSet;

use super::{CanonicalUsageSet, lower_confidence};
use crate::{Confidence, LocalUsageAggregate, TokenUsage, TokenUsageError, TotalTokenAccounting, UsageCall};

/// 按 canonical 调用集合计算可追溯聚合。
pub fn aggregate_canonical_usage(
    canonical: &CanonicalUsageSet,
) -> Result<LocalUsageAggregate, TokenUsageError> {
    aggregate_calls(
        canonical.calls.iter(),
        &canonical.empty_tokens,
        canonical.total_token_accounting,
        canonical.duplicate_source_count,
    )
}

/// 窗口只借用所选调用；按唯一 ID 饱和累计重复数，不拷贝展示与诊断载荷。
pub(crate) fn aggregate_filtered_canonical_usage(
    canonical: &CanonicalUsageSet,
    mut keep: impl FnMut(&UsageCall) -> bool,
    empty_tokens: &TokenUsage,
    accounting: TotalTokenAccounting,
) -> Result<LocalUsageAggregate, TokenUsageError> {
    let calls = canonical.calls.iter().filter(|call| keep(call)).collect::<Vec<_>>();
    let duplicate_source_count = if canonical.duplicate_counts_by_call.is_empty() {
        0
    } else {
        let selected_ids = calls.iter().map(|call| call.logical_call_id.as_str()).collect::<BTreeSet<_>>();
        canonical.duplicate_counts_by_call.iter()
            .filter(|(id, _)| selected_ids.contains(id.as_str()))
            .fold(0_u64, |total, (_, count)| total.saturating_add(*count))
    };
    aggregate_calls(calls.into_iter(), empty_tokens, accounting, duplicate_source_count)
}

/// 共用原顺序的完整聚合折叠，保留分项未知、溢出、来源计数与最低可信度。
fn aggregate_calls<'a>(
    calls: impl ExactSizeIterator<Item = &'a UsageCall>,
    empty_tokens: &TokenUsage,
    accounting: TotalTokenAccounting,
    duplicate_source_count: u64,
) -> Result<LocalUsageAggregate, TokenUsageError> {
    let call_len = calls.len();
    let is_empty = call_len == 0;
    let mut tokens = if is_empty {
        empty_tokens.clone()
    } else {
        TokenUsage::zero()
    };

    let mut cached_read_call_count = if is_empty {
        empty_tokens.cached_input_tokens.map(|_| 0_u64)
    } else {
        Some(0_u64)
    };

    let mut thread_keys: BTreeSet<&str> = BTreeSet::new();
    let mut root_ids: BTreeSet<&str> = BTreeSet::new();
    let mut source_ids: BTreeSet<&str> = BTreeSet::new();
    let mut cross_root_duplicate_source_count = 0_u64;
    let mut confidence = Confidence::Exact;
    let mut accounted_total = 0_u64;

    for call in calls {
        tokens = tokens.checked_add(&call.usage)?;
        accounted_total = accounted_total
            .checked_add(
                call.usage.accounted_total_tokens(accounting),
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

        thread_keys.insert(call.thread_key.as_str());
        for provenance in &call.provenance {
            root_ids.insert(provenance.root_id.as_str());
            source_ids.insert(provenance.source_id.as_str());
        }

        // 单一 provenance（绝大多数调用）不可能产生跨根重复，跳过按根计数的 map 分配。
        let cross_root_delta = if call.provenance.len() > 1 {
            let mut source_counts_by_root = std::collections::BTreeMap::<&str, u64>::new();
            for provenance in &call.provenance {
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
            call_source_count.saturating_sub(single_root_baseline_count)
        } else {
            0_u64
        };
        cross_root_duplicate_source_count = cross_root_duplicate_source_count
            .checked_add(cross_root_delta)
            .ok_or(TokenUsageError::Overflow)?;
        confidence = lower_confidence(confidence, call.confidence);
    }

    let call_count = u64::try_from(call_len).map_err(|_| TokenUsageError::Overflow)?;
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
        duplicate_source_count,
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

#[cfg(test)]
#[path = "borrowed_tests.rs"]
mod borrowed_tests;
