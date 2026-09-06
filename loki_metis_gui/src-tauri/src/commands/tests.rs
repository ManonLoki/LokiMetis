use super::usage::{get_usage_charts_for_state, get_usage_overview_for_state};
use crate::dto::{
    TimeStandardDto, UsageChartDimensionDto, UsageChartGranularityDto, UsageClientKindDto,
    UsageViewKindDto, UsageWindow,
};
use crate::runtime::AppRuntimeState;
use tempfile::tempdir;

/// 产品概览 IPC 必须按固定顺序返回六个日历窗口。
const EXPECTED_OVERVIEW_WINDOWS: [UsageWindow; 6] = [
    UsageWindow::Today,
    UsageWindow::Yesterday,
    UsageWindow::ThisWeek,
    UsageWindow::LastWeek,
    UsageWindow::ThisMonth,
    UsageWindow::LastMonth,
];

/// 无向导时已开启 Agent 也可读取概览，并装配六个窗口键。
#[tokio::test]
async fn overview_returns_six_calendar_windows_without_wizard() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());
    state
        .set_enabled_agents(&[crate::dto::UsageClientKindDto::Codex])
        .await
        .expect("codex can be enabled");

    let overview =
        get_usage_overview_for_state(&state, UsageViewKindDto::Codex, TimeStandardDto::default())
            .await
            .expect("overview is readable");
    let windows = overview.local_records.expect("local records exist").windows;
    let actual: Vec<UsageWindow> = windows.iter().map(|item| item.window).collect();
    assert_eq!(actual, EXPECTED_OVERVIEW_WINDOWS);
}

/// 图表 IPC 固定单日 24 小时、多日逐日，并拒绝物理 Agent 的联合维度。
#[tokio::test]
async fn chart_command_keeps_fixed_bucket_contract_and_dimension_boundary() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    let today = get_usage_charts_for_state(
        &state,
        UsageViewKindDto::Codex,
        UsageWindow::Today,
        UsageChartDimensionDto::Model,
        TimeStandardDto::default(),
    )
    .await
    .expect("physical chart remains readable");
    assert_eq!(today.granularity, UsageChartGranularityDto::Hour);
    assert_eq!(today.buckets.len(), 24);

    let this_month = get_usage_charts_for_state(
        &state,
        UsageViewKindDto::Codex,
        UsageWindow::ThisMonth,
        UsageChartDimensionDto::Root,
        TimeStandardDto::default(),
    )
    .await
    .expect("multi-day chart remains readable");
    assert_eq!(this_month.granularity, UsageChartGranularityDto::Day);
    assert!(!this_month.buckets.is_empty());
    assert!(this_month.buckets.len() <= 31);
    assert!(
        get_usage_charts_for_state(
            &state,
            UsageViewKindDto::Codex,
            UsageWindow::Today,
            UsageChartDimensionDto::Agent,
            TimeStandardDto::default(),
        )
        .await
        .is_err()
    );
}

/// 联合图表在空索引上仍返回完整小时桶，且 provider 明确为联合。
#[tokio::test]
async fn combined_chart_keeps_fixed_hour_buckets_for_empty_indexes() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());
    state
        .set_enabled_agents(&[UsageClientKindDto::Codex])
        .await
        .expect("fixture enables Codex");

    let chart = get_usage_charts_for_state(
        &state,
        UsageViewKindDto::All,
        UsageWindow::Today,
        UsageChartDimensionDto::Agent,
        TimeStandardDto::default(),
    )
    .await
    .expect("combined chart remains readable");
    assert_eq!(chart.granularity, UsageChartGranularityDto::Hour);
    assert_eq!(chart.buckets.len(), 24);
    assert_eq!(chart.dimension, UsageChartDimensionDto::Agent);
    assert_eq!(
        chart.fact.provider,
        loki_metis_core::ProviderKind::CombinedLocalAgents
    );
    assert!(chart.groups.is_empty());
}
