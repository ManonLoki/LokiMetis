//! 定义本机 canonical 调用的固定维度、有界分组和可加总统计量。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{CanonicalUsageSet, Confidence, TokenUsage, TokenUsageError, UsageCall};

/// 统计页最多展示的分组数量；超出部分归入 remainder。
pub const STATISTICS_GROUP_LIMIT: usize = 10;

/// 标识本机统计允许使用的固定分组维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageDimension {
    /// 按上游记录的模型名分组；用量页缺省与旧设置缺字段时使用。
    #[default]
    Model,
    /// 按上游记录的推理强度分组。
    ReasoningEffort,
    /// 按内容无关的项目键分组。
    Project,
    /// 按内容无关的线程键分组。
    Thread,
    /// 按确定性的 canonical provenance 数据根分组。
    Root,
}

/// 保存可逐字段安全相加的统计量；唯一线程和来源数只属于窗口摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMeasure {
    /// 全部 Token 字段的安全聚合。
    pub tokens: TokenUsage,
    /// canonical 逻辑调用数量。
    pub call_count: u64,
    /// 实际包含缓存读取 Token 的调用数量。
    pub cached_read_call_count: Option<u64>,
    /// 所含调用被合并的物理来源观察数量。
    pub duplicate_source_count: u64,
    /// 聚合缓存读取占比；零输入时不适用。
    pub cache_read_basis_points: Option<u16>,
    /// 所含事实中的最低置信度。
    pub confidence: Confidence,
}

impl UsageMeasure {
    /// 逐字段相加两个互斥调用集合，供日桶与 Top-N 对账。
    pub fn checked_add(&self, other: &Self) -> Result<Self, TokenUsageError> {
        let tokens = self.tokens.checked_add(&other.tokens)?;
        Ok(Self {
            call_count: self
                .call_count
                .checked_add(other.call_count)
                .ok_or(TokenUsageError::Overflow)?,
            cached_read_call_count: self
                .cached_read_call_count
                .zip(other.cached_read_call_count)
                .map(|(left, right)| left.checked_add(right).ok_or(TokenUsageError::Overflow))
                .transpose()?,
            duplicate_source_count: self
                .duplicate_source_count
                .checked_add(other.duplicate_source_count)
                .ok_or(TokenUsageError::Overflow)?,
            cache_read_basis_points: tokens.cache_read_basis_points(),
            confidence: lower_confidence(self.confidence, other.confidence),
            tokens,
        })
    }
}

/// 描述一个已按固定维度聚合的内部稳定分组键和统计量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageGroup {
    /// 原始安全字段；空值表示未知或未归类，不用于展示绝对路径。
    pub key: Option<String>,
    /// 该组的可加总统计量。
    pub measure: UsageMeasure,
}

/// 描述确定性 Top-N 分组、可选其余项及同一快照总计。
/// 生成稳定、不可逆向推导原始 key 的跨端展示行 ID。
pub fn stable_group_id(dimension: UsageDimension, key: Option<&str>) -> String {
    let namespace = match dimension {
        UsageDimension::Model => "model",
        UsageDimension::ReasoningEffort => "reasoning",
        UsageDimension::Project => "project",
        UsageDimension::Thread => "thread",
        UsageDimension::Root => "root",
    };
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in namespace
        .bytes()
        .chain([0])
        .chain(key.unwrap_or("<unknown>").bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("statistics-{hash:016x}")
}

/// 有界分组结果：前 N 组、被截断的其余项汇总，以及用于对账的窗口总计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedUsageGroups {
    /// 按总 Token、调用数和稳定键排序后的前 N 组。
    pub groups: Vec<UsageGroup>,
    /// 超过上限的全部组；没有被截断时为空。
    pub remainder: Option<UsageMeasure>,
    /// 与分组使用相同 canonical 调用集合的总计。
    pub total: UsageMeasure,
}

/// 汇总 canonical 集合中可安全逐字段相加的统计量。
pub fn summarize_usage(canonical: &CanonicalUsageSet) -> Result<UsageMeasure, TokenUsageError> {
    measure_calls(canonical.calls.iter(), canonical)
}

/// 按固定维度生成确定性 Top-N 与其余项，不接受任意字段或表达式。
// 步骤拆解（前端“按模型/项目/线程等维度统计”功能的核心实现）：
//   1. 按 dimension 把每条调用分到一个 key（如模型名）下的桶；
//   2. 对每个桶调用 measure_calls 算出可加总的统计量；
//   3. 按总 Token 降序、调用数降序、key 字典序做稳定排序；
//   4. 只保留前 top_limit 组，其余全部合并成一个 “remainder”（其余项），
//      这样前端只需要渲染有限行数，又能通过 remainder 对账总量不丢失。
pub fn group_usage(
    canonical: &CanonicalUsageSet,
    dimension: UsageDimension,
    top_limit: usize,
) -> Result<BoundedUsageGroups, TokenUsageError> {
    let mut calls_by_key = BTreeMap::<Option<String>, Vec<&UsageCall>>::new();
    for call in &canonical.calls {
        calls_by_key
            .entry(group_key(call, dimension))
            .or_default()
            .push(call);
    }

    let mut groups = calls_by_key
        .iter()
        .map(|(key, calls)| {
            Ok(UsageGroup {
                key: key.clone(),
                measure: measure_calls(calls.iter().copied(), canonical)?,
            })
        })
        .collect::<Result<Vec<_>, TokenUsageError>>()?;
    // 三级排序键：总 Token 降序为主，同 Token 时按调用数降序，
    // 再打平时按 key 字典序——保证结果 100% 确定，不依赖 HashMap 遍历顺序。
    groups.sort_by(|left, right| {
        right
            .measure
            .tokens
            .total_tokens
            .cmp(&left.measure.tokens.total_tokens)
            .then_with(|| right.measure.call_count.cmp(&left.measure.call_count))
            .then_with(|| stable_key(&left.key).cmp(stable_key(&right.key)))
    });

    // 排序后，下标 >= top_limit 的组都属于“其余项”；先收集这些组对应的 key 集合，
    // 再回到原始 canonical.calls 里按 key 重新筛选出这些调用，统一 measure_calls 一次，
    // 这样 remainder 里的 duplicate_source_count 等字段仍然是真实可加总的统计量，
    // 而不是简单地把已经算好的多个 UsageMeasure 直接相加（那样会丢失 duplicate 归因）。
    let remainder_keys = groups
        .iter()
        .skip(top_limit)
        .map(|group| group.key.clone())
        .collect::<BTreeSet<_>>();
    let remainder = if remainder_keys.is_empty() {
        None
    } else {
        Some(measure_calls(
            canonical
                .calls
                .iter()
                .filter(|call| remainder_keys.contains(&group_key(call, dimension))),
            canonical,
        )?)
    };
    groups.truncate(top_limit);

    Ok(BoundedUsageGroups {
        groups,
        remainder,
        total: summarize_usage(canonical)?,
    })
}

/// 为单个调用选择固定维度键；空白上游值与缺失值都归为未知。
// UsageDimension::Root 分支比其他维度复杂：一条调用可能有多个 provenance
// （多个来源观察到它），这里选“排序最靠前”的那个 provenance 的 root_id
// 作为该调用归属的根，保证同一条调用在重复统计时永远归到同一个根（确定性）。
fn group_key(call: &UsageCall, dimension: UsageDimension) -> Option<String> {
    match dimension {
        UsageDimension::Model => normalized_optional_key(call.model.as_deref()),
        UsageDimension::ReasoningEffort => {
            normalized_optional_key(call.reasoning_effort.as_deref())
        }
        UsageDimension::Project => normalized_optional_key(call.project_key.as_deref()),
        UsageDimension::Thread => normalized_optional_key(Some(&call.thread_key)),
        UsageDimension::Root => call
            .provenance
            .iter()
            .min_by(|left, right| {
                left.root_id
                    .cmp(&right.root_id)
                    .then_with(|| left.source_id.cmp(&right.source_id))
                    .then_with(|| left.relative_label.cmp(&right.relative_label))
                    .then_with(|| left.archived.cmp(&right.archived))
            })
            .and_then(|source| normalized_optional_key(Some(&source.root_id))),
    }
}

/// 把空白可选字段收敛为未知键，保留非空上游安全标识原值。
fn normalized_optional_key(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

/// 为可选键提供确定性并列排序；未知组始终排在同值已知组之前。
fn stable_key(key: &Option<String>) -> &str {
    key.as_deref().unwrap_or("")
}

/// 对一组互斥 canonical 调用构造可加总统计量并保留 duplicate 事实。
pub(crate) fn measure_calls<'a>(
    calls: impl Iterator<Item = &'a UsageCall>,
    canonical: &CanonicalUsageSet,
) -> Result<UsageMeasure, TokenUsageError> {
    let mut calls = calls.peekable();
    let is_empty = calls.peek().is_none();
    let mut tokens = if is_empty {
        canonical.empty_tokens.clone()
    } else {
        TokenUsage::zero()
    };
    let mut call_count = 0_u64;
    let mut cached_read_call_count = if is_empty {
        canonical.empty_tokens.cached_input_tokens.map(|_| 0_u64)
    } else {
        Some(0_u64)
    };
    let mut duplicate_source_count = 0_u64;
    let mut confidence = Confidence::Exact;
    let mut accounted_total = 0_u64;

    for call in calls {
        tokens = tokens.checked_add(&call.usage)?;
        accounted_total = accounted_total
            .checked_add(
                call.usage
                    .accounted_total_tokens(canonical.total_token_accounting),
            )
            .ok_or(TokenUsageError::Overflow)?;
        call_count = call_count.checked_add(1).ok_or(TokenUsageError::Overflow)?;
        match call.usage.had_cache_read() {
            Some(true) => {
                cached_read_call_count = cached_read_call_count
                    .map(|count| count.checked_add(1).ok_or(TokenUsageError::Overflow))
                    .transpose()?;
            }
            Some(false) => {}
            None => cached_read_call_count = None,
        }
        duplicate_source_count = duplicate_source_count
            .checked_add(
                canonical
                    .duplicate_counts_by_call
                    .get(&call.logical_call_id)
                    .copied()
                    .unwrap_or_default(),
            )
            .ok_or(TokenUsageError::Overflow)?;
        confidence = lower_confidence(confidence, call.confidence);
    }

    tokens.total_tokens = accounted_total;
    Ok(UsageMeasure {
        cache_read_basis_points: tokens.cache_read_basis_points(),
        tokens,
        call_count,
        cached_read_call_count,
        duplicate_source_count,
        confidence,
    })
}

/// 返回两个统计事实中更保守的置信度。
fn lower_confidence(left: Confidence, right: Confidence) -> Confidence {
    if confidence_rank(left) <= confidence_rank(right) {
        left
    } else {
        right
    }
}

/// 把置信度映射为只用于保守聚合的稳定等级。
const fn confidence_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::Suspected => 0,
        Confidence::Derived => 1,
        Confidence::Exact => 2,
    }
}

#[cfg(test)]
mod tests;
