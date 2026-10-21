//! 已开启 Agent 的跨独立 SQLite 只读联合视图纯业务装配。

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::calls_view::{CallOrigins, UsageCallOrigin, build_usage_calls_page_with_origins};
use crate::{
    CanonicalUsageSet, CanonicalizationWarning, CoverageReport, CoverageState,
    LocalIndexState, LocalRecordsSummary, ProviderKind, SessionTokenSnapshot, SourceClientKind,
    SourceProvenance, TimeStandard, TokenUsage, TotalTokenAccounting, UsageCall, UsageCallsPage,
    UsageCallsQuery, UsageSnapshot, agent_wire_label, build_local_windows_with_standard, stable_id,
};

/// 解析或装配联合用量视图时的稳定业务错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UsageViewError {
    /// 「全部」没有任何已开启成员，不能伪装成可信空用量。
    #[error("all usage view has no enabled agents")]
    NoEnabledAgents,
    /// 请求的具体 Agent 尚未由用户开启。
    #[error("requested usage agent is disabled")]
    AgentDisabled,
    /// 「全部」中已开启的 Agent 都没有启用的数据源，不能生成联合用量。
    #[error("all usage view has no configured data sources")]
    NoConfiguredDataSources,
    /// 同一物理 Agent 被重复装入联合快照。
    #[error("combined usage view contains a duplicate agent")]
    DuplicateAgent,
}

/// 保存 adapter 从单个物理 Agent 独占 SQLite 取得的一致快照及质量事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentUsageSnapshot {
    /// 该快照所属的物理 Agent，同一联合视图内不得重复。
    client: SourceClientKind,
    /// 调用方在单个数据库事务内取得的一致快照。
    snapshot: UsageSnapshot,
    /// 已与快照自身质量合并后的有效覆盖报告。
    coverage: CoverageReport,
    /// 该 Agent 的 parser 版本；缺失时在联合版本中记为 unknown。
    source_version: Option<String>,
}

impl AgentUsageSnapshot {
    /// 创建一个单 Agent 只读输入；调用方负责在一个数据库事务中取得 `snapshot`。
    pub fn new(
        client: SourceClientKind,
        snapshot: UsageSnapshot,
        coverage: CoverageReport,
        source_version: Option<String>,
    ) -> Self {
        let coverage = snapshot.effective_coverage(coverage);
        Self {
            client,
            snapshot,
            coverage,
            source_version,
        }
    }
}

/// 保存已经命名空间化的跨 Agent canonical 集合及逐调用原始来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedUsageSnapshot {
    /// 跨 Agent 合并的 canonical 集合；调用 ID 已加 `all-<agent>-call-` 命名空间，跨 Agent 不去重。
    canonical: CanonicalUsageSet,
    /// 由各 Agent 索引状态与合并结果是否为空归约出的联合索引状态。
    index_state: LocalIndexState,
    /// 合并后的覆盖报告。
    coverage: CoverageReport,
    /// 命名空间化数据根 ID 到展示别名的映射。
    root_aliases: BTreeMap<String, String>,
    /// 按调用 ID 命名空间前缀还原每条调用所属的 Agent 与来源版本。
    origins: CallOrigins,
    /// 联合来源版本，形如 `combined-local-agents-v1[codex=…,…]`，任一成员版本变化都会改变它。
    source_version: String,
}

/// 合并多个物理 Agent 的单库一致快照；没有启用数据源的 Agent 不参与联合状态，
/// 跨 Agent 调用绝不相互去重。
pub fn combine_agent_usage_snapshots(
    mut inputs: Vec<AgentUsageSnapshot>,
) -> Result<CombinedUsageSnapshot, UsageViewError> {
    if inputs.is_empty() {
        return Err(UsageViewError::NoEnabledAgents);
    }
    inputs.retain(|input| input.snapshot.roots.iter().any(|root| root.enabled));
    if inputs.is_empty() {
        return Err(UsageViewError::NoConfiguredDataSources);
    }

    let source_version = combined_source_version(&inputs);
    let coverage = combine_coverage(&inputs);
    let empty_tokens = combined_empty_tokens(&inputs);
    let mut seen_clients = BTreeSet::new();
    let mut child_states = Vec::with_capacity(inputs.len());
    let mut canonical = CanonicalUsageSet {
        total_token_accounting: TotalTokenAccounting::Observed,
        empty_tokens,
        calls: Vec::new(),
        duplicate_source_count: 0,
        duplicate_counts_by_call: BTreeMap::new(),
        warnings: Vec::new(),
        snapshots: Vec::new(),
    };
    let mut root_aliases = BTreeMap::new();
    let mut origins = Vec::new();

    for input in inputs {
        let client_key = agent_wire_label(input.client);
        if !seen_clients.insert(client_key) {
            return Err(UsageViewError::DuplicateAgent);
        }
        child_states.push(input.snapshot.index_state);
        canonical.duplicate_source_count = canonical
            .duplicate_source_count
            .saturating_add(input.snapshot.canonical.duplicate_source_count);

        for root in &input.snapshot.roots {
            root_aliases.insert(
                namespaced_id(input.client, "root", &root.root_id),
                root.alias.clone(),
            );
        }
        for (logical_call_id, count) in &input.snapshot.canonical.duplicate_counts_by_call {
            canonical
                .duplicate_counts_by_call
                .insert(namespaced_id(input.client, "call", logical_call_id), *count);
        }
        canonical.warnings.extend(
            input
                .snapshot
                .canonical
                .warnings
                .iter()
                .map(|warning| namespace_warning(input.client, warning)),
        );
        canonical.snapshots.extend(
            input
                .snapshot
                .canonical
                .snapshots
                .iter()
                .map(|snapshot| namespace_session_snapshot(input.client, snapshot)),
        );

        let provider = provider_for_client(input.client);
        // 命名空间前缀即所属 Agent，来源只按 Agent 保存一份，不再逐调用复制。
        origins.push((
            format!("all-{client_key}-call-"),
            UsageCallOrigin {
                client: input.client,
                provider,
                source_version: input.source_version.clone(),
            },
        ));
        for call in input.snapshot.canonical.calls {
            canonical.calls.push(namespace_call(input.client, call));
        }
    }

    canonical
        .calls
        .sort_by(|left, right| left.logical_call_id.cmp(&right.logical_call_id));
    canonical.warnings.sort_by(|left, right| {
        left.logical_call_id
            .cmp(&right.logical_call_id)
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    canonical.snapshots.sort_by(|left, right| {
        left.thread_key
            .cmp(&right.thread_key)
            .then_with(|| left.occurred_at_epoch_ms.cmp(&right.occurred_at_epoch_ms))
            .then_with(|| left.logical_call_id.cmp(&right.logical_call_id))
    });

    let index_state = combined_index_state(&child_states, canonical.calls.is_empty());
    if matches!(
        index_state,
        LocalIndexState::NeedsRescan | LocalIndexState::NotScanned
    ) {
        clear_partial_usage(&mut canonical, &mut origins);
    }

    Ok(CombinedUsageSnapshot {
        canonical,
        index_state,
        coverage,
        root_aliases,
        origins: CallOrigins::ByNamespace(origins),
        source_version,
    })
}

/// 按查看时间标准从联合快照构造六个概览窗口。
pub fn build_combined_local_windows_with_standard(
    combined: &CombinedUsageSnapshot,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<LocalRecordsSummary, String> {
    build_local_windows_with_standard(
        &combined.canonical,
        &combined.coverage,
        combined.index_state,
        observed_at_epoch_ms,
        ProviderKind::CombinedLocalAgents,
        Some(&combined.source_version),
        time_standard,
        device_tz,
    )
}

/// 在完整联合 canonical 集合上一次生成筛选、排序、分页与游标。
pub fn build_combined_usage_calls_page(
    combined: &CombinedUsageSnapshot,
    query: &UsageCallsQuery,
    observed_at_epoch_ms: i64,
) -> Result<UsageCallsPage, String> {
    build_usage_calls_page_with_origins(
        &combined.canonical.calls,
        combined.index_state,
        &combined.root_aliases,
        query,
        observed_at_epoch_ms,
        &combined.origins,
        std::slice::from_ref(&combined.source_version),
    )
}

/// 按查看时间标准从联合快照构造固定时间桶与多维图表。
pub fn build_combined_usage_chart_with_standard(
    combined: &CombinedUsageSnapshot,
    window: crate::LocalUsageWindow,
    dimension: crate::UsageChartDimension,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> Result<crate::UsageChartPage, String> {
    let client_of = |call_id: &str| combined.origins.get(call_id).map(|origin| origin.client);
    crate::chart_view::build_combined_chart_with_standard(
        &combined.canonical,
        &combined.root_aliases,
        &client_of,
        combined.index_state,
        &combined.coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        Some(&combined.source_version),
        time_standard,
        device_tz,
    )
}

/// 把物理 Agent 映射回其既有本机 provider，联合 provider 不进入单行事实。
const fn provider_for_client(client: SourceClientKind) -> ProviderKind {
    match client {
        SourceClientKind::Codex => ProviderKind::RolloutJsonl,
        SourceClientKind::ClaudeCode => ProviderKind::ClaudeTranscriptJsonl,
        SourceClientKind::Grok => ProviderKind::GrokSessionJsonl,
        SourceClientKind::WorkBuddy => ProviderKind::WorkBuddyProjectJsonl,
    }
}

/// 生成不包含路径且不会跨 Agent 碰撞的内部稳定 ID。
fn namespaced_id(client: SourceClientKind, kind: &str, value: &str) -> String {
    stable_id(&format!("all-{}-{kind}", agent_wire_label(client)), value)
}

/// 命名空间化调用、线程、项目、来源与根身份，保留展示标签和 Token 事实。
fn namespace_call(client: SourceClientKind, mut call: UsageCall) -> UsageCall {
    call.logical_call_id = namespaced_id(client, "call", &call.logical_call_id);
    call.thread_key = namespaced_id(client, "thread", &call.thread_key);
    call.project_key = call
        .project_key
        .as_deref()
        .map(|value| namespaced_id(client, "project", value));
    call.adapter_consistency_key = call
        .adapter_consistency_key
        .as_deref()
        .map(|value| namespaced_id(client, "consistency", value));
    call.provenance = call
        .provenance
        .into_iter()
        .map(|source| namespace_provenance(client, source))
        .collect();
    call
}

/// 命名空间化只读来源身份，不改变安全相对标签或归档状态。
fn namespace_provenance(
    client: SourceClientKind,
    mut source: SourceProvenance,
) -> SourceProvenance {
    source.source_id = namespaced_id(client, "source", &source.source_id);
    source.root_id = namespaced_id(client, "root", &source.root_id);
    source
}

/// 命名空间化 canonical 警告中的调用身份。
fn namespace_warning(
    client: SourceClientKind,
    warning: &CanonicalizationWarning,
) -> CanonicalizationWarning {
    CanonicalizationWarning {
        logical_call_id: namespaced_id(client, "call", &warning.logical_call_id),
        kind: warning.kind,
    }
}

/// 命名空间化历史兼容快照，使过滤复制也不会跨 Agent 混同。
fn namespace_session_snapshot(
    client: SourceClientKind,
    snapshot: &SessionTokenSnapshot,
) -> SessionTokenSnapshot {
    SessionTokenSnapshot {
        thread_key: namespaced_id(client, "thread", &snapshot.thread_key),
        occurred_at_epoch_ms: snapshot.occurred_at_epoch_ms,
        cumulative_total_tokens: snapshot.cumulative_total_tokens,
        logical_call_id: namespaced_id(client, "call", &snapshot.logical_call_id),
        model: snapshot.model.clone(),
        reasoning_effort: snapshot.reasoning_effort.clone(),
        project_key: snapshot
            .project_key
            .as_deref()
            .map(|value| namespaced_id(client, "project", value)),
        provenance: snapshot
            .provenance
            .iter()
            .cloned()
            .map(|source| namespace_provenance(client, source))
            .collect(),
    }
}

/// 联合空窗口仅在所有成员都提供某一分项时把该分项解释为可信零。
fn combined_empty_tokens(inputs: &[AgentUsageSnapshot]) -> TokenUsage {
    TokenUsage::zero_with_component_availability(
        inputs.iter().all(|input| {
            input
                .snapshot
                .canonical
                .empty_tokens
                .cached_input_tokens
                .is_some()
        }),
        inputs.iter().all(|input| {
            input
                .snapshot
                .canonical
                .empty_tokens
                .cache_write_input_tokens
                .is_some()
        }),
        inputs.iter().all(|input| {
            input
                .snapshot
                .canonical
                .empty_tokens
                .reasoning_output_tokens
                .is_some()
        }),
    )
}

/// 生成包含固定成员与原始 parser 标签的联合来源版本，供事实与游标审计。
fn combined_source_version(inputs: &[AgentUsageSnapshot]) -> String {
    let members = inputs
        .iter()
        .map(|input| {
            format!(
                "{}={}",
                agent_wire_label(input.client),
                input.source_version.as_deref().unwrap_or("unknown")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("combined-local-agents-v1[{members}]")
}

/// 合成覆盖质量和安全计数；任一失败或取消都不能被其余完整项掩盖。
fn combine_coverage(inputs: &[AgentUsageSnapshot]) -> CoverageReport {
    let state = if inputs
        .iter()
        .any(|input| input.coverage.state == CoverageState::Failed)
    {
        CoverageState::Failed
    } else if inputs
        .iter()
        .any(|input| input.coverage.state == CoverageState::Cancelled)
    {
        CoverageState::Cancelled
    } else if inputs
        .iter()
        .any(|input| input.coverage.state == CoverageState::Partial)
    {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };

    let sum_field = |field: fn(&CoverageReport) -> u64| -> u64 {
        inputs.iter().fold(0_u64, |total, input| {
            total.saturating_add(field(&input.coverage))
        })
    };

    CoverageReport {
        state,
        roots_scanned: sum_field(|coverage| coverage.roots_scanned),
        roots_discovered: sum_field(|coverage| coverage.roots_discovered),
        permission_denied_count: sum_field(|coverage| coverage.permission_denied_count),
        skipped_count: sum_field(|coverage| coverage.skipped_count),
        warning_count: sum_field(|coverage| coverage.warning_count),
    }
}

/// 按 NeedsRescan、NotScanned、Ready、ReadyNoCalls 的批准优先级合成状态。
fn combined_index_state(states: &[LocalIndexState], calls_empty: bool) -> LocalIndexState {
    if states.contains(&LocalIndexState::NeedsRescan) {
        LocalIndexState::NeedsRescan
    } else if states.contains(&LocalIndexState::NotScanned) {
        LocalIndexState::NotScanned
    } else if calls_empty {
        LocalIndexState::ReadyNoCalls
    } else {
        LocalIndexState::Ready
    }
}

/// 未扫描或待重扫时清除其余成员的部分事实，避免把部分和冒充「全部」。
fn clear_partial_usage(
    canonical: &mut CanonicalUsageSet,
    origins: &mut Vec<(String, UsageCallOrigin)>,
) {
    canonical.calls.clear();
    canonical.duplicate_source_count = 0;
    canonical.duplicate_counts_by_call.clear();
    canonical.warnings.clear();
    canonical.snapshots.clear();
    origins.clear();
}

#[cfg(test)]
#[path = "combined_view_tests.rs"]
mod tests;
