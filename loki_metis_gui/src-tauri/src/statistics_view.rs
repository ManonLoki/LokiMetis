//! 从本机索引快照映射为本地自然日桶和安全统计 DTO。

use std::collections::BTreeMap;
use std::path::Path;

#[cfg(test)]
use loki_metis_core::SourceClientKind;
use loki_metis_core::{
    CoverageReport, ProviderKind, STATISTICS_GROUP_LIMIT, TimeStandard, UsageDimension,
    build_usage_statistics_with_standard,
};
use tauri::async_runtime::spawn_blocking;

use crate::dto::{UsageDailyBucketDto, UsageGroupDto, UsageStatisticsDto, UsageWindow};
use crate::local_view::{local_read_error, open_recent_usage_snapshot};

/// 从一个 SQLite 只读事务读取统计快照，并生成命中结构。
#[cfg(test)]
pub(crate) async fn load_usage_statistics(
    app_data_dir: &Path,
    coverage: &CoverageReport,
    window: UsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
) -> Result<UsageStatisticsDto, String> {
    let parser_version = SourceClientKind::Codex.parser_version();
    let source_label = ProviderKind::RolloutJsonl.parser_source_label(parser_version);
    load_usage_statistics_for_parser(
        app_data_dir,
        coverage,
        window,
        dimension,
        observed_at_epoch_ms,
        parser_version,
        ProviderKind::RolloutJsonl,
        source_label.as_str(),
        TimeStandard::Local,
    )
    .await
}

/// 按客户端专属 parser generation 从一个 SQLite 快照读取统计。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_usage_statistics_for_parser(
    app_data_dir: &Path,
    coverage: &CoverageReport,
    window: UsageWindow,
    dimension: UsageDimension,
    observed_at_epoch_ms: i64,
    parser_version: u32,
    provider: ProviderKind,
    source_label: &str,
    time_standard: TimeStandard,
) -> Result<UsageStatisticsDto, String> {
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
        build_usage_statistics_with_standard(
            &snapshot.canonical,
            &root_aliases,
            snapshot.index_state,
            &coverage,
            to_core_window(window),
            dimension,
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

    Ok(to_dto_statistics(page))
}

/// 把 core 的统计分页结果映射为 DTO 统计响应。
pub(crate) fn to_dto_statistics(page: loki_metis_core::UsageStatisticsPage) -> UsageStatisticsDto {
    UsageStatisticsDto {
        window: to_dto_window(page.window),
        dimension: page.dimension,
        index_state: page.index_state,
        lower_bound_epoch_ms: page.lower_bound_epoch_ms,
        observed_at_epoch_ms: page.observed_at_epoch_ms,
        fact: page.fact,
        daily_buckets: page
            .daily_buckets
            .into_iter()
            .map(|bucket| UsageDailyBucketDto {
                local_date: bucket.local_date,
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

/// 把 core 的单个统计分组映射为 DTO。
pub(crate) fn to_dto_group(group: loki_metis_core::UsageGroupDisplay) -> UsageGroupDto {
    UsageGroupDto {
        id: group.id,
        label: group.label,
        label_code: group.label_code.into(),
        disambiguation_index: group.disambiguation_index,
        total_token_share_basis_points: group.total_token_share_basis_points,
        measure: group.measure,
        remainder: group.remainder,
    }
}

/// 把 DTO 窗口枚举映射为 core 的本机窗口枚举。
pub(crate) fn to_core_window(window: UsageWindow) -> loki_metis_core::LocalUsageWindow {
    match window {
        UsageWindow::Today => loki_metis_core::LocalUsageWindow::Today,
        UsageWindow::Yesterday => loki_metis_core::LocalUsageWindow::Yesterday,
        UsageWindow::ThisWeek => loki_metis_core::LocalUsageWindow::ThisWeek,
        UsageWindow::LastWeek => loki_metis_core::LocalUsageWindow::LastWeek,
        UsageWindow::ThisMonth => loki_metis_core::LocalUsageWindow::ThisMonth,
        UsageWindow::LastMonth => loki_metis_core::LocalUsageWindow::LastMonth,
    }
}

/// 把 core 的本机窗口枚举映射为 DTO 窗口枚举；`local_view`/`statistics_view`
/// 两个视图适配器共用同一份映射，避免新增窗口变体时要在两处同步改动。
pub(crate) fn to_dto_window(window: loki_metis_core::LocalUsageWindow) -> UsageWindow {
    match window {
        loki_metis_core::LocalUsageWindow::Today => UsageWindow::Today,
        loki_metis_core::LocalUsageWindow::Yesterday => UsageWindow::Yesterday,
        loki_metis_core::LocalUsageWindow::ThisWeek => UsageWindow::ThisWeek,
        loki_metis_core::LocalUsageWindow::LastWeek => UsageWindow::LastWeek,
        loki_metis_core::LocalUsageWindow::ThisMonth => UsageWindow::ThisMonth,
        loki_metis_core::LocalUsageWindow::LastMonth => UsageWindow::LastMonth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试入口仍保持与核心统计窗口映射一致。
    #[test]
    fn to_dto_keeps_window_mapping() {
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::Today)),
            UsageWindow::Today
        );
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::Yesterday)),
            UsageWindow::Yesterday
        );
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::ThisWeek)),
            UsageWindow::ThisWeek
        );
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::LastWeek)),
            UsageWindow::LastWeek
        );
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::ThisMonth)),
            UsageWindow::ThisMonth
        );
        assert_eq!(
            to_dto_window(to_core_window(UsageWindow::LastMonth)),
            UsageWindow::LastMonth
        );
    }
}
