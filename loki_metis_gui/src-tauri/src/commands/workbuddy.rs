//! WorkBuddy 本地用量统计读取 command：开关关闭时拒绝返回任何数据。

use tauri::State;

use loki_metis_core::UsageDimension;

use crate::backend::workbuddy::{
    WorkbuddyReadError, WorkbuddySourceInspection, discover_workbuddy_sources,
    inspect_workbuddy_project_sources, read_workbuddy_statistics, read_workbuddy_usage_details,
    resolve_workbuddy_homes,
};
use crate::dto::{
    TimeStandardDto, UsageWindow, WorkbuddySourceRow, WorkbuddySourceStatusDto,
    WorkbuddyStatisticsDto, WorkbuddyUsageDetailsDto,
};
use crate::runtime::{AppRuntimeState, now_epoch_ms};
use crate::statistics_view::{to_core_window, to_dto_statistics};

/// WorkBuddy 本地统计开关关闭时返回的稳定提示。
const fn workbuddy_stats_disabled_message() -> &'static str {
    "WorkBuddy 本地统计尚未开启；请先在设置中打开对应开关。"
}

/// WorkBuddy 数据源不可用（未安装或无本机记录）时返回的稳定提示。
const fn workbuddy_stats_unavailable_message() -> &'static str {
    "未找到 WorkBuddy 本机数据；请确认已安装并至少运行过一次。"
}

/// WorkBuddy project JSONL 数据源已发现但读取失败时返回的稳定提示。
const fn workbuddy_stats_read_failed_message() -> &'static str {
    "读取 WorkBuddy 本地用量统计失败；请稍后重试。"
}

/// 读取 WorkBuddy 本地用量统计快照；开关关闭时直接拒绝，不触碰磁盘。
#[tauri::command]
pub(crate) async fn get_workbuddy_statistics(
    state: State<'_, AppRuntimeState>,
    time_standard: TimeStandardDto,
) -> Result<WorkbuddyStatisticsDto, String> {
    if !state.workbuddy_stats_enabled().await {
        return Err(workbuddy_stats_disabled_message().to_owned());
    }
    let workbuddy_homes = resolve_workbuddy_homes();
    let time_standard = time_standard.into_time_standard(&loki_metis_core::device_time_zone_name());
    let now_epoch_ms = now_epoch_ms();

    read_workbuddy_statistics(&workbuddy_homes, now_epoch_ms, time_standard)
        .await
        .map(WorkbuddyStatisticsDto::from)
        .map_err(|error| match error {
            WorkbuddyReadError::SourceUnavailable => {
                workbuddy_stats_unavailable_message().to_owned()
            }
            WorkbuddyReadError::Read => workbuddy_stats_read_failed_message().to_owned(),
        })
}

/// 一次读取 WorkBuddy 用量页的逐请求统计与同窗口实际模型明细。
#[tauri::command]
pub(crate) async fn get_workbuddy_usage_statistics(
    state: State<'_, AppRuntimeState>,
    window: UsageWindow,
    dimension: UsageDimension,
    time_standard: TimeStandardDto,
) -> Result<WorkbuddyUsageDetailsDto, String> {
    if !state.workbuddy_stats_enabled().await {
        return Err(workbuddy_stats_disabled_message().to_owned());
    }
    let workbuddy_homes = resolve_workbuddy_homes();
    let time_standard = time_standard.into_time_standard(&loki_metis_core::device_time_zone_name());
    let details = read_workbuddy_usage_details(
        &workbuddy_homes,
        to_core_window(window),
        dimension,
        now_epoch_ms(),
        time_standard,
    )
    .await
    .map_err(|error| match error {
        WorkbuddyReadError::SourceUnavailable => workbuddy_stats_unavailable_message().to_owned(),
        WorkbuddyReadError::Read => workbuddy_stats_read_failed_message().to_owned(),
    })?;
    Ok(WorkbuddyUsageDetailsDto {
        statistics: to_dto_statistics(details.statistics),
        model_usage: details.model_usage.into(),
    })
}

/// 读取数据源页展示的 WorkBuddy 只读发现状态；对齐 Codex/Claude Code/Grok 的
/// 数据源表与发现面板，但「扫描」只是重新做一次本机 project JSONL 探测，
/// 不注册数据根、不建产品索引。国内版与国际版各占一行。开关关闭时直接返回
/// 关闭状态，不触碰磁盘。
#[tauri::command]
pub(crate) async fn get_workbuddy_source_status(
    state: State<'_, AppRuntimeState>,
) -> Result<WorkbuddySourceStatusDto, String> {
    Ok(workbuddy_source_status_for_state(&state).await)
}

/// 承载数据源状态 command 的可测试路径：开关关闭时直接短路，不做任何磁盘探测。
async fn workbuddy_source_status_for_state(state: &AppRuntimeState) -> WorkbuddySourceStatusDto {
    let enabled = state.workbuddy_stats_enabled().await;
    if !enabled {
        return WorkbuddySourceStatusDto::disabled();
    }
    let rows = tauri::async_runtime::spawn_blocking(|| {
        inspect_workbuddy_project_sources(&discover_workbuddy_sources())
            .into_iter()
            .map(workbuddy_source_row)
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    WorkbuddySourceStatusDto::new(true, rows)
}

/// 把单个固定根的探测结果折算成数据源表一行；只统计该根自己的缺口。
fn workbuddy_source_row(inspection: WorkbuddySourceInspection) -> WorkbuddySourceRow {
    let WorkbuddySourceInspection { source, evidence } = inspection;
    let (file_count, skipped_count, error_count, ready) = match evidence {
        Ok(evidence) => {
            let skipped_count = evidence
                .skipped_count
                .saturating_add(u64::from(evidence.budget_exhausted));
            let error_count = evidence.permission_denied_count;
            (
                evidence.file_count,
                skipped_count,
                error_count,
                evidence.file_count > 0 && skipped_count == 0 && error_count == 0,
            )
        }
        Err(WorkbuddyReadError::Read) => (0, 0, 1, false),
        Err(WorkbuddyReadError::SourceUnavailable) => (0, 0, 0, false),
    };
    WorkbuddySourceRow {
        root_id: source.root_id,
        alias: source.alias,
        file_count,
        skipped_count,
        error_count,
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::workbuddy_source_status_for_state;
    use crate::runtime::AppRuntimeState;
    use tempfile::tempdir;

    /// 开关关闭时数据源状态必须直接短路为关闭且未发现，不触碰真实用户主目录。
    #[tokio::test]
    async fn disabled_switch_short_circuits_without_touching_disk() {
        let temp = tempdir().expect("isolated app-data is available");
        let state = AppRuntimeState::new(temp.path().to_path_buf());

        let status = workbuddy_source_status_for_state(&state).await;

        assert!(!status.enabled);
        assert!(!status.installed);
        assert_eq!(status.alias, None);
        assert!(status.roots.is_empty());
    }
}
