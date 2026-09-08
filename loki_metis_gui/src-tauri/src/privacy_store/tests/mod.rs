use super::*;
use loki_metis_core::RetentionDays;
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

/// 通过正式加载入口执行旧字段清理与规范写回。
fn initialize_settings(app_data_dir: &Path) -> Result<LocalPrivacySettings, PrivacyStoreError> {
    super::load_or_initialize_settings(app_data_dir)
}

/// 构造完整本机设置快照，供原子存储往返测试复用。
fn sample_settings(
    initialization_completed: bool,
    initial_scan_attempted: bool,
) -> LocalPrivacySettings {
    LocalPrivacySettings {
        language_preference: LanguagePreferenceDto::System,
        scan_interval: ScanIntervalMinutes::default(),
        retention_days: RetentionDays::default(),
        initialization_completed,
        initial_scan_attempted,
        enabled_agents: EnabledAgents::empty(),
        workbuddy_stats_enabled: false,
    }
}

mod atomic_storage;
