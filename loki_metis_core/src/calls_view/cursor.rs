//! 调用列表分页游标与查询/快照指纹，供端到端分页稳定性校验。

use super::row::UsageCallRow;
use super::{UsageCallSortDirection, UsageCallSortField, UsageCallsQuery};
use crate::{LocalIndexState, TotalTokenAccounting, is_safe_usage_filter_id};

/// 调用分页游标固定前缀。
pub const CURSOR_PREFIX: &str = "c1_";

/// 游标长度上限（防御式防止超长 payload）。
pub const MAX_CURSOR_CHARS: usize = 512;

/// 返回分页游标无效时的统一中文错误。
pub fn cursor_error() -> String {
    "调用游标无效或已过期，请从第一页重新开始。".to_owned()
}

/// 将筛选、排序指针编码为版本化指纹游标。
pub fn encode_cursor(
    query_fingerprint: u64,
    snapshot_fingerprint: u64,
    last_call_id: &str,
) -> String {
    let payload = format!("1|{query_fingerprint:016x}|{snapshot_fingerprint:016x}|{last_call_id}");
    let encoded = payload
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{CURSOR_PREFIX}{encoded}")
}

/// 把分页查询游标解析为固定字段；失败返回 `None`。
pub fn decode_cursor(cursor: &str) -> Option<DecodedCursor> {
    if cursor.len() > MAX_CURSOR_CHARS {
        return None;
    }
    let encoded = cursor.strip_prefix(CURSOR_PREFIX)?;
    if encoded.is_empty() || !encoded.len().is_multiple_of(2) {
        return None;
    }

    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    let (pairs, remainder) = encoded.as_bytes().as_chunks::<2>();
    debug_assert!(remainder.is_empty(), "偶数长度游标不应留下半字节");
    for pair in pairs {
        let text = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    let payload = String::from_utf8(bytes).ok()?;

    let mut fields = payload.split('|');
    if fields.next()? != "1" {
        return None;
    }
    let query_fingerprint = u64::from_str_radix(fields.next()?, 16).ok()?;
    let snapshot_fingerprint = u64::from_str_radix(fields.next()?, 16).ok()?;
    let last_call_id = fields.next()?.to_owned();

    if fields.next().is_some() || !is_safe_usage_filter_id(&last_call_id) {
        return None;
    }

    Some(DecodedCursor {
        query_fingerprint,
        snapshot_fingerprint,
        last_call_id,
    })
}

/// 计算过滤参数稳定指纹。
pub fn fingerprint_query(query: &UsageCallsQuery) -> u64 {
    let mut hash = Fingerprint::new();
    for value in [
        query.filters.model.as_deref(),
        query.filters.reasoning_effort.as_deref(),
        query.filters.project.as_deref(),
        query.filters.thread.as_deref(),
        query.filters.root.as_deref(),
    ] {
        hash.add(value.unwrap_or(""));
    }
    hash.add(sort_field_key(query.sort_field));
    hash.add(match query.sort_direction {
        UsageCallSortDirection::Asc => "asc",
        UsageCallSortDirection::Desc => "desc",
    });
    hash.finish()
}

/// 计算当前快照稳定指纹。
pub fn fingerprint_snapshot(
    rows: &[UsageCallRow],
    index_state: LocalIndexState,
    filters: &crate::UsageAvailableFilters,
    accounting: TotalTokenAccounting,
) -> u64 {
    let mut ordered = rows.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.call.logical_call_id.cmp(&right.call.logical_call_id));

    let mut hash = Fingerprint::new();
    hash.add(index_state_key(index_state));
    for row in ordered {
        hash.add(&row.call.logical_call_id);
        hash.add(&row.call.occurred_at_epoch_ms.to_string());
        hash.add(&row.model_id);
        hash.add(&row.reasoning_effort_id);
        hash.add(&row.project_id);
        hash.add(&row.thread_id);
        hash.add(&row.call.usage.input_tokens.to_string());
        hash.add(&optional_token_fingerprint(
            row.call.usage.cached_input_tokens,
        ));
        hash.add(
            &row.call
                .usage
                .cache_write_input_tokens
                .map_or_else(String::new, |value| value.to_string()),
        );
        hash.add(&row.call.usage.output_tokens.to_string());
        hash.add(&optional_token_fingerprint(
            row.call.usage.reasoning_output_tokens,
        ));
        hash.add(
            &row.call
                .usage
                .accounted_total_tokens(accounting)
                .to_string(),
        );
        hash.add(if row.call.usage.total_is_derived {
            "derived-total"
        } else {
            "observed-total"
        });
        hash.add(match row.call.confidence {
            crate::Confidence::Exact => "exact",
            crate::Confidence::Derived => "derived",
            crate::Confidence::Suspected => "suspected",
        });
        for root_id in &row.root_ids {
            hash.add(root_id);
        }
    }

    let filter_groups = [
        ("models", &filters.models),
        ("reasoningEfforts", &filters.reasoning_efforts),
        ("projects", &filters.projects),
        ("threads", &filters.threads),
        ("roots", &filters.roots),
    ];
    for (dimension, options) in filter_groups {
        hash.add(dimension);
        for option in options {
            hash.add(&option.id);
            hash.add(&option.label);
        }
    }

    hash.finish()
}

/// 把 Agent 集合、provider 与 parser 版本等快照上下文并入基础指纹。
pub(crate) fn fingerprint_snapshot_context(base: u64, context: &[String]) -> u64 {
    let mut hash = Fingerprint::new();
    hash.add(&base.to_string());
    for value in context {
        hash.add(value);
    }
    hash.finish()
}

/// 在排序页边界前向定位下一行；查询/快照变化会报错重置。
pub fn cursor_start(
    cursor: Option<&str>,
    query_fingerprint: u64,
    snapshot_fingerprint: u64,
    rows: &[UsageCallRow],
) -> Result<usize, String> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let decoded = decode_cursor(cursor).ok_or_else(cursor_error)?;
    if decoded.query_fingerprint != query_fingerprint
        || decoded.snapshot_fingerprint != snapshot_fingerprint
    {
        return Err(cursor_error());
    }
    rows.iter()
        .position(|row| row.call.logical_call_id == decoded.last_call_id)
        .map(|position| position.saturating_add(1))
        .ok_or_else(cursor_error)
}

/// 将可选 Token 值编码进游标指纹，并区分缺失与真实零值。
fn optional_token_fingerprint(value: Option<u64>) -> String {
    value.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
}

/// 返回本机索引状态在稳定游标指纹中的规范键。
fn index_state_key(index_state: LocalIndexState) -> &'static str {
    match index_state {
        LocalIndexState::NotScanned => "notScanned",
        LocalIndexState::NeedsRescan => "needsRescan",
        LocalIndexState::ReadyNoCalls => "readyNoCalls",
        LocalIndexState::Ready => "ready",
    }
}

/// 返回调用排序字段在稳定游标指纹中的规范键。
fn sort_field_key(field: UsageCallSortField) -> &'static str {
    match field {
        UsageCallSortField::OccurredAt => "occurredAt",
        UsageCallSortField::Model => "model",
        UsageCallSortField::ReasoningEffort => "reasoningEffort",
        UsageCallSortField::InputTokens => "inputTokens",
        UsageCallSortField::CachedInputTokens => "cachedInputTokens",
        UsageCallSortField::UncachedInputTokens => "uncachedInputTokens",
        UsageCallSortField::OutputTokens => "outputTokens",
        UsageCallSortField::ReasoningOutputTokens => "reasoningOutputTokens",
        UsageCallSortField::TotalTokens => "totalTokens",
    }
}

/// 解析后的游标内部表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedCursor {
    /// 查询指纹。
    pub query_fingerprint: u64,
    /// 快照指纹。
    pub snapshot_fingerprint: u64,
    /// 上一页最后一条调用 ID。
    pub last_call_id: String,
}

/// 非加密的 FNV-1a 指纹器。
#[derive(Default)]
struct Fingerprint(u64);

impl Fingerprint {
    /// 创建 FNV-1a 算法初始值。
    const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    /// 向指纹中追加一个可见字段。
    fn add(&mut self, value: &str) {
        for byte in value.bytes().chain([0]) {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    /// 返回当前指纹。
    const fn finish(self) -> u64 {
        self.0
    }
}
