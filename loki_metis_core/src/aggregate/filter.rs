//! 切片选择与质量信息保留。

use std::collections::{BTreeMap, BTreeSet};

use crate::{CanonicalUsageSet, SessionTokenSnapshot, UsageCall};

/// 按 `keep` 谓词筛选 canonical 调用子集，并同步裁剪重复计数、告警与空值形状归属。
pub fn filter_canonical_usage(
    canonical: &CanonicalUsageSet,
    mut keep: impl FnMut(&UsageCall) -> bool,
) -> CanonicalUsageSet {
    let calls = canonical
        .calls
        .iter()
        .filter(|call| keep(call))
        .cloned()
        .collect::<Vec<_>>();
    build_canonical_subset(canonical, calls)
}

/// 单次遍历按 `classify` 把 canonical 调用分派进两个互斥子集，供同一调用集需要
/// 两个不相交切片时使用，避免各自重新扫描一遍全量调用。
pub fn partition_canonical_usage_two(
    canonical: &CanonicalUsageSet,
    mut classify: impl FnMut(&UsageCall) -> Option<bool>,
) -> (CanonicalUsageSet, CanonicalUsageSet) {
    let mut first = Vec::new();
    let mut second = Vec::new();
    for call in &canonical.calls {
        match classify(call) {
            Some(true) => first.push(call.clone()),
            Some(false) => second.push(call.clone()),
            None => {}
        }
    }
    (
        build_canonical_subset(canonical, first),
        build_canonical_subset(canonical, second),
    )
}

/// 用已选定的调用子集重建同步裁剪过重复计数、告警与空值形状归属的 canonical 集合。
fn build_canonical_subset(
    canonical: &CanonicalUsageSet,
    calls: Vec<UsageCall>,
) -> CanonicalUsageSet {
    let selected_ids = calls
        .iter()
        .map(|call| call.logical_call_id.as_str())
        .collect::<BTreeSet<_>>();

    let duplicate_counts_by_call = canonical
        .duplicate_counts_by_call
        .iter()
        .filter(|(logical_call_id, _)| selected_ids.contains(logical_call_id.as_str()))
        .map(|(logical_call_id, count)| (logical_call_id.clone(), *count))
        .collect::<BTreeMap<_, _>>();

    let duplicate_source_count = duplicate_counts_by_call
        .values()
        .fold(0_u64, |total, count| total.saturating_add(*count));

    let warnings = canonical
        .warnings
        .iter()
        .filter(|warning| selected_ids.contains(warning.logical_call_id.as_str()))
        .cloned()
        .collect();

    CanonicalUsageSet {
        total_token_accounting: canonical.total_token_accounting,
        empty_tokens: canonical.empty_tokens.clone(),
        calls,
        duplicate_source_count,
        duplicate_counts_by_call,
        warnings,
        snapshots: Vec::new(),
    }
}

/// 把父集合中满足时间谓词的会话快照拷进已筛过的 canonical 子集。
pub fn copy_matching_snapshots(
    parent: &CanonicalUsageSet,
    mut subset: CanonicalUsageSet,
    mut keep: impl FnMut(&SessionTokenSnapshot) -> bool,
) -> CanonicalUsageSet {
    subset.snapshots = parent
        .snapshots
        .iter()
        .filter(|snapshot| keep(snapshot))
        .cloned()
        .collect();
    subset
}
