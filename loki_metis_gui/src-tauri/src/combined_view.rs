//! 从物理 Agent 独立索引与可选 WorkBuddy project JSONL 快照装配联合视图。

use std::sync::Arc;

use loki_metis_core::{
    AgentUsageSnapshot, CoverageReport, ProviderKind, SourceClientKind, TimeStandard,
    UsageViewError, UsageViewKind, WorkbuddyStatisticsSnapshot,
    build_combined_local_windows_with_standard, build_combined_usage_calls_page,
    build_combined_usage_chart_with_standard, build_workbuddy_only_local_windows,
    combine_agent_usage_snapshots, mark_workbuddy_unavailable_in_local_windows,
    merge_workbuddy_into_local_windows, resolve_usage_view_members,
};
use tauri::async_runtime::spawn_blocking;

use crate::agent_client::analysis::LocalIndexBinding;
use crate::calls_view::{to_core_query, to_dto_page};
use crate::chart_view::to_dto_chart;
use crate::dto::{
    LocalRecordsSectionDto, UsageCallsPageDto, UsageCallsQueryDto, UsageChartDimensionDto,
    UsageChartDto, UsageWindow,
};
use crate::local_view::{local_read_error, map_local_windows, open_recent_usage_snapshot};
use crate::runtime::AppRuntimeState;
use crate::statistics_view::to_core_window;

/// 「全部」概览的本机事实，以及可选 WorkBuddy 当次是否读取失败。
pub(crate) struct CombinedOverviewResult {
    pub(crate) local_records: LocalRecordsSectionDto,
    pub(crate) workbuddy_read_failed: bool,
}

/// 区分未安装来源、完整快照与已存在来源的读取失败，避免 `Option` 静默吞掉失败。
enum WorkbuddySnapshotOutcome {
    Disabled,
    Absent,
    Available(Box<WorkbuddyStatisticsSnapshot>),
    ReadFailed,
}

/// 读取全部已开启 Agent 的单库一致快照和可选 WorkBuddy 快照，装配联合概览。
pub(crate) async fn load_combined_overview(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<CombinedOverviewResult, String> {
    let outcome =
        maybe_workbuddy_snapshot(state, observed_at_epoch_ms, time_standard.clone(), false).await;
    let (workbuddy, workbuddy_read_failed, workbuddy_source_absent) = match outcome {
        WorkbuddySnapshotOutcome::Disabled => (None, false, false),
        WorkbuddySnapshotOutcome::Absent => (None, false, true),
        WorkbuddySnapshotOutcome::Available(snapshot) => (Some(snapshot), false, false),
        WorkbuddySnapshotOutcome::ReadFailed => (None, true, false),
    };
    let members = resolve_usage_view_members(UsageViewKind::All, state.enabled_agents().await);
    if matches!(members, Err(UsageViewError::NoEnabledAgents)) {
        if workbuddy_read_failed {
            return Err(local_read_error());
        }
        if workbuddy_source_absent {
            return Err("未找到 WorkBuddy 本机 project JSONL 用量数据。".to_owned());
        }
        let Some(snapshot) = workbuddy else {
            return Err(usage_view_error_message(UsageViewError::NoEnabledAgents));
        };
        return Ok(CombinedOverviewResult {
            local_records: workbuddy_only_records(
                &snapshot.windows,
                &snapshot.coverage,
                observed_at_epoch_ms,
                ProviderKind::CombinedLocalAgents,
            ),
            workbuddy_read_failed: false,
        });
    }
    let inputs =
        load_enabled_agent_snapshots(state, observed_at_epoch_ms, time_standard.clone()).await?;
    let combined = spawn_blocking(move || combine_agent_usage_snapshots(inputs))
        .await
        .map_err(|_| local_read_error())?;
    let combined = match combined {
        Ok(combined) => combined,
        Err(UsageViewError::NoConfiguredDataSources) => {
            if workbuddy_read_failed {
                return Err(local_read_error());
            }
            if workbuddy_source_absent {
                return Err("未找到 WorkBuddy 本机 project JSONL 用量数据。".to_owned());
            }
            let Some(snapshot) = workbuddy else {
                return Err(usage_view_error_message(
                    UsageViewError::NoConfiguredDataSources,
                ));
            };
            return Ok(CombinedOverviewResult {
                local_records: workbuddy_only_records(
                    &snapshot.windows,
                    &snapshot.coverage,
                    observed_at_epoch_ms,
                    ProviderKind::CombinedLocalAgents,
                ),
                workbuddy_read_failed: false,
            });
        }
        Err(error) => return Err(usage_view_error_message(error)),
    };
    let summary = spawn_blocking(move || {
        let mut summary = build_combined_local_windows_with_standard(
            &combined,
            observed_at_epoch_ms,
            time_standard,
            &jiff::tz::TimeZone::system(),
        )
        .map_err(|_| ())?;
        if let Some(snapshot) = workbuddy {
            summary =
                merge_workbuddy_into_local_windows(summary, &snapshot.windows, &snapshot.coverage);
        } else if workbuddy_read_failed {
            summary = mark_workbuddy_unavailable_in_local_windows(summary);
        }
        Ok::<_, ()>(summary)
    })
    .await
    .map_err(|_| local_read_error())?
    .map_err(|_| local_read_error())?;
    Ok(CombinedOverviewResult {
        local_records: map_local_windows(summary),
        workbuddy_read_failed,
    })
}

/// 读取 WorkBuddy project JSONL；仅独立概览按需附加 Trace 诊断。
/// 未安装时省略，读取失败不得静默少算。
async fn maybe_workbuddy_snapshot(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
    include_trace_diagnostics: bool,
) -> WorkbuddySnapshotOutcome {
    if !state.workbuddy_stats_enabled().await {
        return WorkbuddySnapshotOutcome::Disabled;
    }
    let Some(workbuddy_home) = crate::backend::workbuddy::resolve_workbuddy_home() else {
        return WorkbuddySnapshotOutcome::Absent;
    };
    let snapshot = if include_trace_diagnostics {
        crate::backend::workbuddy::read_workbuddy_statistics(
            &workbuddy_home,
            observed_at_epoch_ms,
            time_standard,
        )
        .await
    } else {
        crate::backend::workbuddy::read_workbuddy_usage_snapshot(
            &workbuddy_home,
            observed_at_epoch_ms,
            time_standard,
        )
        .await
    };
    workbuddy_snapshot_outcome(snapshot)
}

/// 把 adapter 读取结果映射为显式装配状态；只有批准布局缺失可以省略。
fn workbuddy_snapshot_outcome(
    snapshot: Result<WorkbuddyStatisticsSnapshot, crate::backend::workbuddy::WorkbuddyReadError>,
) -> WorkbuddySnapshotOutcome {
    match snapshot {
        Ok(snapshot) => WorkbuddySnapshotOutcome::Available(Box::new(snapshot)),
        Err(crate::backend::workbuddy::WorkbuddyReadError::SourceUnavailable) => {
            WorkbuddySnapshotOutcome::Absent
        }
        Err(crate::backend::workbuddy::WorkbuddyReadError::Read) => {
            WorkbuddySnapshotOutcome::ReadFailed
        }
    }
}

/// 把 WorkBuddy 快照映射为与本机概览相同的六个窗口 DTO，供 WorkBuddy 选项卡使用。
pub(crate) async fn load_workbuddy_overview(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<LocalRecordsSectionDto, String> {
    if !state.workbuddy_stats_enabled().await {
        return Err("WorkBuddy 本地统计尚未开启；请先在设置中打开对应开关。".to_owned());
    }
    let snapshot =
        match maybe_workbuddy_snapshot(state, observed_at_epoch_ms, time_standard, true).await {
            WorkbuddySnapshotOutcome::Available(snapshot) => snapshot,
            WorkbuddySnapshotOutcome::Absent | WorkbuddySnapshotOutcome::Disabled => {
                return Err("未找到 WorkBuddy 本机 project JSONL 用量数据。".to_owned());
            }
            WorkbuddySnapshotOutcome::ReadFailed => return Err(local_read_error()),
        };
    Ok(workbuddy_only_records(
        &snapshot.windows,
        &snapshot.coverage,
        observed_at_epoch_ms,
        ProviderKind::WorkbuddyProjectJsonl,
    ))
}

/// 只有 WorkBuddy 时的窗口区；provider 决定这批数字以哪种身份对外呈现
/// （「全部」用联合视图身份，WorkBuddy 独立概览保留 project JSONL 身份）。
fn workbuddy_only_records(
    windows: &[loki_metis_core::WorkbuddyWindowAggregate],
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
) -> LocalRecordsSectionDto {
    map_local_windows(build_workbuddy_only_local_windows(
        windows,
        coverage,
        observed_at_epoch_ms,
        provider,
    ))
}

/// 读取全部已开启物理 Agent 并生成调用页；WorkBuddy 按产品边界不进入调用明细。
pub(crate) async fn load_combined_calls(
    state: &AppRuntimeState,
    query: &UsageCallsQueryDto,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<UsageCallsPageDto, String> {
    let inputs = load_enabled_agent_snapshots(state, observed_at_epoch_ms, time_standard).await?;
    let query = to_core_query(query);
    let page = spawn_blocking(move || {
        let combined = combine_agent_usage_snapshots(inputs).map_err(usage_view_error_message)?;
        build_combined_usage_calls_page(&combined, &query, observed_at_epoch_ms)
            .map_err(|_| local_read_error())
    })
    .await
    .map_err(|_| local_read_error())??;
    Ok(to_dto_page(page))
}

/// 读取全部已开启 Agent 的独立快照，并在完整联合集合上生成图表。
pub(crate) async fn load_combined_chart(
    state: &AppRuntimeState,
    window: UsageWindow,
    dimension: UsageChartDimensionDto,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<UsageChartDto, String> {
    let inputs =
        load_enabled_agent_snapshots(state, observed_at_epoch_ms, time_standard.clone()).await?;
    let page = spawn_blocking(move || {
        let combined = combine_agent_usage_snapshots(inputs).map_err(usage_view_error_message)?;
        build_combined_usage_chart_with_standard(
            &combined,
            to_core_window(window),
            dimension.into(),
            observed_at_epoch_ms,
            time_standard,
            &jiff::tz::TimeZone::system(),
        )
        .map_err(|_| local_read_error())
    })
    .await
    .map_err(|_| local_read_error())??;
    Ok(to_dto_chart(page))
}

/// 以一个共享观测时刻并发读取各库快照；每个物理 Agent 拥有独立 SQLite 数据库，
/// 互不阻塞，任一库失败即放弃整个联合响应。
async fn load_enabled_agent_snapshots(
    state: &AppRuntimeState,
    observed_at_epoch_ms: i64,
    time_standard: TimeStandard,
) -> Result<Vec<AgentUsageSnapshot>, String> {
    let members = resolve_usage_view_members(UsageViewKind::All, state.enabled_agents().await)
        .map_err(usage_view_error_message)?;
    let mut reads = tokio::task::JoinSet::new();
    for client in members {
        let binding = Arc::clone(&state.agent_clients.get(client).local_analysis);
        let coverage = state.coverages.get(client).read().await.clone();
        let time_standard = time_standard.clone();
        reads.spawn(async move {
            load_agent_snapshot(
                client,
                binding,
                coverage,
                observed_at_epoch_ms,
                &time_standard,
            )
            .await
        });
    }
    let mut inputs = Vec::with_capacity(reads.len());
    while let Some(result) = reads.join_next().await {
        inputs.push(result.map_err(|_| local_read_error())??);
    }
    Ok(inputs)
}

/// 从一个物理 Agent 的绑定取得 snapshot、覆盖与原始 parser 来源标签。
async fn load_agent_snapshot(
    client: SourceClientKind,
    binding: Arc<LocalIndexBinding>,
    coverage: CoverageReport,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
) -> Result<AgentUsageSnapshot, String> {
    let (_, source_version) = binding.identity();
    let snapshot = open_recent_usage_snapshot(
        binding.app_data_dir(),
        binding.parser_version(),
        observed_at_epoch_ms,
        time_standard,
    )
    .await?;
    Ok(AgentUsageSnapshot::new(
        client,
        snapshot,
        coverage,
        Some(source_version),
    ))
}

/// 把联合视图成员错误映射为不含磁盘细节的稳定提示。
fn usage_view_error_message(error: UsageViewError) -> String {
    match error {
        UsageViewError::NoEnabledAgents => "请先在设置中开启至少一个 AI Agent。".to_owned(),
        UsageViewError::AgentDisabled => "当前 AI Agent 尚未开启。".to_owned(),
        UsageViewError::NoConfiguredDataSources => {
            "请先为至少一个已开启 AI Agent 配置数据源。".to_owned()
        }
        UsageViewError::DuplicateAgent => local_read_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 相同 WorkBuddy 窗口在独立页和「全部」页必须携带各自正确的 provider。
    #[test]
    fn workbuddy_overview_provider_depends_on_view_context() {
        let coverage = loki_metis_core::complete_workbuddy_coverage();
        let all = workbuddy_only_records(&[], &coverage, 1, ProviderKind::CombinedLocalAgents);
        let standalone =
            workbuddy_only_records(&[], &coverage, 1, ProviderKind::WorkbuddyProjectJsonl);

        assert!(
            all.windows
                .iter()
                .all(|window| window.fact.provider == ProviderKind::CombinedLocalAgents)
        );
        assert!(
            standalone
                .windows
                .iter()
                .all(|window| { window.fact.provider == ProviderKind::WorkbuddyProjectJsonl })
        );
    }

    /// 缺失批准布局可以省略，已存在来源的读取失败必须保持显式失败状态。
    #[test]
    fn workbuddy_read_errors_keep_absence_distinct_from_failure() {
        assert!(matches!(
            workbuddy_snapshot_outcome(Err(
                crate::backend::workbuddy::WorkbuddyReadError::SourceUnavailable
            )),
            WorkbuddySnapshotOutcome::Absent
        ));
        assert!(matches!(
            workbuddy_snapshot_outcome(Err(crate::backend::workbuddy::WorkbuddyReadError::Read)),
            WorkbuddySnapshotOutcome::ReadFailed
        ));
    }
}
