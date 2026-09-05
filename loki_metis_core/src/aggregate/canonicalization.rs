//! 调用级去重与可见性衍生。

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Confidence, SessionTokenSnapshot, SourceProvenance, TokenUsage, TotalTokenAccounting, UsageCall,
};

/// 描述 definite duplicate 合并时发现的质量问题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CanonicalizationWarningKind {
    /// 同一逻辑调用指纹携带不同 Token 字段。
    ConflictingDuplicate,
}

/// 描述不含正文或绝对路径的 canonical 质量警告。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalizationWarning {
    /// 出现冲突的逻辑调用指纹。
    pub logical_call_id: String,
    /// 稳定警告类别。
    pub kind: CanonicalizationWarningKind,
}

/// 按逻辑调用指纹去重后的调用集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalUsageSet {
    /// 当前集合的 provider 级总 Token 聚合口径；筛选子集必须原样继承。
    pub total_token_accounting: TotalTokenAccounting,
    /// 空筛选或冲突整组丢弃后仍保留的 provider Token 分项可用性。
    pub empty_tokens: TokenUsage,
    /// 每个逻辑调用只保留一次的 canonical 事实。
    pub calls: Vec<UsageCall>,
    /// 被合并的重复来源总数。
    pub duplicate_source_count: u64,
    /// 按逻辑调用保存被合并观察数。
    pub duplicate_counts_by_call: BTreeMap<String, u64>,
    /// 由逻辑调用冲突生成的质量警告。
    pub warnings: Vec<CanonicalizationWarning>,
    /// Codex 历史兼容 cumulative 快照；不参与 ADR-104 生产聚合。
    pub snapshots: Vec<SessionTokenSnapshot>,
}

/// 统一去重结果并保持 provenance。
pub fn canonicalize_usage_calls(calls: Vec<UsageCall>) -> CanonicalUsageSet {
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

impl CanonicalUsageSet {
    /// 把单一 provider 的已批准总量口径附到 canonical 集合并返回自身。
    pub fn with_total_token_accounting(mut self, accounting: TotalTokenAccounting) -> Self {
        self.total_token_accounting = accounting;
        self
    }
}

/// 按 thread + 时间 + logical_call_id 去重会话快照，合并跨文件 provenance。
///
/// 同一 `session_id` 出现在活动文件与归档副本时不得把 latest 计两次。
pub fn canonicalize_session_snapshots(
    snapshots: Vec<SessionTokenSnapshot>,
) -> Vec<SessionTokenSnapshot> {
    let mut canonical =
        std::collections::BTreeMap::<(String, i64, String), SessionTokenSnapshot>::new();
    for snapshot in snapshots {
        let key = (
            snapshot.thread_key.clone(),
            snapshot.occurred_at_epoch_ms,
            snapshot.logical_call_id.clone(),
        );
        match canonical.get_mut(&key) {
            None => {
                canonical.insert(key, snapshot);
            }
            Some(existing) => {
                merge_provenance(&mut existing.provenance, snapshot.provenance);
            }
        }
    }
    let mut snapshots = canonical.into_values().collect::<Vec<_>>();
    for snapshot in &mut snapshots {
        snapshot.provenance.sort_by(|left, right| {
            left.root_id
                .cmp(&right.root_id)
                .then_with(|| left.source_id.cmp(&right.source_id))
                .then_with(|| left.relative_label.cmp(&right.relative_label))
                .then_with(|| left.archived.cmp(&right.archived))
        });
    }
    snapshots
}

/// 把已去重快照附到 canonical 集合；调用去重结果保持不变。
pub fn attach_session_snapshots(
    mut canonical: CanonicalUsageSet,
    snapshots: Vec<SessionTokenSnapshot>,
) -> CanonicalUsageSet {
    canonical.snapshots = canonicalize_session_snapshots(snapshots);
    canonical
}

/// 计算空集合时可见字段可用性，避免把未知误标记为 0。
pub(crate) fn empty_tokens_for_calls(calls: &[UsageCall]) -> TokenUsage {
    let Some(_) = calls.first() else {
        return TokenUsage::zero();
    };

    TokenUsage::zero_with_component_availability(
        calls
            .iter()
            .all(|call| call.usage.cached_input_tokens.is_some()),
        calls
            .iter()
            .all(|call| call.usage.cache_write_input_tokens.is_some()),
        calls
            .iter()
            .all(|call| call.usage.reasoning_output_tokens.is_some()),
    )
}

/// 合并重复调用的来源证据，并保持每个物理来源只出现一次。
fn merge_provenance(existing: &mut Vec<SourceProvenance>, incoming: Vec<SourceProvenance>) {
    for provenance in incoming {
        if !existing.contains(&provenance) {
            existing.push(provenance);
        }
    }
}

/// 把置信度映射为可比较等级，供冲突事实选择使用。
const fn confidence_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::Suspected => 0,
        Confidence::Derived => 1,
        Confidence::Exact => 2,
    }
}

/// 返回两份事实中较保守的置信度，避免聚合后夸大数据质量。
pub(crate) fn lower_confidence(left: Confidence, right: Confidence) -> Confidence {
    if confidence_rank(left) <= confidence_rank(right) {
        left
    } else {
        right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::TokenUsage;

    /// 验证只有源数据实际提供的分项才会被标记为“可用（零值）”，
    /// 缺失的分项保持 `None` 而不是被误判为已提供的零。
    #[test]
    fn empty_tokens_for_calls_keep_partial_components_available() {
        let call = UsageCall {
            logical_call_id: "a".to_owned(),
            occurred_at_epoch_ms: 1,
            model: Some("gpt".to_owned()),
            reasoning_effort: None,
            project_key: None,
            thread_key: "t".to_owned(),
            project_label: None,
            thread_label: None,
            usage: TokenUsage::new_with_availability(2, Some(1), None, 3, None, Some(5))
                .expect("fixture is valid"),
            adapter_consistency_key: None,
            confidence: Confidence::Exact,
            provenance: vec![],
        };
        let empty = empty_tokens_for_calls(&[call]);
        assert_eq!(empty.cached_input_tokens, Some(0));
        assert!(empty.cache_write_input_tokens.is_none());
        assert!(empty.reasoning_output_tokens.is_none());
    }
}
