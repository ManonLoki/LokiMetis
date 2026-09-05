//! 本机扫描：错误文案、发现合并、平台排除、进度映射与生命周期状态机。

mod errors;
mod orchestration;
mod policy;
mod progress;
mod state;

pub use errors::{
    LocalScanErrorCategory, LocalScanErrorKindProvider, clear_local_index_success_message,
    clear_local_index_while_scanning_message, local_scan_client_failure_message,
    local_scan_error_message, local_scan_error_message_from, local_scan_in_progress_error_message,
    local_scan_not_running_cancellable_message, local_scan_task_error_message,
    local_scan_worker_unavailable_detail, local_scan_writer_busy_failure_detail,
    local_scan_writer_busy_message, local_storage_read_error_message,
    local_usage_overview_unavailable_message, scan_cancelled_status_message,
    scan_cancelling_status_message, scan_completed_status_message, scan_idle_status_message,
    scan_progress_finished_message, scan_running_status_message,
    scan_scope_local_fixed_volumes_label, scan_scope_registered_roots_label,
    source_root_operations_blocked_by_scan_message,
};
pub use orchestration::{
    DiscoveredRootIdentity, RegisteredRootIdentity, ScanDiscoveryResult,
    confirmed_invalid_roots_to_remove, discovery_roots_for_active_registration,
    merge_discovery_results, reuse_registered_root_identities, scan_discovery_for_scan_kind,
};
pub use policy::{
    append_platform_discovery_excludes, append_platform_discovery_excludes_for_volumes,
    is_discovery_path_excluded, is_full_discovery_skip_directory_name,
    path_has_full_discovery_skip_directory_name,
};
pub use progress::{
    ScanProgressEvent, ScanProgressScope, ScanProgressScopeCode, ScanTaskProgress,
    scan_discovering_scope_label, scan_discovery_finished_label, scan_indexing_scope_label,
    scan_task_progress_view,
};
pub use state::{
    ScanCoordinator, ScanLease, ScanLifecycle, ScanScopeCode, ScanScopeProgress, ScanStateError,
    ScanStatus,
};
