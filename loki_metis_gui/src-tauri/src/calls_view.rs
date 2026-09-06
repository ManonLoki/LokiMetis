//! 从单一 SQLite 快照装配调用列表 DTO（载体端薄适配）。

use std::collections::BTreeMap;
use std::path::Path;

use crate::dto::{
    UsageAvailableFiltersDto, UsageCallItemDto,
    UsageCallSortDirection as UsageCallSortDirectionDto,
    UsageCallSortField as UsageCallSortFieldDto, UsageCallsPageDto, UsageCallsQueryDto,
    UsageFilterOptionDto,
};
use crate::local_view::{local_read_error, open_recent_usage_snapshot};
#[cfg(test)]
use loki_metis_core::SourceClientKind;
use loki_metis_core::build_usage_calls_page;
use loki_metis_core::{
    ProviderKind, TimeStandard, UsageCallFilters, UsageCallItem, UsageCallSortDirection,
    UsageCallSortField, UsageCallsPage, UsageCallsQuery, UsageFilterOption,
};
use tauri::async_runtime::spawn_blocking;

/// 后端固定单页上限；IPC 与前端不能覆盖。
#[cfg(test)]
const USAGE_CALL_PAGE_SIZE: usize = loki_metis_core::USAGE_CALL_PAGE_SIZE;

/// 从本机索引读取一个稳定调用页，不触发扫描或外部请求。
#[cfg(test)]
pub(crate) async fn load_usage_calls(
    app_data_dir: &Path,
    query: &UsageCallsQueryDto,
    observed_at_epoch_ms: i64,
) -> Result<UsageCallsPageDto, String> {
    let parser_version = SourceClientKind::Codex.parser_version();
    let source_label = ProviderKind::RolloutJsonl.parser_source_label(parser_version);
    load_usage_calls_for_parser(
        app_data_dir,
        query,
        observed_at_epoch_ms,
        parser_version,
        SourceClientKind::Codex,
        ProviderKind::RolloutJsonl,
        source_label.as_str(),
        TimeStandard::Local,
    )
    .await
}

/// 按客户端专属 parser generation 读取一个稳定调用页。
#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_usage_calls_for_parser(
    app_data_dir: &Path,
    query: &UsageCallsQueryDto,
    observed_at_epoch_ms: i64,
    parser_version: u32,
    client: loki_metis_core::SourceClientKind,
    local_provider: ProviderKind,
    source_label: &str,
    time_standard: TimeStandard,
) -> Result<UsageCallsPageDto, String> {
    let snapshot = open_recent_usage_snapshot(
        app_data_dir,
        parser_version,
        observed_at_epoch_ms,
        &time_standard,
    )
    .await?;
    let core_query = to_core_query(query);
    let source_label = source_label.to_owned();
    let page = spawn_blocking(move || {
        let root_aliases = snapshot
            .roots
            .into_iter()
            .map(|root| (root.root_id, root.alias))
            .collect::<BTreeMap<_, _>>();
        build_usage_calls_page(
            &snapshot.canonical.calls,
            snapshot.index_state,
            &root_aliases,
            &core_query,
            observed_at_epoch_ms,
            client,
            local_provider,
            Some(source_label.as_str()),
        )
        .map_err(|_| ())
    })
    .await
    .map_err(|_| local_read_error())?
    .map_err(|_| local_read_error())?;

    Ok(to_dto_page(page))
}

/// 把前端调用页请求 DTO 映射为 core 的查询类型。
pub(crate) fn to_core_query(query: &UsageCallsQueryDto) -> UsageCallsQuery {
    UsageCallsQuery {
        filters: UsageCallFilters {
            model: query.filters.model.clone(),
            reasoning_effort: query.filters.reasoning_effort.clone(),
            project: query.filters.project.clone(),
            thread: query.filters.thread.clone(),
            root: query.filters.root.clone(),
        },
        sort_field: to_core_sort_field(query.sort_field),
        sort_direction: to_core_sort_direction(query.sort_direction),
        cursor: query.cursor.clone(),
    }
}

/// 把 DTO 排序字段映射为 core 的排序字段。
fn to_core_sort_field(field: UsageCallSortFieldDto) -> UsageCallSortField {
    match field {
        UsageCallSortFieldDto::OccurredAt => UsageCallSortField::OccurredAt,
        UsageCallSortFieldDto::Model => UsageCallSortField::Model,
        UsageCallSortFieldDto::ReasoningEffort => UsageCallSortField::ReasoningEffort,
        UsageCallSortFieldDto::InputTokens => UsageCallSortField::InputTokens,
        UsageCallSortFieldDto::CachedInputTokens => UsageCallSortField::CachedInputTokens,
        UsageCallSortFieldDto::UncachedInputTokens => UsageCallSortField::UncachedInputTokens,
        UsageCallSortFieldDto::OutputTokens => UsageCallSortField::OutputTokens,
        UsageCallSortFieldDto::ReasoningOutputTokens => UsageCallSortField::ReasoningOutputTokens,
        UsageCallSortFieldDto::TotalTokens => UsageCallSortField::TotalTokens,
    }
}

/// 把 DTO 排序方向映射为 core 的排序方向。
fn to_core_sort_direction(direction: UsageCallSortDirectionDto) -> UsageCallSortDirection {
    match direction {
        UsageCallSortDirectionDto::Asc => UsageCallSortDirection::Asc,
        UsageCallSortDirectionDto::Desc => UsageCallSortDirection::Desc,
    }
}

/// 把 core 的调用分页结果映射为 DTO 分页响应。
pub(crate) fn to_dto_page(page: UsageCallsPage) -> UsageCallsPageDto {
    UsageCallsPageDto {
        index_state: page.index_state,
        observed_at_epoch_ms: page.observed_at_epoch_ms,
        items: page.items.into_iter().map(to_dto_item).collect::<Vec<_>>(),
        available_filters: UsageAvailableFiltersDto {
            models: map_filter_options(page.available_filters.models),
            reasoning_efforts: map_filter_options(page.available_filters.reasoning_efforts),
            projects: map_filter_options(page.available_filters.projects),
            threads: map_filter_options(page.available_filters.threads),
            roots: map_filter_options(page.available_filters.roots),
        },
        total_count: page.total_count,
        next_cursor: page.next_cursor,
    }
}

/// 把 core 已排序的筛选选项列表映射为 DTO 列表。
fn map_filter_options(options: Vec<UsageFilterOption>) -> Vec<UsageFilterOptionDto> {
    // core 的 finalize_filter_options 已按 (label, id) 排序，这里只做字段映射。
    options
        .into_iter()
        .map(|option| UsageFilterOptionDto {
            id: option.id,
            label: option.label,
            label_code: option.label_code.into(),
            disambiguation_index: option.disambiguation_index,
        })
        .collect()
}

/// 把 core 的单条调用记录映射为 DTO。
fn to_dto_item(item: UsageCallItem) -> UsageCallItemDto {
    UsageCallItemDto {
        id: item.id,
        client: item.client.into(),
        occurred_at_epoch_ms: item.occurred_at_epoch_ms,
        project_label: item.project_label,
        project_label_code: item.project_label_code.into(),
        thread_label: item.thread_label,
        thread_label_code: item.thread_label_code.into(),
        model_label: item.model_label,
        model_label_code: item.model_label_code.into(),
        reasoning_effort_label: item.reasoning_effort_label,
        reasoning_effort_label_code: item.reasoning_effort_label_code.into(),
        uncached_input_tokens: item.uncached_input_tokens,
        fact: item.fact,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 统一复用 core 的每页大小定义，避免前后端出现不同翻页口径。
    #[test]
    fn adapter_respects_core_page_size() {
        assert_eq!(USAGE_CALL_PAGE_SIZE, loki_metis_core::USAGE_CALL_PAGE_SIZE);
    }
}
