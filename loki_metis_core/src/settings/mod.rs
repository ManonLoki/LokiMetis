//! 设备身份、扫描间隔与设置路径共用文案。

mod device_identity;
mod enabled_agents;
mod initialization;
mod messages;
mod retention_days;
mod scan_interval;
mod workbuddy_stats;

/// 新安装是否默认只读取本机记录；该字段只作为兼容偏好保存。
pub const DEFAULT_LOCAL_ONLY: bool = true;

pub use device_identity::{
    DEVICE_NAME_MAX_CHARS, DEVICE_USERNAME_MAX_CHARS, DeviceName, DeviceNameError, DeviceUniqueId,
    DeviceUniqueIdError, DeviceUsername, DeviceUsernameError, device_name_error_message,
    device_username_error_message, resolve_device_unique_id,
};
pub use enabled_agents::{
    EnabledAgents, EnabledAgentsError, agent_wire_label, enabled_agents_error_message,
    parse_single_agent_label,
};
pub use initialization::{
    CURRENT_INITIALIZATION_WIZARD_KEY, initialization_completed_from_stored,
    initialization_wizard_key_for_store,
};
pub use messages::{
    device_username_save_failed_message, index_location_claude_code_label,
    index_location_codex_label, index_location_grok_build_cli_label,
    initial_scan_state_save_failed_message, initialization_state_save_failed_message,
    language_setting_save_failed_message, last_selected_agent_save_failed_message,
    overview_window_save_failed_message, privacy_settings_save_failed_message,
    provider_mode_switch_failed_message, retention_days_range_message,
    retention_days_save_failed_message, scaffold_status_not_ready_message,
    scaffold_status_ready_message, scan_interval_range_message, scan_interval_save_failed_message,
    usage_query_save_failed_message, usage_smoke_overview_message,
    workbuddy_stats_enabled_save_failed_message,
};
pub use retention_days::{
    DEFAULT_RETENTION_DAYS, MAX_RETENTION_DAYS, MIN_RETENTION_DAYS, RetentionDays,
    RetentionDaysError, resolve_retention_days,
};
pub use scan_interval::{
    DEFAULT_SCAN_INTERVAL_MINUTES, MAX_SCAN_INTERVAL_MINUTES, MIN_SCAN_INTERVAL_MINUTES,
    ScanIntervalError, ScanIntervalMinutes, resolve_scan_interval_minutes,
};
pub use workbuddy_stats::{DEFAULT_WORKBUDDY_STATS_ENABLED, resolve_workbuddy_stats_enabled};

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证新安装默认保存仅本机偏好。
    #[test]
    fn defaults_new_install_to_local_only() {
        const { assert!(DEFAULT_LOCAL_ONLY) }
    }
}
