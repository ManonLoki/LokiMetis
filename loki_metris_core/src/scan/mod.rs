//! 本机扫描：错误文案、发现合并、平台排除、进度映射与生命周期状态机。

mod errors;
mod orchestration;
mod policy;

pub use errors::{
    LocalScanErrorCategory, LocalScanErrorKindProvider, local_scan_error_message,
    local_scan_error_message_from, local_scan_task_error_message, scan_cancelled_status_message,
};
pub use orchestration::{
    DiscoveredRootIdentity, DiscoveryOutcome, RegisteredRootIdentity,
    confirmed_invalid_roots_to_remove, discovery_roots_for_active_registration,
    merge_discovery_results, reuse_registered_root_identities, scan_discovery_for_scan_kind,
};
pub use policy::{
    append_platform_discovery_excludes_for_volumes, is_discovery_path_excluded,
    is_full_discovery_skip_directory_name, path_has_full_discovery_skip_directory_name,
};
