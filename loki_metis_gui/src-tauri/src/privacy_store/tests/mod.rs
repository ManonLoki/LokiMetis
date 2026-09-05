use super::*;
use loki_metis_core::{CURRENT_INITIALIZATION_WIZARD_KEY, RetentionDays};
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

/// 既有用例不关心稳定设备 ID 候选时，按缺失处理并走生成回退。
fn initialize_settings<F, H>(
    app_data_dir: &Path,
    username_candidate: F,
    hostname_candidate: H,
) -> Result<LocalPrivacySettings, PrivacyStoreError>
where
    F: FnOnce() -> Option<String>,
    H: FnOnce() -> Option<String>,
{
    super::load_or_initialize_settings(app_data_dir, username_candidate, hostname_candidate, || {
        None
    })
}

/// 构造带固定身份字段的完整设置快照，供往返测试复用。
fn sample_settings(
    local_only: bool,
    device_username: Option<DeviceUsername>,
    device_username_initialized: bool,
    initialization_completed: bool,
    initial_scan_attempted: bool,
) -> LocalPrivacySettings {
    LocalPrivacySettings {
        language_preference: LanguagePreferenceDto::System,
        local_only,
        device_username,
        device_username_initialized,
        device_name: Some(
            DeviceName::from_hostname("fixture-host")
                .expect("fixture hostname is valid")
                .expect("fixture hostname is non-empty"),
        ),
        device_unique_id: Some(
            DeviceUniqueId::parse("11111111-2222-4333-8444-555555555555")
                .expect("fixture uuid is valid"),
        ),
        scan_interval: ScanIntervalMinutes::default(),
        retention_days: RetentionDays::default(),
        initialization_completed,
        initial_scan_attempted,
        enabled_agents: EnabledAgents::empty(),
        workbuddy_stats_enabled: false,
    }
}

mod atomic_storage;
