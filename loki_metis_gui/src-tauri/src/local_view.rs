//! 把本机索引快照映射为 GUI DTO（载体端薄适配）。

use std::path::Path;

use crate::backend::local_index::{LocalIndex, RootRecord};
#[cfg(test)]
use loki_metis_core::LocalIndexState;
use loki_metis_core::{
    CoverageReport, ProviderKind, SourceClientKind, SourceDiscoveryCode, SourceDiscoveryMethod,
    SourceRootInput, SourceRootSummary, TimeStandard, UsageSnapshot, WindowBoundaries,
    build_empty_local_windows, build_local_windows_with_standard, build_source_roots,
    local_storage_read_error_message,
};
use tauri::async_runtime::spawn_blocking;

#[cfg(test)]
use crate::dto::UsageWindow;
use crate::dto::{LocalRecordsSectionDto, SourceDiscoveryCodeDto, SourceRootDto, WindowUsageDto};

/// 按客户端专属 parser generation 打开索引并读取一次只读快照。
/// `calls_view`/`local_view`/`statistics_view` 三个视图适配器共用这一步
/// “打开 -> 读快照 -> 丢弃连接”的固定前奏，只在读到快照之后的映射逻辑上分叉。
pub(crate) async fn open_usage_snapshot(
    app_data_dir: &Path,
    parser_version: u32,
) -> Result<UsageSnapshot, String> {
    let mut index = LocalIndex::open_in_app_data(app_data_dir, parser_version)
        .await
        .map_err(|_| local_read_error())?;
    index.usage_snapshot().await.map_err(|_| local_read_error())
}

/// 只从 SQLite 装载近 30 个所选标准自然日，避免视图读取全历史后再过滤。
pub(crate) async fn open_recent_usage_snapshot(
    app_data_dir: &Path,
    parser_version: u32,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
) -> Result<UsageSnapshot, String> {
    let mut index = LocalIndex::open_in_app_data(app_data_dir, parser_version)
        .await
        .map_err(|_| local_read_error())?;
    let cutoff = WindowBoundaries::for_standard(
        observed_at_epoch_ms,
        time_standard,
        &jiff::tz::TimeZone::system(),
    )
    .thirty_days;
    index
        .usage_snapshot_since(cutoff)
        .await
        .map_err(|_| local_read_error())
}

/// 从本机索引读取三个本机自然日窗口，并保持统一 scope 与覆盖元数据。
#[cfg(test)]
pub(crate) async fn load_local_windows(
    app_data_dir: &Path,
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
) -> Result<LocalRecordsSectionDto, String> {
    let parser_version = SourceClientKind::Codex.parser_version();
    let source_label = ProviderKind::RolloutJsonl.parser_source_label(parser_version);
    load_local_windows_for_parser(
        app_data_dir,
        coverage,
        observed_at_epoch_ms,
        parser_version,
        ProviderKind::RolloutJsonl,
        source_label,
        TimeStandard::Local,
    )
    .await
}

/// 按客户端专属 parser generation 与 provider 读取六个日历窗口。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_local_windows_for_parser(
    app_data_dir: &Path,
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
    parser_version: u32,
    provider: ProviderKind,
    source_label: String,
    time_standard: TimeStandard,
) -> Result<LocalRecordsSectionDto, String> {
    let snapshot = open_recent_usage_snapshot(
        app_data_dir,
        parser_version,
        observed_at_epoch_ms,
        &time_standard,
    )
    .await?;
    let coverage = coverage.clone();
    let window_summary = spawn_blocking(move || {
        build_local_windows_with_standard(
            &snapshot.canonical,
            &coverage,
            snapshot.index_state,
            observed_at_epoch_ms,
            provider,
            Some(&source_label),
            time_standard,
            &jiff::tz::TimeZone::system(),
        )
        .map_err(|_| ())
    })
    .await
    .map_err(|_| local_read_error())?
    .map_err(|_| local_read_error())?;

    Ok(map_local_windows(window_summary))
}

/// 把 core 的本机窗口摘要映射为 DTO。
pub(crate) fn map_local_windows(
    summary: loki_metis_core::LocalRecordsSummary,
) -> LocalRecordsSectionDto {
    LocalRecordsSectionDto {
        index_state: summary.index_state,
        windows: summary
            .windows
            .into_iter()
            .map(|window| WindowUsageDto {
                window: crate::statistics_view::to_dto_window(window.window),
                fact: window.fact,
            })
            .collect(),
    }
}

/// 按客户端专属 parser generation 读取来源 registry 与来源根快照。
pub(crate) async fn load_source_roots_for_parser(
    app_data_dir: &Path,
    parser_version: u32,
    environment_label: &'static str,
) -> Result<Vec<SourceRootDto>, String> {
    load_source_root_summaries_for_parser(app_data_dir, parser_version, environment_label)
        .await
        .map(map_source_root_summaries_to_dto)
}

/// 按 parser 版本读取来源根摘要，供 GUI 内核外层扫描/查询复用。
pub(crate) async fn load_source_root_summaries_for_parser(
    app_data_dir: &Path,
    parser_version: u32,
    environment_label: &'static str,
) -> Result<Vec<SourceRootSummary>, String> {
    let snapshot = open_usage_snapshot(app_data_dir, parser_version).await?;
    let source_inputs = snapshot
        .roots
        .iter()
        .map(to_source_root_input)
        .collect::<Vec<_>>();

    Ok(build_source_roots(
        &source_inputs,
        &snapshot.canonical,
        environment_label,
    ))
}

/// 为指定客户端构造无扫描四窗口兜底零值。
pub(crate) fn empty_local_windows(
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
) -> LocalRecordsSectionDto {
    let parser_version = SourceClientKind::Codex.parser_version();
    let source_label = ProviderKind::RolloutJsonl.parser_source_label(parser_version);
    empty_local_windows_for_parser(
        coverage,
        observed_at_epoch_ms,
        ProviderKind::RolloutJsonl,
        source_label,
    )
}

/// 为指定客户端构造可控 source version 的四窗口兜底零值。
pub(crate) fn empty_local_windows_for_parser(
    coverage: &CoverageReport,
    observed_at_epoch_ms: i64,
    provider: ProviderKind,
    source_label: String,
) -> LocalRecordsSectionDto {
    let summary = build_empty_local_windows(
        coverage,
        observed_at_epoch_ms,
        provider,
        Some(&source_label),
    );

    map_local_windows(summary)
}

/// 把本机索引的 `RootRecord` 映射为 core 侧构造来源摘要所需的输入。
fn to_source_root_input(record: &RootRecord) -> SourceRootInput {
    SourceRootInput {
        id: record.root_id.clone(),
        alias: record.alias.clone(),
        enabled: record.enabled,
        activation_state: record.activation_state,
        is_primary: record.is_primary,
        discovery_method: to_source_discovery_method(record.discovery_method),
        source_file_count: record.source_file_count,
        call_observation_count: record.call_observation_count,
    }
}

/// 把本机索引的发现方式枚举映射为 core 的语义发现方式类型。
fn to_source_discovery_method(
    method: crate::backend::local_index::DiscoveryMethod,
) -> SourceDiscoveryMethod {
    match method {
        crate::backend::local_index::DiscoveryMethod::DefaultHome => {
            SourceDiscoveryMethod::DefaultHome
        }
        crate::backend::local_index::DiscoveryMethod::Environment => {
            SourceDiscoveryMethod::Environment
        }
        crate::backend::local_index::DiscoveryMethod::Registered => {
            SourceDiscoveryMethod::Registered
        }
        crate::backend::local_index::DiscoveryMethod::FullDevice => {
            SourceDiscoveryMethod::FullDevice
        }
        crate::backend::local_index::DiscoveryMethod::MetadataDiscovery => {
            SourceDiscoveryMethod::MetadataDiscovery
        }
    }
}

/// 批量把来源根摘要映射为 DTO 列表。
pub(crate) fn map_source_root_summaries_to_dto(
    source_roots: Vec<SourceRootSummary>,
) -> Vec<SourceRootDto> {
    source_roots.into_iter().map(to_source_root_dto).collect()
}

/// 把单个来源根摘要映射为 DTO；扫描时间戳由调用方另行填充。
pub(crate) fn to_source_root_dto(root: SourceRootSummary) -> SourceRootDto {
    SourceRootDto {
        id: root.id,
        alias: root.alias,
        enabled: root.enabled,
        activation_state: match root.activation_state {
            loki_metis_core::RootActivationState::ConfirmedUnindexed => {
                crate::dto::RootActivationStateDto::ConfirmedUnindexed
            }
            loki_metis_core::RootActivationState::Indexing => {
                crate::dto::RootActivationStateDto::Indexing
            }
            loki_metis_core::RootActivationState::Ready => {
                crate::dto::RootActivationStateDto::Ready
            }
            loki_metis_core::RootActivationState::ValidationFailed => {
                crate::dto::RootActivationStateDto::ValidationFailed
            }
        },
        is_primary: root.is_primary,
        discovery_label: root.discovery_label.to_owned(),
        discovery_code: to_source_discovery_code(root.discovery_code),
        file_count: root.file_count,
        skipped_count: root.skipped_count,
        error_count: root.error_count,
        duplicate_count: root.duplicate_count,
        last_scan_at_epoch_ms: None,
    }
}

/// 把 core 的来源发现代码映射为 DTO 的稳定本地化代码。
fn to_source_discovery_code(code: SourceDiscoveryCode) -> SourceDiscoveryCodeDto {
    match code {
        SourceDiscoveryCode::DefaultRoot => SourceDiscoveryCodeDto::DefaultRoot,
        SourceDiscoveryCode::CodexEnvironment => SourceDiscoveryCodeDto::CodexEnvironment,
        SourceDiscoveryCode::ClaudeEnvironment => SourceDiscoveryCodeDto::ClaudeEnvironment,
        SourceDiscoveryCode::GrokEnvironment => SourceDiscoveryCodeDto::GrokEnvironment,
        SourceDiscoveryCode::UserRegistered => SourceDiscoveryCodeDto::UserRegistered,
        SourceDiscoveryCode::FullDevice => SourceDiscoveryCodeDto::FullDevice,
        SourceDiscoveryCode::MetadataDiscovery => SourceDiscoveryCodeDto::MetadataDiscovery,
    }
}

/// 公开读取失败后的统一降级说明。
pub(crate) fn local_read_error() -> String {
    local_storage_read_error_message().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 兜底入口仍会返回六个固定窗口。
    #[test]
    fn empty_overview_contains_only_six_approved_windows() {
        use loki_metis_core::{Completeness, Confidence, CoverageState, Freshness};

        let coverage = CoverageReport {
            state: CoverageState::Complete,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        };

        let overview = empty_local_windows(&coverage, 42);

        assert_eq!(overview.index_state, LocalIndexState::NotScanned);
        assert_eq!(overview.windows.len(), 6);
        assert_eq!(overview.windows[0].fact.freshness, Freshness::Unknown);
        assert_eq!(overview.windows[0].fact.completeness, Completeness::Unknown);
        assert_eq!(overview.windows[0].fact.confidence, Confidence::Derived);
        assert_eq!(overview.windows[0].window, UsageWindow::Today);
        assert_eq!(overview.windows[1].window, UsageWindow::Yesterday);
        assert_eq!(overview.windows[2].window, UsageWindow::ThisWeek);
        assert_eq!(overview.windows[3].window, UsageWindow::LastWeek);
        assert_eq!(overview.windows[4].window, UsageWindow::ThisMonth);
        assert_eq!(overview.windows[5].window, UsageWindow::LastMonth);
    }
}
