//! 用量查询命令：概览、本机调用与统计。

use std::sync::Arc;

use loki_metis_core::{
    TimeStandard, UsageDimension, local_usage_overview_unavailable_message,
    workbuddy_combined_usage_unavailable_message,
};
use tauri::State;

use crate::calls_view::load_usage_calls_for_parser;
use crate::chart_view::load_usage_chart_for_parser;
use crate::combined_view::{load_combined_calls, load_combined_chart, load_combined_overview};
use crate::commands::ensure_business_access;
use crate::dto::{
    AgentClientKindDto, TimeStandardDto, UiMessageCodeDto, UsageCallsPageDto, UsageCallsQueryDto,
    UsageChartDimensionDto, UsageChartDto, UsageOverviewDto, UsageStatisticsDto, UsageViewKindDto,
    UsageWindow,
};
use crate::local_view::empty_local_windows_for_parser;
use crate::local_view::load_local_windows_for_parser;
use crate::runtime::{AppRuntimeState, now_epoch_ms};
use crate::statistics_view::load_usage_statistics_for_parser;

/// 校验业务访问权限并解析出观测时刻、时间标准与目标物理客户端（`None` 表示合并视图）。
async fn resolve_view(
    state: &AppRuntimeState,
    client: UsageViewKindDto,
    time_standard: TimeStandardDto,
) -> Result<(i64, TimeStandard, Option<AgentClientKindDto>), String> {
    ensure_business_access(state).await?;
    let observed_at_epoch_ms = now_epoch_ms();
    let time_standard =
        time_standard.into_time_standard(&loki_metis_core::device_time_zone_name());
    Ok((observed_at_epoch_ms, time_standard, client.local_client()))
}

/// 读取本机窗口。
#[tauri::command]
pub(crate) async fn get_usage_overview(
    state: State<'_, AppRuntimeState>,
    client: UsageViewKindDto,
    time_standard: TimeStandardDto,
) -> Result<UsageOverviewDto, String> {
    get_usage_overview_for_state(&state, client, time_standard).await
}

/// 承载概览 command 的可测试执行路径，避免测试构造真实 Tauri 窗口状态。
pub(super) async fn get_usage_overview_for_state(
    state: &AppRuntimeState,
    client: UsageViewKindDto,
    time_standard: TimeStandardDto,
) -> Result<UsageOverviewDto, String> {
    let (observed_at_epoch_ms, time_standard, local_client) =
        resolve_view(state, client, time_standard).await?;
    if client == UsageViewKindDto::Workbuddy {
        return Ok(UsageOverviewDto {
            product_definition_required: false,
            implementation_message: None,
            implementation_message_code: None,
            local_records: Some(
                crate::combined_view::load_workbuddy_overview(
                    state,
                    observed_at_epoch_ms,
                    time_standard,
                )
                .await?,
            ),
        });
    }
    let Some(local_client) = local_client else {
        let combined = load_combined_overview(state, observed_at_epoch_ms, time_standard).await?;
        return Ok(UsageOverviewDto {
            product_definition_required: false,
            implementation_message: combined
                .workbuddy_read_failed
                .then(|| workbuddy_combined_usage_unavailable_message().to_owned()),
            implementation_message_code: combined
                .workbuddy_read_failed
                .then_some(UiMessageCodeDto::OverviewWorkbuddyUnavailable),
            local_records: Some(combined.local_records),
        });
    };
    let coverage_state = Arc::clone(state.coverages.get(local_client.into()));
    let coverage = coverage_state.read().await.clone();
    let binding = Arc::clone(&state.agent_clients.get(local_client.into()).local_analysis);
    let (provider, source_label) = binding.identity();
    let local_result = load_local_windows_for_parser(
        binding.app_data_dir(),
        &coverage,
        observed_at_epoch_ms,
        binding.parser_version(),
        provider,
        source_label.clone(),
        time_standard,
    )
    .await;
    let (local_records, implementation_message, implementation_message_code) = match local_result {
        Ok(records) => (records, None, None),
        Err(_) => {
            let coverage = coverage_state.read().await.clone();
            let failed_coverage = loki_metis_core::CoverageReport {
                state: loki_metis_core::CoverageState::Failed,
                ..coverage
            };
            *coverage_state.write().await = failed_coverage.clone();
            (
                empty_local_windows_for_parser(
                    &failed_coverage,
                    observed_at_epoch_ms,
                    provider,
                    source_label,
                ),
                Some(local_usage_overview_unavailable_message().to_owned()),
                Some(UiMessageCodeDto::OverviewLocalIndexUnavailable),
            )
        }
    };

    Ok(UsageOverviewDto {
        product_definition_required: false,
        implementation_message,
        implementation_message_code,
        local_records: Some(local_records),
    })
}

/// 按固定筛选、排序与稳定游标读取一页本机调用。
#[tauri::command]
pub(crate) async fn get_usage_calls(
    state: State<'_, AppRuntimeState>,
    client: UsageViewKindDto,
    query: UsageCallsQueryDto,
    time_standard: TimeStandardDto,
) -> Result<UsageCallsPageDto, String> {
    get_usage_calls_for_state(&state, client, &query, time_standard).await
}

/// 承载调用 command 的可测试执行路径，避免测试构造真实 Tauri 窗口状态。
pub(super) async fn get_usage_calls_for_state(
    state: &AppRuntimeState,
    client: UsageViewKindDto,
    query: &UsageCallsQueryDto,
    time_standard: TimeStandardDto,
) -> Result<UsageCallsPageDto, String> {
    let (observed_at_epoch_ms, time_standard, local_client) =
        resolve_view(state, client, time_standard).await?;
    if client == UsageViewKindDto::Workbuddy {
        return Err("WorkBuddy 视图不提供调用列表。".to_owned());
    }
    let Some(client) = local_client else {
        return load_combined_calls(state, query, observed_at_epoch_ms, time_standard).await;
    };
    let binding = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    let (provider, source_label) = binding.identity();
    load_usage_calls_for_parser(
        binding.app_data_dir(),
        query,
        observed_at_epoch_ms,
        binding.parser_version(),
        client.into(),
        provider,
        source_label.as_str(),
        time_standard,
    )
    .await
}

/// 按固定窗口与固定维度读取有界、可对账的本机统计，不隐式启动扫描。
#[tauri::command]
pub(crate) async fn get_usage_statistics(
    state: State<'_, AppRuntimeState>,
    client: AgentClientKindDto,
    window: UsageWindow,
    dimension: UsageDimension,
    time_standard: TimeStandardDto,
) -> Result<UsageStatisticsDto, String> {
    ensure_business_access(&state).await?;
    let binding = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    let coverage = state.coverages.get(client.into()).read().await.clone();
    let (provider, source_label) = binding.identity();
    let time_standard =
        time_standard.into_time_standard(&loki_metis_core::device_time_zone_name());
    load_usage_statistics_for_parser(
        binding.app_data_dir(),
        &coverage,
        window,
        dimension,
        now_epoch_ms(),
        binding.parser_version(),
        provider,
        source_label.as_str(),
        time_standard,
    )
    .await
}

/// 按固定窗口与维度读取可对账的图表时间桶和分布，不隐式启动扫描。
#[tauri::command]
pub(crate) async fn get_usage_charts(
    state: State<'_, AppRuntimeState>,
    client: UsageViewKindDto,
    window: UsageWindow,
    dimension: UsageChartDimensionDto,
    time_standard: TimeStandardDto,
) -> Result<UsageChartDto, String> {
    get_usage_charts_for_state(&state, client, window, dimension, time_standard).await
}

/// 承载图表 command 的可测试执行路径，避免测试构造真实 Tauri 窗口状态。
pub(super) async fn get_usage_charts_for_state(
    state: &AppRuntimeState,
    client: UsageViewKindDto,
    window: UsageWindow,
    dimension: UsageChartDimensionDto,
    time_standard: TimeStandardDto,
) -> Result<UsageChartDto, String> {
    let (observed_at_epoch_ms, time_standard, local_client) =
        resolve_view(state, client, time_standard).await?;
    if client == UsageViewKindDto::Workbuddy {
        return Err("WorkBuddy 图表请使用本机统计快照。".to_owned());
    }
    let Some(client) = local_client else {
        return load_combined_chart(
            state,
            window,
            dimension,
            observed_at_epoch_ms,
            time_standard,
        )
        .await;
    };
    let binding = Arc::clone(&state.agent_clients.get(client.into()).local_analysis);
    let coverage = state.coverages.get(client.into()).read().await.clone();
    let (provider, source_label) = binding.identity();
    load_usage_chart_for_parser(
        binding.app_data_dir(),
        &coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        binding.parser_version(),
        provider,
        source_label.as_str(),
        time_standard,
    )
    .await
}


