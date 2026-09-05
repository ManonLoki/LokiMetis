//! LokiMetis 看板领域核心：平台无关的用量类型、窗口、聚合、数据源策略与设置规则。
//! GUI adapter 只负责本机 I/O、IPC 与展示；本 crate 不依赖 Tauri 或 WebView。

#![deny(missing_docs)]

mod agent_hooks;
mod aggregate;
mod bounded_minutes;
mod calls_view;
mod chart_view;
mod client_ports;
mod combined_view;
mod dashboard_capabilities;
mod display_label;
mod display_name;
mod local_index;
mod local_view;
mod pet_overlay;
mod policy;
mod private_sqlite;
mod provider;
mod root_discovery;
mod scan;
mod settings;
mod source;
mod source_root;
mod statistics;
mod statistics_view;
mod time_standard;
mod timeline;
mod usage;
mod workbuddy_stats;

pub use agent_hooks::{
    AiTool, AiToolDescriptor, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookConfigDirectories,
    HookConfigLocation, HookConfigPreview, HookConfigWriteResult, HookError, HookWriteOutcome,
    MAX_NATIVE_HOOK_INPUT_BYTES, MinimalHookPayload, PreparedNativeHook, ai_tool_descriptors,
    ai_tool_name, generate_hook_config, generate_wsl_hook_config, hook_config_filename,
    hook_config_has_managed_marker, hook_config_write_result,
    managed_hook_marker, merge_hook_config, normalize_enabled_ai_tools, prepare_native_hook,
    tool_from_slug,
};
pub use aggregate::{
    CanonicalUsageSet, CanonicalizationWarning, CanonicalizationWarningKind, MetricFusionError,
    aggregate_canonical_usage, attach_session_snapshots, canonicalize_session_snapshots,
    canonicalize_usage_calls, filter_canonical_usage, partition_canonical_usage_two,
    prefer_metric_within_scope,
};
pub use calls_view::{
    BuildUsageCallRowsError, USAGE_CALL_PAGE_SIZE, UsageAvailableFilters, UsageCallFilters,
    UsageCallItem, UsageCallRow, UsageCallSortDirection, UsageCallSortField, UsageCallsPage,
    UsageCallsQuery, UsageFilterOption, build_usage_calls_page, cursor_error, cursor_start,
    decode_cursor, encode_cursor, fingerprint_query, fingerprint_snapshot,
};
pub use chart_view::{
    UsageChartBucket, UsageChartDimension, UsageChartGranularity, UsageChartPage,
    build_usage_chart_with_standard,
};
pub use client_ports::{
    AgentClientRegistry, LocalScanFuture, LocalScanOutput, LocalScanProgress, LocalUsageScanner,
    ScanCancellation, USAGE_INDEX_FILE_NAME, source_client_app_data_dir,
    source_client_usage_index_path,
};
pub use combined_view::{
    AgentUsageSnapshot, CombinedUsageSnapshot, UsageViewError, UsageViewKind,
    build_combined_local_windows_with_standard, build_combined_usage_calls_page,
    build_combined_usage_chart_with_standard, combine_agent_usage_snapshots,
    resolve_usage_view_members,
};
pub use dashboard_capabilities::{
    DASHBOARD_CAPABILITY_NAMES, PHYSICAL_SCAN_CLIENTS, dashboard_capability_names,
    physical_scan_clients,
};
pub use display_label::DisplayLabelCode;
pub use display_name::{
    SAFE_DISPLAY_NAME_MAX_CHARS, claude_project_display_label, grok_project_display_label,
    safe_path_basename, safe_thread_title,
};
pub use pet_overlay::{
    PET_OVERLAY_WINDOW_SPEC, PetOverlayImageRef, PetOverlaySlot, PetOverlayView,
    PetOverlayWindowSpec, pet_overlay_tool_from_label, pet_overlay_window_spec,
    project_pet_overlay_slots,
};
pub use local_index::{
    ClaudeBatchOutcome, DiscoveryMethod, LocalError, LocalErrorKind, LocalIndex, RegisteredRoot,
    RootRecord, SourceParseCheckpoint, StoredSourceFile, UsageSnapshot, path_key, stable_id,
};
pub use local_view::{
    LocalRecordsSummary, SourceDiscoveryCode, SourceDiscoveryMethod, SourceRootInput,
    SourceRootSummary, WindowUsage, build_empty_local_windows, build_local_windows,
    build_local_windows_with_standard, build_source_roots,
};
pub use policy::messages::{
    source_root_alias_invalid_message, source_root_alias_updated_message,
    source_root_already_registered_message, source_root_disabled_message,
    source_root_enabled_message, source_root_id_invalid_message, source_root_not_found_message,
    source_root_primary_already_selected_message, source_root_primary_changed_message,
    source_root_primary_cleared_message, source_root_primary_not_set_message,
    source_root_primary_only_for_codex_message, source_root_primary_root_not_enabled_message,
    source_root_primary_validation_failed_message, source_root_registered_message,
    source_root_reindex_requires_enabled_message, source_root_reindex_validation_failed_message,
    source_root_removed_message, source_root_selected_directory_unreadable_message,
    source_root_store_error_message,
};
pub use policy::{
    BusinessAccessError, DiscoveryBatchIndexDecision, DiscoveryBatchKind,
    LOCAL_DISCOVERY_WORKER_LIMIT, LOCAL_INDEX_WORKER_LIMIT, LocalIndexScanPolicy,
    LocalIndexScanWindow, ManualSourceAddDecision, ManualSourceInspectOutcome,
    PeriodicQuickScanError, PeriodicScanTick, PrimarySourceRootSupportError, ScanKind,
    ScanStartAccessError, ScanStartOrigin, SourceClientKind, SourceFileObservation,
    SourceRootAliasValidationError, SourceRootDiscoveryVerificationError,
    SourceRootIdValidationError, SourceRootMutationFeedback, SourceRootMutationKind,
    SourceRootMutationMessageCode, SourceRootMutationOutcome, UsageFilterIdValidationError,
    WORKBUDDY_HOME_DIR_NAME, WORKBUDDY_PROJECTS_DIR_NAME, WorkbuddyScanSource,
    business_access_error_message, coverage_completeness, decide_manual_source_add,
    decide_periodic_scan_tick, discover_workbuddy_scan_source, discovery_batch_index_decision,
    empty_coverage, ensure_business_access, ensure_periodic_quick_scan_allowed,
    ensure_primary_source_root_supported, ensure_scan_start_allowed,
    ensure_single_verified_source_root, extract_single_verified_source_root, initial_coverage,
    is_safe_usage_filter_id, list_scan_source_clients, list_workbuddy_scan_sources,
    local_index_scan_policy, local_index_scan_policy_with_retention, merge_coverage_reports,
    normalize_source_root_alias, retain_ingestable_calls, safe_root_label, safe_short_value,
    safe_technical_label, scan_start_access_error_message, source_file_needs_visit,
    source_root_add_outcome, source_root_alias_from_path, source_root_mutation_feedback,
    source_root_primary_outcome, source_root_remove_outcome, source_root_rename_outcome,
    source_root_set_enabled_outcome, source_root_toggle_enabled_outcome, validate_source_root_id,
    validate_usage_filter_id, workbuddy_home_from_user_home, workbuddy_scan_source_candidate,
};
pub use provider::{
    Completeness, Confidence, Freshness, MetricFact, MetricScope, ProviderKind,
    empty_token_usage_for_provider,
};
pub use root_discovery::{
    RootActivationState, RootCandidate, RootCandidateEvidence, RootCandidateSelectionError,
    RootDiscoveryCoordinator, RootDiscoveryLifecycle, RootDiscoveryPlatform, RootDiscoveryProgress,
    RootDiscoveryScope, RootDiscoveryStatus, RootDiscoveryStrategy,
};
pub use scan::{
    DiscoveredRootIdentity, LocalScanErrorCategory, LocalScanErrorKindProvider,
    RegisteredRootIdentity, ScanCoordinator, ScanDiscoveryResult, ScanLease, ScanLifecycle,
    ScanProgressEvent, ScanProgressScope, ScanProgressScopeCode, ScanScopeCode, ScanScopeProgress,
    ScanStateError, ScanStatus, ScanTaskProgress, append_platform_discovery_excludes,
    append_platform_discovery_excludes_for_volumes, clear_local_index_success_message,
    clear_local_index_while_scanning_message, confirmed_invalid_roots_to_remove,
    discovery_roots_for_active_registration, is_discovery_path_excluded,
    is_full_discovery_skip_directory_name, local_scan_client_failure_message,
    local_scan_error_message, local_scan_error_message_from, local_scan_in_progress_error_message,
    local_scan_not_running_cancellable_message, local_scan_task_error_message,
    local_scan_worker_unavailable_detail, local_scan_writer_busy_failure_detail,
    local_scan_writer_busy_message, local_storage_read_error_message,
    local_usage_overview_unavailable_message, merge_discovery_results,
    path_has_full_discovery_skip_directory_name, reuse_registered_root_identities,
    scan_cancelled_status_message, scan_cancelling_status_message, scan_completed_status_message,
    scan_discovering_scope_label, scan_discovery_finished_label, scan_discovery_for_scan_kind,
    scan_idle_status_message, scan_indexing_scope_label, scan_progress_finished_message,
    scan_running_status_message, scan_scope_local_fixed_volumes_label,
    scan_scope_registered_roots_label, scan_task_progress_view,
    source_root_operations_blocked_by_scan_message,
};
pub use settings::{
    CURRENT_INITIALIZATION_WIZARD_KEY, DEFAULT_LOCAL_ONLY, DEFAULT_RETENTION_DAYS,
    DEFAULT_SCAN_INTERVAL_MINUTES, DEFAULT_WORKBUDDY_STATS_ENABLED, DEVICE_NAME_MAX_CHARS,
    DEVICE_USERNAME_MAX_CHARS, DeviceName, DeviceNameError, DeviceUniqueId, DeviceUniqueIdError,
    DeviceUsername, DeviceUsernameError, EnabledAgents, EnabledAgentsError, MAX_RETENTION_DAYS,
    MAX_SCAN_INTERVAL_MINUTES, MIN_RETENTION_DAYS, MIN_SCAN_INTERVAL_MINUTES, RetentionDays,
    RetentionDaysError, ScanIntervalError, ScanIntervalMinutes, agent_wire_label,
    device_name_error_message, device_username_error_message, device_username_save_failed_message,
    enabled_agents_error_message, index_location_claude_code_label, index_location_codex_label,
    index_location_grok_build_cli_label, initial_scan_state_save_failed_message,
    initialization_completed_from_stored, initialization_state_save_failed_message,
    initialization_wizard_key_for_store, language_setting_save_failed_message,
    last_selected_agent_save_failed_message, overview_window_save_failed_message,
    parse_single_agent_label, privacy_settings_save_failed_message,
    provider_mode_switch_failed_message, resolve_device_unique_id, resolve_retention_days,
    resolve_scan_interval_minutes, resolve_workbuddy_stats_enabled, retention_days_range_message,
    retention_days_save_failed_message, scaffold_status_not_ready_message,
    scaffold_status_ready_message, scan_interval_range_message, scan_interval_save_failed_message,
    usage_query_save_failed_message, usage_smoke_overview_message,
    workbuddy_stats_enabled_save_failed_message,
};
pub use source::{
    CoverageReport, CoverageState, LocalIndexState, LocalUsageAggregate, SessionTokenSnapshot,
    SourceProvenance, UsageCall, empty_local_usage_aggregate_for_provider,
    immediate_reindex_required, local_fact_quality,
};
pub use source_root::{
    CatalogFuture, SourceRootCandidate, SourceRootCatalog, SourceRootCatalogError,
    SourceRootCatalogOperationError, SourceRootCatalogRecord, SourceRootLookupError,
    SourceRootReindexRequest, resolve_enabled_source_root_by_id, resolve_source_root_by_id,
    source_root_add, source_root_reindex_request, source_root_remove, source_root_rename,
    source_root_set_enabled, source_root_set_primary,
};
pub use statistics::{
    BoundedUsageGroups, STATISTICS_GROUP_LIMIT, UsageDimension, UsageGroup, UsageMeasure,
    group_usage, stable_group_id, summarize_usage,
};
pub use statistics_view::{
    UsageDailyBucket, UsageGroupDisplay, UsageStatisticsPage, build_usage_statistics,
    build_usage_statistics_with_standard,
};
pub use time_standard::{
    TimeStandard, TimeStandardMode, device_time_zone_name, is_known_iana_time_zone,
};
pub use timeline::{
    LocalUsageWindow, TimelineError, WindowBoundaries, belongs_to_local_window, belongs_to_window,
    civil_at, civil_date_for_timestamp, dates_for_day_count, dates_for_window, day_start_epoch_ms,
    filter_canonical_usage_for_date, filter_canonical_usage_for_dates,
    filter_canonical_usage_for_local_date, filter_canonical_usage_for_local_dates,
    filter_canonical_usage_for_local_window, filter_canonical_usage_for_window,
    first_representable_in_date, first_representable_local_instant, inclusive_calendar_range,
    local_date_for_timestamp, local_dates_for_day_count, local_dates_for_window,
    local_day_start_epoch_ms, occurred_on_dates, occurred_on_local_dates,
    retention_cutoff_epoch_ms,
};
pub use usage::{
    IncrementalTokenUsageDecision, SelectedTokenUsage, TokenUsage, TokenUsageError,
    TotalTokenAccounting, select_incremental_call_usage, select_single_call_usage,
};
pub use workbuddy_stats::{
    WorkbuddyDailyBucket, WorkbuddyHourlyBucket, WorkbuddyHourlyTrend, WorkbuddyModelUsageGroup,
    WorkbuddyModelUsageWindow, WorkbuddyStatisticsSnapshot, WorkbuddyTraceRecord,
    WorkbuddyTraceStatus, WorkbuddyUsageEventRecord, WorkbuddyUsageOrigin, WorkbuddyUsageQuality,
    WorkbuddyWindowAggregate, build_workbuddy_only_local_windows, build_workbuddy_usage_details,
    complete_workbuddy_coverage, compute_workbuddy_statistics_with_standard,
    mark_workbuddy_unavailable_in_local_windows, merge_workbuddy_into_local_windows,
    workbuddy_combined_usage_unavailable_message,
};

/// 表示工程骨架是否就绪，以及是否仍需补充产品定义。
#[derive(Debug, PartialEq, Eq)]
pub struct ScaffoldStatus {
    /// 标识项目的中性共享核心已经完成结构初始化。
    pub initialized: bool,
    /// 标识产品目的、核心输入输出和成功标准尚待明确。
    pub product_definition_required: bool,
}

/// 查询不带任何业务假设或外部副作用的中性脚手架状态。
///
/// 该异步 API 供 GUI 薄适配器复用，但不绑定 Tokio 类型、序列化格式或界面状态。
pub async fn scaffold_status() -> ScaffoldStatus {
    ScaffoldStatus {
        initialized: true,
        product_definition_required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证中性 core 只报告结构已经初始化，同时明确要求后续产品定义。
    #[tokio::test]
    async fn reports_that_product_definition_is_required() {
        assert_eq!(
            scaffold_status().await,
            ScaffoldStatus {
                initialized: true,
                product_definition_required: true,
            }
        );
    }
}
