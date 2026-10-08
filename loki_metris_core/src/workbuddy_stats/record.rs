//! WorkBuddy project JSONL 用量事件的校验、去重与安全归一化。

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::{TokenUsage, safe_path_basename, safe_technical_label, safe_thread_title};

/// 单条上游 usage 事件允许声明的最大请求数，限制异常 fan-out 的内存消耗。
const MAX_REQUESTS_PER_EVENT: i64 = 10_000;

/// 区分 WorkBuddy 顶层会话与受限 `subagents` 布局中的请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbuddyUsageOrigin {
    /// `projects/<project>/<session>.jsonl` 中的请求。
    TopLevel,
    /// `projects/<project>/<session>/subagents/<agent>.jsonl` 中的请求。
    Subagent,
}

/// adapter 从单条 JSONL 记录投影出的最小用量事实。
///
/// 所有身份字段在进入 core 前已经过稳定哈希；项目标签只允许安全末段。该结构
/// 不实现 `Debug` 或序列化，避免未来日志误带尚未校验的模型文本。
#[derive(Clone, PartialEq)]
pub struct WorkbuddyUsageEventRecord {
    /// `(sessionId, providerData.messageId)` 生成的内容无关稳定调用 ID。
    pub logical_call_id: String,
    /// 上游 session ID 生成的内容无关稳定线程 ID。
    pub session_key: String,
    /// 当前来源文件生成的稳定 ID，不含路径。
    pub source_id: String,
    /// 该记录所属的 WorkBuddy 固定只读根 ID，用于按根还原展示别名与来源归属。
    pub root_id: String,
    /// 请求发生时刻的 Unix 毫秒时间戳。
    pub occurred_at_epoch_ms: i64,
    /// 当前事件实际执行的模型；core 会执行安全 slug 校验。
    pub model: Option<String>,
    /// 内容无关的项目稳定 ID。
    pub project_key: Option<String>,
    /// 已截取的安全项目末段；core 会再次校验。
    pub project_label: Option<String>,
    /// 上游 `providerData.usage.requests`。
    pub request_count: i64,
    /// 全部输入 Token，包含缓存输入。
    pub input_tokens: i64,
    /// 输入 Token 中命中缓存的子集。
    pub cached_input_tokens: i64,
    /// 输出 Token。
    pub output_tokens: i64,
    /// 上游明确总量，必须严格等于输入加输出。
    pub total_tokens: i64,
    /// 上游逐请求积分；缺失时 Token 仍可用，但积分覆盖不完整。
    pub credit: Option<f64>,
    /// 顶层或 subagent 来源。
    pub origin: WorkbuddyUsageOrigin,
}

/// WorkBuddy usage 记录校验与去重后的质量计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkbuddyUsageQuality {
    /// 字段或 Token 不变量不合法、因而被排除的记录数。
    pub invalid_record_count: u64,
    /// 相同稳定调用 ID 携带无法排序的冲突事实、因而整组被排除的记录数。
    pub conflicting_duplicate_record_count: u64,
    /// 完全一致或被后续合法更新取代的观察数量；只计最终一次用量。
    pub duplicate_record_count: u64,
}

/// core 内部已验证且已去重的逐请求聚合事件。
#[derive(Clone)]
pub(super) struct ValidatedWorkbuddyUsageRecord {
    pub(super) logical_call_id: String,
    pub(super) session_key: String,
    pub(super) source_ids: Vec<String>,
    pub(super) root_id: String,
    pub(super) occurred_at_epoch_ms: i64,
    pub(super) model: Option<String>,
    pub(super) project_key: Option<String>,
    pub(super) project_label: Option<String>,
    pub(super) request_count: u32,
    pub(super) usage: TokenUsage,
    pub(super) credit: Option<f64>,
    pub(super) origin: WorkbuddyUsageOrigin,
}

/// 只返回质量计数；当前仅用于核对校验与去重规则。
#[cfg(test)]
pub(super) fn workbuddy_usage_quality(
    records: &[WorkbuddyUsageEventRecord],
) -> WorkbuddyUsageQuality {
    prepare_workbuddy_usage_records(records).1
}

/// 校验、稳定归一化并按逻辑调用 ID 去重；只接纳同源递进观察。
pub(super) fn prepare_workbuddy_usage_records(
    records: &[WorkbuddyUsageEventRecord],
) -> (Vec<ValidatedWorkbuddyUsageRecord>, WorkbuddyUsageQuality) {
    let mut quality = WorkbuddyUsageQuality::default();
    let mut groups = BTreeMap::<&str, Vec<&WorkbuddyUsageEventRecord>>::new();
    for record in records {
        if record.logical_call_id.trim().is_empty() {
            quality.invalid_record_count = quality.invalid_record_count.saturating_add(1);
            continue;
        }
        groups
            .entry(record.logical_call_id.as_str())
            .or_default()
            .push(record);
    }

    let mut prepared = Vec::with_capacity(groups.len());
    for group in groups.into_values() {
        let mut validated = Vec::with_capacity(group.len());
        let mut group_invalid = false;
        for record in group {
            match validate_record(record) {
                Some(record) => {
                    validated.push(record);
                }
                None => {
                    quality.invalid_record_count = quality.invalid_record_count.saturating_add(1);
                    group_invalid = true;
                }
            }
        }
        if group_invalid || validated.is_empty() {
            continue;
        }

        let observation_count = validated.len();
        validated.sort_by_key(|record| record.occurred_at_epoch_ms);
        let mut selected = validated.remove(0);
        let mut conflict = false;
        for mut candidate in validated {
            if same_fact(&selected, &candidate) {
                selected.source_ids.extend(candidate.source_ids);
            } else if is_later_usage_update(&selected, &candidate) {
                candidate.source_ids.extend(selected.source_ids);
                selected = candidate;
            } else {
                conflict = true;
                break;
            }
        }
        if conflict {
            quality.conflicting_duplicate_record_count = quality
                .conflicting_duplicate_record_count
                .saturating_add(u64::try_from(observation_count).unwrap_or(u64::MAX));
            continue;
        }

        quality.duplicate_record_count = quality
            .duplicate_record_count
            .saturating_add(u64::try_from(observation_count.saturating_sub(1)).unwrap_or(u64::MAX));
        selected.source_ids.sort();
        selected.source_ids.dedup();
        prepared.push(selected);
    }
    prepared.sort_by(|left, right| {
        left.occurred_at_epoch_ms
            .cmp(&right.occurred_at_epoch_ms)
            .then_with(|| left.logical_call_id.cmp(&right.logical_call_id))
    });
    (prepared, quality)
}

/// 把一条 adapter 投影转换为强类型 TokenUsage；失败时不保留局部事实。
fn validate_record(record: &WorkbuddyUsageEventRecord) -> Option<ValidatedWorkbuddyUsageRecord> {
    if record.session_key.trim().is_empty()
        || record.source_id.trim().is_empty()
        || record.root_id.trim().is_empty()
        || Timestamp::from_millisecond(record.occurred_at_epoch_ms).is_err()
        || !(1..=MAX_REQUESTS_PER_EVENT).contains(&record.request_count)
    {
        return None;
    }
    let input_tokens = u64::try_from(record.input_tokens).ok()?;
    let cached_input_tokens = u64::try_from(record.cached_input_tokens).ok()?;
    let output_tokens = u64::try_from(record.output_tokens).ok()?;
    let total_tokens = u64::try_from(record.total_tokens).ok()?;
    if input_tokens.checked_add(output_tokens)? != total_tokens {
        return None;
    }
    let usage = TokenUsage::new_with_availability(
        input_tokens,
        Some(cached_input_tokens),
        None,
        output_tokens,
        None,
        Some(total_tokens),
    )
    .ok()?;
    let credit = record
        .credit
        .filter(|value| value.is_finite() && *value >= 0.0);
    Some(ValidatedWorkbuddyUsageRecord {
        logical_call_id: record.logical_call_id.clone(),
        session_key: record.session_key.clone(),
        source_ids: vec![record.source_id.clone()],
        root_id: record.root_id.clone(),
        occurred_at_epoch_ms: record.occurred_at_epoch_ms,
        model: record.model.as_deref().and_then(safe_thread_title),
        project_key: record.project_key.as_deref().and_then(safe_technical_label),
        project_label: record.project_label.as_deref().and_then(safe_path_basename),
        request_count: u32::try_from(record.request_count).ok()?,
        usage,
        credit,
        origin: record.origin,
    })
}

/// 判断两个同 ID 观察是否描述同一事实；来源 ID 不参与比较，只用于合并追溯。
fn same_fact(left: &ValidatedWorkbuddyUsageRecord, right: &ValidatedWorkbuddyUsageRecord) -> bool {
    same_identity(left, right)
        && left.occurred_at_epoch_ms == right.occurred_at_epoch_ms
        && left.request_count == right.request_count
        && left.usage == right.usage
        && optional_float_eq(left.credit, right.credit)
}

/// 同源晚到观察可补齐积分；总量增加时允许输出分量随输入修正。
/// 总量不变时，所有 Token 分量必须完全相同。
fn is_later_usage_update(
    previous: &ValidatedWorkbuddyUsageRecord,
    later: &ValidatedWorkbuddyUsageRecord,
) -> bool {
    same_identity(previous, later)
        && previous
            .source_ids
            .iter()
            .any(|source| later.source_ids.contains(source))
        && later.occurred_at_epoch_ms > previous.occurred_at_epoch_ms
        && later.request_count == previous.request_count
        && (if later.usage.total_tokens > previous.usage.total_tokens {
            later.usage.input_tokens >= previous.usage.input_tokens
                && later.usage.cached_input_tokens >= previous.usage.cached_input_tokens
        } else {
            later.usage == previous.usage
        })
        && match (previous.credit, later.credit) {
            (Some(previous), Some(later)) => later >= previous,
            (None, _) => true,
            (Some(_), None) => false,
        }
}

/// 调用身份、模型、项目和层级必须始终稳定；来源文件另由更新规则核对。
fn same_identity(
    left: &ValidatedWorkbuddyUsageRecord,
    right: &ValidatedWorkbuddyUsageRecord,
) -> bool {
    left.logical_call_id == right.logical_call_id
        && left.session_key == right.session_key
        && left.root_id == right.root_id
        && left.model == right.model
        && left.project_key == right.project_key
        && left.project_label == right.project_label
        && left.origin == right.origin
}

/// 已经过 finite 校验的积分使用位级比较，避免近似比较掩盖重复冲突。
fn optional_float_eq(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
        (None, None) => true,
        _ => false,
    }
}
