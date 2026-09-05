//! 从本机索引快照映射图表时间桶与固定维度分布 DTO。

use std::collections::BTreeMap;
use std::path::Path;

use loki_metis_core::{
    CoverageReport, ProviderKind, STATISTICS_GROUP_LIMIT, TimeStandard, UsageChartDimension,
    build_usage_chart_with_standard,
};
use tauri::async_runtime::spawn_blocking;

use crate::dto::{
    UsageChartBucketDto, UsageChartDimensionDto, UsageChartDto, UsageChartGranularityDto,
    UsageWindow,
};
use crate::local_view::{local_read_error, open_recent_usage_snapshot};
use crate::statistics_view::{to_core_window, to_dto_group, to_dto_window};

/// 从单个物理 Agent 的专属 SQLite 一致快照读取图表。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_usage_chart_for_parser(
    app_data_dir: &Path,
    coverage: &CoverageReport,
    window: UsageWindow,
    dimension: UsageChartDimensionDto,
    observed_at_epoch_ms: i64,
    parser_version: u32,
    provider: ProviderKind,
    source_label: &str,
    time_standard: TimeStandard,
) -> Result<UsageChartDto, String> {
    let snapshot = open_recent_usage_snapshot(
        app_data_dir,
        parser_version,
        observed_at_epoch_ms,
        &time_standard,
    )
    .await?;
    let coverage = coverage.clone();
    let source_label = source_label.to_owned();
    let page = spawn_blocking(move || {
        let root_aliases = snapshot
            .roots
            .into_iter()
            .map(|root| (root.root_id, root.alias))
            .collect::<BTreeMap<_, _>>();
        build_usage_chart_with_standard(
            &snapshot.canonical,
            &root_aliases,
            snapshot.index_state,
            &coverage,
            to_core_window(window),
            UsageChartDimension::from(dimension),
            observed_at_epoch_ms,
            provider,
            Some(source_label.as_str()),
            time_standard,
            &jiff::tz::TimeZone::system(),
        )
        .map_err(|_| ())
    })
    .await
    .map_err(|_| local_read_error())?
    .map_err(|_| local_read_error())?;

    Ok(to_dto_chart(page))
}

/// 把 core 图表页映射成跨 WebView 的稳定 DTO。
pub(crate) fn to_dto_chart(page: loki_metis_core::UsageChartPage) -> UsageChartDto {
    UsageChartDto {
        window: to_dto_window(page.window),
        dimension: page.dimension.into(),
        granularity: match page.granularity {
            loki_metis_core::UsageChartGranularity::Hour => {
                UsageChartGranularityDto::Hour
            }
            loki_metis_core::UsageChartGranularity::Day => {
                UsageChartGranularityDto::Day
            }
        },
        index_state: page.index_state,
        lower_bound_epoch_ms: page.lower_bound_epoch_ms,
        observed_at_epoch_ms: page.observed_at_epoch_ms,
        fact: page.fact,
        buckets: page
            .buckets
            .into_iter()
            .map(|bucket| UsageChartBucketDto {
                key: bucket.key,
                label: bucket.label,
                in_progress: bucket.in_progress,
                measure: bucket.measure,
            })
            .collect(),
        groups: page
            .groups
            .into_iter()
            .take(STATISTICS_GROUP_LIMIT)
            .map(to_dto_group)
            .collect(),
        remainder: page.remainder.map(to_dto_group),
    }
}
