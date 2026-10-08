//! 纯业务的调用列表视图装配：筛选、排序、分页、游标。

mod cursor;
mod row;

use std::collections::BTreeMap;

pub use cursor::{
    cursor_error, cursor_start, decode_cursor, encode_cursor, fingerprint_query,
    fingerprint_snapshot,
};
pub use row::{BuildUsageCallRowsError, UsageCallRow};

use crate::{
    Completeness, Confidence, Freshness, LocalIndexState, MetricFact, MetricScope, ProviderKind,
    SourceClientKind, TokenUsage, TotalTokenAccounting, UsageCall, agent_wire_label,
    display_label::DisplayLabelCode,
};
use row::{build_rows, collect_available_filters, compare_rows, matches_filters, validate_filters};

/// 按当前 provider 口径替换展示 total，其余可审计分项保持原样。
fn accounted_display_usage(usage: &TokenUsage, accounting: TotalTokenAccounting) -> TokenUsage {
    let mut display = usage.clone();
    display.total_tokens = usage.accounted_total_tokens(accounting);
    display
}

/// 调用列表页每页固定条数。
pub const USAGE_CALL_PAGE_SIZE: usize = 100;

/// 支持的调用列表排序字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageCallSortField {
    /// 按调用发生时间排序。
    OccurredAt,
    /// 按模型标签排序。
    Model,
    /// 按推理强度标签排序。
    ReasoningEffort,
    /// 按输入总 Token 排序。
    InputTokens,
    /// 按缓存输入 Token 排序。
    CachedInputTokens,
    /// 按未缓存输入 Token 排序。
    UncachedInputTokens,
    /// 按输出 Token 排序。
    OutputTokens,
    /// 按推理输出 Token 排序。
    ReasoningOutputTokens,
    /// 按总 Token 排序。
    TotalTokens,
}

/// 支持的调用列表排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UsageCallSortDirection {
    /// 升序。
    Asc,
    /// 降序。
    #[default]
    Desc,
}

/// 用户可提交的调用筛选。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageCallFilters {
    /// 按模型过滤。
    pub model: Option<String>,
    /// 按推理强度过滤。
    pub reasoning_effort: Option<String>,
    /// 按项目过滤。
    pub project: Option<String>,
    /// 按线程过滤。
    pub thread: Option<String>,
    /// 按数据根过滤。
    pub root: Option<String>,
}

/// 单次调用列表查询。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCallsQuery {
    /// 固定筛选项。
    pub filters: UsageCallFilters,
    /// 排序字段。
    pub sort_field: UsageCallSortField,
    /// 排序方向。
    pub sort_direction: UsageCallSortDirection,
    /// 上一页游标。
    pub cursor: Option<String>,
}

impl Default for UsageCallsQuery {
    /// 返回调用页首屏使用的稳定默认查询。
    fn default() -> Self {
        Self {
            filters: UsageCallFilters::default(),
            sort_field: UsageCallSortField::OccurredAt,
            sort_direction: UsageCallSortDirection::Desc,
            cursor: None,
        }
    }
}

/// 描述调用页单条可展示项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCallItem {
    /// 内容无关的稳定调用 ID。
    pub id: String,
    /// 本条调用所属的具体物理 Agent；联合视图不得返回 `All` 伪客户端。
    pub client: SourceClientKind,
    /// 调用发生时的时间戳。
    pub occurred_at_epoch_ms: i64,
    /// 项目展示标签。
    pub project_label: String,
    /// 项目标签语义。
    pub project_label_code: DisplayLabelCode,
    /// 线程展示标签。
    pub thread_label: String,
    /// 线程标签语义。
    pub thread_label_code: DisplayLabelCode,
    /// 模型显示标签。
    pub model_label: String,
    /// 模型标签语义。
    pub model_label_code: DisplayLabelCode,
    /// 推理强度展示标签。
    pub reasoning_effort_label: String,
    /// 推理强度标签语义。
    pub reasoning_effort_label_code: DisplayLabelCode,
    /// 与 Token fact 相同口径的未缓存输入，避免前后端算法漂移。
    pub uncached_input_tokens: Option<u64>,
    /// 本条调用的完整 Token 事实。
    pub usage: crate::TokenUsage,
    /// 调用置信度。
    pub confidence: Confidence,
    /// 本条调用已生成的只读聚合事实。
    pub fact: MetricFact<crate::TokenUsage>,
}

/// 保存联合调用中每条 canonical 事实原本所属的物理 Agent 与 provider。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsageCallOrigin {
    /// 原始物理 Agent。
    pub(crate) client: SourceClientKind,
    /// 原始本机 provider。
    pub(crate) provider: ProviderKind,
    /// 原始 parser 来源版本。
    pub(crate) source_version: Option<String>,
}

/// 描述调用筛选选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageFilterOption {
    /// 可见筛选 ID（内部不含路径）。
    pub id: String,
    /// 可见标签。
    pub label: String,
    /// 标签语义。
    pub label_code: DisplayLabelCode,
    /// 同名标签的本地消歧。
    pub disambiguation_index: Option<usize>,
}

/// 描述可用于固定字段组合的筛选枚举。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageAvailableFilters {
    /// 模型筛选。
    pub models: Vec<UsageFilterOption>,
    /// 推理强度筛选。
    pub reasoning_efforts: Vec<UsageFilterOption>,
    /// 项目筛选。
    pub projects: Vec<UsageFilterOption>,
    /// 线程筛选。
    pub threads: Vec<UsageFilterOption>,
    /// 数据根筛选。
    pub roots: Vec<UsageFilterOption>,
}

/// 一页调用视图模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCallsPage {
    /// 与调用集合来源一致的索引状态。
    pub index_state: LocalIndexState,
    /// 统一观测时刻。
    pub observed_at_epoch_ms: i64,
    /// 页面项。
    pub items: Vec<UsageCallItem>,
    /// 由页面启用集合生成的筛选列表。
    pub available_filters: UsageAvailableFilters,
    /// 总命中条数。
    pub total_count: u64,
    /// 下一页游标。
    pub next_cursor: Option<String>,
}

/// 把固定窗口、筛选、排序与稳定游标组装为可直接返回前端的调用页。
#[allow(clippy::too_many_arguments)]
pub fn build_usage_calls_page(
    canonical: &[UsageCall],
    index_state: LocalIndexState,
    root_aliases: &BTreeMap<String, String>,
    query: &UsageCallsQuery,
    observed_at_epoch_ms: i64,
    client: SourceClientKind,
    provider: ProviderKind,
    source_version: Option<&str>,
) -> Result<UsageCallsPage, String> {
    let source_version = source_version.map(str::to_string);
    let origins = canonical
        .iter()
        .map(|call| {
            (
                call.logical_call_id.clone(),
                UsageCallOrigin {
                    client,
                    provider,
                    source_version: source_version.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let snapshot_context = vec![
        agent_wire_label(client).to_owned(),
        provider_fingerprint_label(provider).to_owned(),
        source_version.unwrap_or_else(|| "unknown-source-version".to_owned()),
    ];
    build_usage_calls_page_with_origins(
        canonical,
        index_state,
        root_aliases,
        query,
        observed_at_epoch_ms,
        &origins,
        &snapshot_context,
    )
}

/// 以逐调用来源装配联合调用页；调用集合必须已经按 Agent 命名空间化。
pub(crate) fn build_usage_calls_page_with_origins(
    canonical: &[UsageCall],
    index_state: LocalIndexState,
    root_aliases: &BTreeMap<String, String>,
    query: &UsageCallsQuery,
    observed_at_epoch_ms: i64,
    origins: &BTreeMap<String, UsageCallOrigin>,
    snapshot_context: &[String],
) -> Result<UsageCallsPage, String> {
    validate_filters(&query.filters)?;
    let accounting = TotalTokenAccounting::Observed;

    let mut rows = build_rows(canonical);
    let available_filters = collect_available_filters(&rows, root_aliases);
    let snapshot_fingerprint = cursor::fingerprint_snapshot_context(
        fingerprint_snapshot(&rows, index_state, &available_filters, accounting),
        snapshot_context,
    );
    let query_fingerprint = fingerprint_query(query);

    rows.retain(|row| matches_filters(row, &query.filters));
    rows.sort_by(|left, right| compare_rows(left, right, query, accounting));

    let total_count =
        u64::try_from(rows.len()).map_err(|_| "调用结果数量超出本机可表示范围。".to_owned())?;
    let start = cursor_start(
        query.cursor.as_deref(),
        query_fingerprint,
        snapshot_fingerprint,
        &rows,
    )?;
    let end = start.saturating_add(USAGE_CALL_PAGE_SIZE).min(rows.len());
    let items = rows[start..end]
        .iter()
        .map(|row| {
            let origin = origins
                .get(&row.call.logical_call_id)
                .ok_or_else(|| "联合调用缺少所属 Agent；请重新读取本机索引。".to_owned())?;
            Ok(UsageCallItem {
                id: row.call.logical_call_id.clone(),
                client: origin.client,
                occurred_at_epoch_ms: row.call.occurred_at_epoch_ms,
                project_label: row.project_label.clone(),
                project_label_code: row.project_label_code,
                thread_label: row.thread_label.clone(),
                thread_label_code: row.thread_label_code,
                model_label: row.model_label.clone(),
                model_label_code: row.model_label_code,
                reasoning_effort_label: row.reasoning_effort_label.clone(),
                reasoning_effort_label_code: row.reasoning_effort_label_code,
                uncached_input_tokens: row.call.usage.uncached_input_tokens(),
                usage: accounted_display_usage(&row.call.usage, accounting),
                confidence: row.call.confidence,
                fact: MetricFact::new(
                    accounted_display_usage(&row.call.usage, accounting),
                    origin.provider,
                    MetricScope::ThreadObserved,
                    observed_at_epoch_ms,
                    Freshness::Fresh,
                    Completeness::Complete,
                    row.call.confidence,
                    origin.source_version.clone(),
                ),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let next_cursor = if end < rows.len() {
        rows.get(end.saturating_sub(1)).map(|row| {
            cursor::encode_cursor(
                query_fingerprint,
                snapshot_fingerprint,
                &row.call.logical_call_id,
            )
        })
    } else {
        None
    };

    Ok(UsageCallsPage {
        index_state,
        observed_at_epoch_ms,
        items,
        available_filters,
        total_count,
        next_cursor,
    })
}

/// 返回游标必须绑定的稳定 provider 线标。
fn provider_fingerprint_label(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::RolloutJsonl => "rolloutJsonl",
        ProviderKind::ClaudeTranscriptJsonl => "claudeTranscriptJsonl",
        ProviderKind::GrokSessionJsonl => "grokSessionJsonl",
        ProviderKind::CombinedLocalAgents => "combinedLocalAgents",
        ProviderKind::WorkbuddyProjectJsonl => "workbuddyProjectJsonl",
    }
}
