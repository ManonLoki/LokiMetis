//! 诡秘神谕（LokiMetris）领域核心：平台无关的用量类型、窗口、聚合、数据源策略与设置规则。
//! GUI adapter 只负责本机 I/O、IPC 与展示；本 crate 不依赖 GPUI 或任何宿主 UI 库。
//!
//! # 模块分组与业务流向
//!
//! - 用量事实：`usage`（Token 字段口径）、`provider`、`source`（单次调用事实与来源）
//!   → `aggregate`（跨来源去重规范化、过滤与求和）→ `timeline`/`time_standard`（自然日窗口）
//!   → `statistics`（固定维度 Top-N 分组）。
//! - 持久化：`local_index`（Codex、Claude Code、Grok 各自独立的派生 SQLite 库、解析检查点、数据根登记）与 `private_sqlite`。
//! - 页面模型：`local_view`（概览窗口与数据根摘要）、`statistics_view`、`chart_view`、`calls_view`、
//!   `combined_view`（All 汇总）、`workbuddy_stats`、`today_usage`、`usage_milestones`。
//!   adapter 读出索引后只调用这些构建函数，不自行聚合。
//! - 扫描与数据根策略：`policy`（扫描窗口、来源访问判定、数据根校验与变更结果）、`scan`、`root_discovery`、
//!   `source_root`、`client_ports`。
//! - 监控与桌宠：`agent_hooks`（Hook 协议、配置生成与显示状态机）、`provider_quota`（5 小时、周与账单月窗口解释）、`ai_capabilities`、`monitor_profile`、
//!   `monitor_gallery`、`pet_overlay`、`pet_overlay_position`、`pet_preferences`、`window_placement`。
//! - 设置与展示辅助：`settings`、`bounded_minutes`、`display_label`、`display_name`、`codex_skin`。

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod agent_hooks;
mod aggregate;
mod ai_capabilities;
mod bounded_minutes;
mod calls_view;
mod chart_view;
mod client_ports;
mod codex_skin;
pub mod cursor_usage;
mod combined_view;
mod display_label;
mod display_name;
pub mod devices;
mod local_index;
mod local_view;
mod monitor_gallery;
mod monitor_profile;
mod pet_overlay;
mod pet_overlay_position;
mod pet_preferences;
mod policy;
mod private_sqlite;
mod provider;
mod provider_quota;
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
pub mod window_placement;
pub mod today_usage;
pub mod usage_milestones;
pub use agent_hooks::{HookNotification, hook_notification};
pub use agent_hooks::HookEventScope;
pub use today_usage::{UsageDay, TodayAvailability, TodayUsageSnapshot, TodayUsageDisplay, project_today_usage_display};
mod workbuddy_stats;

#[cfg(any(test, feature = "test-support"))]
pub use agent_hooks::ai_tool_descriptors;
pub use agent_hooks::{
    AiTool, AiToolDescriptor, CLAUDE_STATUS_LINE_EVENT, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_EPHEMERAL_PORT,
    HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, HookBehavior, HookConfigDirectories, HookConfigLocation,
    HookConfigPreview, HookConfigWriteResult, HookError, HookEventDecision, HookRegistrationMode, HookStateMachine,
    HookTransition, HookWriteOutcome, MAX_NATIVE_HOOK_INPUT_BYTES, MinimalHookPayload,
    PreparedNativeHook, ai_tool_name, generate_hook_auxiliary_configs,
    generate_hook_config, generate_wsl_hook_config, hook_config_filename, hook_config_write_result,
    hook_relay_loopback_address, hook_supports_wsl, managed_hook_marker, merge_hook_config,
    normalize_enabled_ai_tools, prepare_native_hook, remove_managed_hook_entries, tool_from_slug,
};
pub use aggregate::{
    CanonicalUsageSet, CanonicalizationWarning, CanonicalizationWarningKind,
    aggregate_canonical_usage, attach_session_snapshots, canonicalize_session_snapshots,
    canonicalize_usage_calls, filter_canonical_usage,
};
pub use ai_capabilities::{
    PUBLIC_AI_CAPABILITIES, PublicAiCapability, dashboard_selection_for_ai_tools,
    is_public_monitor_tool, public_ai_capabilities, public_monitor_ai_tools,
};
pub use calls_view::{
    USAGE_CALL_PAGE_SIZE, UsageAvailableFilters, UsageCallFilters, UsageCallItem, UsageCallRow,
    UsageCallSortDirection, UsageCallSortField, UsageCallsPage, UsageCallsQuery,
    UsageFilterOption, build_usage_calls_page, cursor_error, cursor_start, decode_cursor,
    encode_cursor, fingerprint_query, fingerprint_snapshot,
};
pub use chart_view::{
    UsageChartBucket, UsageChartDimension, UsageChartGranularity, UsageChartPage,
    build_usage_chart_with_standard,
};
pub use client_ports::{
    LocalScanOutput, LocalScanProgress, ScanCancellation, USAGE_INDEX_FILE_NAME,
    source_client_app_data_dir, source_client_usage_index_path,
};
pub use codex_skin::{
    ColorMode, MAX_SKIN_CREATOR_TEXT_CHARS, MAX_SKIN_DELETE_BATCH_ITEMS, MAX_SKIN_ID_BYTES,
    MAX_SKIN_IMPORT_BATCH_FILES, MAX_THEME_COMMENT_CHARS, MAX_THEME_CSS_BYTES,
    MAX_THEME_DESCRIPTION_CHARS, MAX_THEME_IMAGE_BYTES, SkinHostKind, SkinPackageType,
    SkinReference, SkinRuleError, SkinSource, is_valid_skin_id, normalize_skin_creator_text,
    validate_skin_code_execution_consent, validate_skin_delete_batch,
    validate_skin_import_batch_size, validate_skin_reference, validate_supported_color_modes,
    validate_theme_image_name, validate_theme_metadata,
};
pub use combined_view::{
    AgentUsageSnapshot, CombinedUsageSnapshot, UsageViewError,
    build_combined_local_windows_with_standard, build_combined_usage_calls_page,
    build_combined_usage_chart_with_standard, combine_agent_usage_snapshots,
};

pub use display_label::DisplayLabelCode;
pub use display_name::{
    SAFE_DISPLAY_NAME_MAX_CHARS, claude_project_display_label, grok_project_display_label,
    safe_path_basename, safe_thread_title,
};
pub use local_index::{
    ClaudeBatchOutcome, DiscoveryMethod, LocalError, LocalErrorKind, LocalIndex, RegisteredRoot,
    RootRecord, RootUsageSummaryRecord, SourceParseCheckpoint, StoredSourceFile, UsageSnapshot,
    path_key, stable_id,
};
pub use local_view::{
    LocalRecordsSummary, SourceDiscoveryCode, SourceDiscoveryMethod, SourceRootInput,
    SourceRootSummary, WindowUsage, build_indexed_source_roots,
    build_local_windows_with_standard,
};
pub use monitor_gallery::{
    ImageFormat, MonitorImageCatalog, MonitorImageCounts, MonitorImageMetadata,
    assemble_image_catalog, image_data_url, metadata_from_bytes,
};
pub use monitor_profile::{
    AiProfileDraft, AiProfileDraftSet, DEFAULT_PROFILE_SLOT, HookContent, MAX_PROFILE_SLOT,
    MIN_PROFILE_SLOT, MonitorCapabilityRange, clamp_profile_slot, merge_profile_drafts,
    profile_slot_range, validate_profile_draft, validate_profile_behaviors, ensure_monitor_image_unused,
};
pub use pet_overlay::{
    PET_OVERLAY_SLOT_COUNT, PetLayout, PetOverlayPage, PetOverlaySlot, PetOverlayTile,
    PetOverlayToolState, PetOverlayView, apply_pet_overlay_transition, pet_overlay_page_count,
    project_pet_overlay_from_drafts, project_pet_overlay_page,
    project_available_pet_overlay_page, first_populated_pet_slot, wrap_pet_overlay_page,
};
pub use pet_overlay_position::{
    PetOverlayPosition, PetOverlayWorkArea, resolve_pet_overlay_position,
};
pub use policy::messages::{
    source_root_not_found_message, source_root_reindex_requires_enabled_message,
    source_root_reindex_validation_failed_message,
};
pub use policy::{
    BusyWriterAction, scan_target_clients,
    DiscoveryBatchIndexDecision, DiscoveryBatchKind, LOCAL_DISCOVERY_WORKER_LIMIT,
    LocalIndexScanPolicy, LocalIndexScanWindow, ManualSourceAddDecision,
    ManualSourceInspectOutcome, PrimarySourceRootSupportError, ScanKind, ScanStartOrigin,
    SourceClientKind, SourceFileObservation, SourceRootAliasValidationError,
    SourceRootDiscoveryVerificationError, SourceRootIdValidationError, SourceRootMutationKind,
    SourceRootMutationOutcome, UsageFilterIdValidationError, WORKBUDDY_HOME_DIR_NAME,
    WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME, WORKBUDDY_PROJECTS_DIR_NAME, WorkBuddyScanSource,
    coverage_completeness, decide_manual_source_add, discover_workbuddy_scan_sources,
    discovery_batch_index_decision, empty_coverage, ensure_primary_source_root_supported,
    ensure_single_verified_source_root, extract_single_verified_source_root,
    is_safe_usage_filter_id, local_index_scan_policy_with_retention, merge_coverage_reports,
    extend_coverage_with_partial_scan, normalize_source_root_alias, retain_ingestable_calls,
    safe_model_label, safe_root_label, safe_short_value, safe_technical_label,
    source_file_needs_visit, source_file_visit_decision, PersistedSourceQuality,
    SourceVisitDecision, source_root_add_outcome, source_root_alias_from_path,
    source_root_primary_outcome, source_root_remove_outcome, source_root_rename_outcome,
    source_root_set_enabled_outcome, validate_source_root_id, validate_usage_filter_id,
    workbuddy_home_from_user_home, workbuddy_homes_from_user_home,
};
pub use provider::{
    Completeness, Confidence, Freshness, MetricFact, MetricScope, ProviderKind,
    empty_token_usage_for_provider,
};
pub use provider_quota::{
    ProviderQuotaSnapshot,
    ProviderQuota, QuotaFailure, QuotaProvider, QuotaWindow, QuotaWindowKind,
    CLAUDE_STATUSLINE_QUOTA_FILENAME, claude_statusline_quota_record,
    classify_window_seconds, parse_provider_quota, supported_quota_providers,
};
pub use root_discovery::{
    RootActivationState, RootCandidate, RootCandidateEvidence, RootCandidateSelectionError,
    RootDiscoveryCoordinator, RootDiscoveryLifecycle, RootDiscoveryPlatform, RootDiscoveryProgress,
    RootDiscoveryScope, RootDiscoveryStatus, RootDiscoveryStrategy,
};
pub use scan::{
    DiscoveredRootIdentity, LocalScanErrorCategory, LocalScanErrorKindProvider,
    RegisteredRootIdentity, ScanDiscoveryResult, append_platform_discovery_excludes_for_volumes,
    confirmed_invalid_roots_to_remove, discovery_roots_for_active_registration,
    is_discovery_path_excluded, is_full_discovery_skip_directory_name, local_scan_error_message,
    local_scan_error_message_from, local_scan_task_error_message, merge_discovery_results,
    path_has_full_discovery_skip_directory_name, reuse_registered_root_identities,
    scan_cancelled_status_message, scan_discovery_for_scan_kind,
};
pub use settings::{
    UsageSettings, UsageSettingField, DEFAULT_RETENTION_DAYS, DEFAULT_SCAN_INTERVAL_MINUTES,
    MAX_RETENTION_DAYS, MAX_SCAN_INTERVAL_MINUTES, MIN_RETENTION_DAYS, MIN_SCAN_INTERVAL_MINUTES,
    RetentionDays, RetentionDaysError, ScanIntervalError, ScanIntervalMinutes, agent_wire_label,
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
    TimeStandard,
};
pub use timeline::{
    LocalUsageWindow, TimelineError, WindowBoundaries, civil_at, civil_date_for_timestamp,
    dates_for_day_count, dates_for_window, day_start_epoch_ms, filter_canonical_usage_for_date,
    filter_canonical_usage_for_dates, first_representable_in_date,
    first_representable_local_instant, inclusive_calendar_range, next_day_start_epoch_ms,
    occurred_on_dates, overview_snapshot_start_epoch_ms, retention_cutoff_epoch_ms,
};
pub use usage::{
    CodexUsageAuthority, IncrementalTokenUsageDecision, SelectedTokenUsage, TokenUsage, TokenUsageError,
    TotalTokenAccounting, select_incremental_call_usage, select_single_call_usage,
};
pub use workbuddy_stats::{
    WorkBuddyDailyBucket, WorkBuddyHourlyBucket, WorkBuddyHourlyTrend, WorkBuddyModelUsageGroup,
    WorkBuddyModelUsageWindow, WorkBuddyStatisticsSnapshot, WorkBuddyTraceRecord,
    WorkBuddyTraceStatus, WorkBuddyUsageEventRecord, WorkBuddyUsageOrigin, WorkBuddyUsageQuality,
    WorkBuddyWindowAggregate, build_workbuddy_only_local_windows, build_workbuddy_usage_details,
    combine_workbuddy_root_coverage, complete_workbuddy_coverage,
    compute_workbuddy_statistics_with_standard, mark_workbuddy_unavailable_in_local_windows,
    merge_workbuddy_into_local_windows,
};

// 桌宠运行态的尺寸与字段级偏好规则由core统一。
pub use pet_preferences::{PetWindowPreferences, PetSizeRange, PetPreferenceChange, normalize_pet_window_preferences, change_pet_preferences, pet_window_dimensions, pet_size_from_resize};
