//! 把本机索引快照映射为 GUI DTO（载体端薄适配）。

use std::path::Path;

use crate::backend::local_index::LocalIndex;
use loki_metis_core::{
    CoverageReport, ProviderKind, SourceDiscoveryCode, SourceRootSummary, TimeStandard,
    UsageSnapshot, build_empty_local_windows, build_indexed_source_roots,
    build_local_windows_with_standard, local_storage_read_error_message,
    overview_snapshot_start_epoch_ms,
};
#[cfg(test)]
use loki_metis_core::{LocalIndexState, SourceClientKind};
use tauri::async_runtime::spawn_blocking;

#[cfg(test)]
use crate::dto::UsageWindow;
use crate::dto::{LocalRecordsSectionDto, SourceDiscoveryCodeDto, SourceRootDto, WindowUsageDto};

/// 按六个日历窗口的最早起点装载 SQLite 快照，避免月度窗口被固定 30 日裁剪。
pub(crate) async fn open_calendar_usage_snapshot(
    app_data_dir: &Path,
    parser_version: u32,
    observed_at_epoch_ms: i64,
    time_standard: &TimeStandard,
) -> Result<UsageSnapshot, String> {
    let mut index = LocalIndex::open_read_only_in_app_data(app_data_dir, parser_version)
        .await
        .map_err(|_| local_read_error())?;
    let cutoff = overview_snapshot_start_epoch_ms(
        observed_at_epoch_ms,
        time_standard,
        &jiff::tz::TimeZone::system(),
    )
    .map_err(|_| local_read_error())?;
    index
        .usage_view_snapshot_since(cutoff)
        .await
        .map_err(|_| local_read_error())
}

/// 从本机索引读取六个日历窗口，并保持统一 scope 与覆盖元数据。
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
    let snapshot = open_calendar_usage_snapshot(
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
    let index = LocalIndex::open_read_only_in_app_data(app_data_dir, parser_version)
        .await
        .map_err(|_| local_read_error())?;
    let roots = index
        .root_usage_summary_records()
        .await
        .map_err(|_| local_read_error())?;
    Ok(build_indexed_source_roots(&roots, environment_label))
}

#[cfg(test)]
/// 为指定客户端构造无扫描六窗口兜底零值。
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

/// 为指定客户端构造可控 source version 的六窗口兜底零值。
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
    use sea_orm::{ConnectOptions, ConnectionTrait, Database};

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

    /// 月末与次日零点读取真实 SQLite 时，完整月窗和今日/昨日必须同步换桶。
    #[tokio::test]
    async fn calendar_snapshot_keeps_month_start_and_previous_month() {
        let temp = tempfile::tempdir().expect("isolated index directory exists");
        let parser_version = SourceClientKind::Codex.parser_version();
        let index = LocalIndex::open_in_app_data(temp.path(), parser_version)
            .await
            .expect("index schema is created");
        let database_path = index.database_path().to_path_buf();
        drop(index);

        let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
        options.map_sqlx_sqlite_opts(move |sqlite_options| {
            sqlite_options.filename(&database_path).create_if_missing(false)
        });
        let fixture = Database::connect(options)
            .await
            .expect("fixture database opens");
        let zone = jiff::tz::TimeZone::UTC;
        let standard = TimeStandard::utc();
        let day_start = |month, day| {
            loki_metis_core::day_start_epoch_ms(
                jiff::civil::date(2026, month, day),
                &standard,
                &zone,
            )
            .expect("fixture day starts")
        };
        let july_first = day_start(7, 1);
        let august_first = day_start(8, 1);
        let august_last = day_start(8, 31);
        fixture
            .execute_unprepared(&format!(
                "INSERT INTO source_roots
                   (root_id, access_path, alias, enabled, discovery_method,
                    last_coverage_state, activation_state)
                 VALUES ('root-calendar', X'01', 'Calendar', 1, 'registered', 'complete', 'ready');
                 INSERT INTO source_files
                   (source_id, root_id, relative_label, file_identity, archived,
                    observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                    oversized_tail, parser_version, generation, ready, thread_key,
                    project_key, model, reasoning_effort, call_sequence, token_snapshots_ready)
                 VALUES ('source-calendar', 'root-calendar', 'sessions/test.jsonl', 'file', 0,
                    10, {august_last}, 10, 0, 0, {parser_version}, 1, 1, 'thread',
                    NULL, NULL, NULL, 3, 1);
                 INSERT INTO usage_calls
                   (source_id, generation, logical_call_id, occurred_at_epoch_ms,
                    model, reasoning_effort, project_key, thread_key, input_tokens,
                    cached_input_tokens, cache_write_input_tokens, output_tokens,
                    reasoning_output_tokens, total_tokens, total_is_derived, confidence)
                 VALUES
                   ('source-calendar', 1, 'july', {july_first}, NULL, NULL, NULL, 'thread', 10, 0, NULL, 0, 0, 10, 0, 'exact'),
                   ('source-calendar', 1, 'august-start', {august_first}, NULL, NULL, NULL, 'thread', 20, 0, NULL, 0, 0, 20, 0, 'exact'),
                   ('source-calendar', 1, 'august-end', {august_last}, NULL, NULL, NULL, 'thread', 30, 0, NULL, 0, 0, 30, 0, 'exact');"
            ))
            .await
            .expect("calendar calls are inserted");
        drop(fixture);

        let observed = august_last + 15 * 60 * 60 * 1_000;
        let snapshot = open_calendar_usage_snapshot(temp.path(), parser_version, observed, &standard)
            .await
            .expect("all calendar calls load");
        assert_eq!(snapshot.canonical.calls.len(), 3);
        let coverage = CoverageReport {
            state: loki_metis_core::CoverageState::Complete,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        };
        let windows = load_local_windows_for_parser(
            temp.path(),
            &coverage,
            observed,
            parser_version,
            ProviderKind::RolloutJsonl,
            "calendar-test".to_owned(),
            standard.clone(),
        )
        .await
        .expect("calendar windows aggregate");
        assert_eq!(windows.windows[4].window, UsageWindow::ThisMonth);
        assert_eq!(windows.windows[4].fact.value.tokens.total_tokens, 50);
        assert_eq!(windows.windows[5].window, UsageWindow::LastMonth);
        assert_eq!(windows.windows[5].fact.value.tokens.total_tokens, 10);

        let next_day = day_start(9, 1);
        let rolled = load_local_windows_for_parser(
            temp.path(),
            &coverage,
            next_day,
            parser_version,
            ProviderKind::RolloutJsonl,
            "calendar-test".to_owned(),
            standard,
        )
        .await
        .expect("midnight windows aggregate");
        assert_eq!(rolled.windows[0].fact.value.tokens.total_tokens, 0);
        assert_eq!(rolled.windows[1].fact.value.tokens.total_tokens, 30);
        assert_eq!(rolled.windows[4].fact.value.tokens.total_tokens, 0);
        assert_eq!(rolled.windows[5].fact.value.tokens.total_tokens, 50);
    }
}
