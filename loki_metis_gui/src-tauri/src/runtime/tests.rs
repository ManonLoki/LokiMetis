use super::*;
use crate::dto::{AgentClientKindDto, LanguagePreferenceDto, UsageClientKindDto};
use loki_metis_core::{
    DeviceUsername, DeviceUsernameError, ScanIntervalMinutes, device_username_error_message,
    retention_days_range_message, scan_interval_range_message,
};
use tempfile::tempdir;

/// 验证持久化规范化快照沿用单一扫描间隔的默认五分钟 TTL。
#[test]
fn persisted_snapshot_uses_provider_ttl() {
    assert_eq!(ScanIntervalMinutes::default().milliseconds(), 300_000);
}

/// 验证重复读取缓存事实不会按后续轮询时间滑动延长持久有效期。
#[test]
fn persisted_snapshot_expiry_is_anchored_to_fact_observation() {
    let fact_observed_at = 20_000;

    assert_eq!(
        snapshot_valid_until_epoch_ms(fact_observed_at, 120_000),
        140_000
    );
}

/// 设置读写只落本机偏好。
#[tokio::test]
async fn privacy_settings_persist_without_official_provider_work() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || None);

    let settings = state.privacy_settings(UsageClientKindDto::Codex).await;
    assert!(settings.local_only);
    assert!(*state.privacy_settings_loaded.lock().await);
    state
        .set_local_only(false)
        .await
        .expect("localOnly preference is persisted as a compatibility field");
    state
        .set_scan_interval(9)
        .await
        .expect("scan interval is persisted without network");
}

/// 验证设备用户名校验错误会映射为不包含原始个人值的稳定中文说明。
#[test]
fn maps_device_username_errors_without_echoing_input() {
    assert_eq!(
        device_username_error_message(DeviceUsernameError::TooLong),
        "设备用户名最多 64 个字符。"
    );
    assert_eq!(
        device_username_error_message(DeviceUsernameError::ContainsControlCharacter),
        "设备用户名不能包含换行或其他控制字符。"
    );
}

/// 验证用户名与仅本机模式通过完整快照更新，任一字段变化都不会覆盖另一字段。
#[tokio::test]
async fn preserves_username_across_local_only_updates_and_clear() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    state
        .set_device_username("  示例用户的工作设备  ".to_owned())
        .await
        .expect("username is stored");
    state
        .set_local_only(true)
        .await
        .expect("local-only mode is stored");

    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert!(settings.local_only);
    assert_eq!(
        settings.device_username.as_deref(),
        Some("示例用户的工作设备")
    );
    assert_eq!(settings.device_name.as_deref(), Some("test-host"));
    assert!(settings.device_unique_id.is_some());
    assert_eq!(
        load_settings(temp.path())
            .expect("persisted settings remain valid")
            .device_username
            .as_ref()
            .map(DeviceUsername::as_str),
        Some("示例用户的工作设备")
    );

    state
        .set_device_username("  ".to_owned())
        .await
        .expect("blank input clears username");
    let cleared = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert!(cleared.local_only);
    assert!(cleared.device_username.is_none());
    assert!(
        load_settings(temp.path())
            .expect("cleared settings remain valid")
            .device_username_initialized
    );

    drop(state);
    let reloaded = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("不应回填的用户".to_owned())
    });
    assert!(
        reloaded
            .privacy_settings(AgentClientKindDto::Codex.into())
            .await
            .device_username
            .is_none()
    );
}

/// 验证远端分钟间隔完整持久化且不会覆盖用户名、仅本机模式或首次扫描标记。
#[tokio::test]
async fn persists_remote_interval_without_overwriting_other_settings() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    assert!(state.set_initialization_completed(true).await.is_ok());
    assert!(
        state
            .claim_initial_scan()
            .await
            .expect("initial scan is claimed")
    );
    state
        .set_local_only(true)
        .await
        .expect("local-only mode is stored");
    state
        .set_scan_interval(15)
        .await
        .expect("scan interval is stored");

    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert!(settings.local_only);
    assert_eq!(settings.device_username.as_deref(), Some("合成系统用户"));
    assert_eq!(settings.scan_interval_minutes, 15);
    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert!(stored.initial_scan_attempted);
    assert_eq!(stored.scan_interval.get(), 15);
    assert_eq!(
        state.set_scan_interval(0).await,
        Err(scan_interval_range_message().to_owned())
    );
    assert_eq!(
        state.set_scan_interval(1_441).await,
        Err(scan_interval_range_message().to_owned())
    );
}

/// 验证单一扫描间隔完整持久化，本机调度与远端偏好读取同一分钟数。
#[tokio::test]
async fn persists_one_scan_interval_for_local_and_remote_preference() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    state
        .set_scan_interval(3)
        .await
        .expect("scan interval is stored");

    let settings = state
        .privacy_settings(AgentClientKindDto::ClaudeCode.into())
        .await;
    assert_eq!(settings.scan_interval_minutes, 3);
    assert_eq!(state.scan_interval().await.get(), 3);
    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert_eq!(stored.scan_interval.get(), 3);
    let payload = std::fs::read_to_string(temp.path().join("privacy-settings.json"))
        .expect("persisted settings are readable");
    assert!(payload.contains("\"scanIntervalMinutes\":3"));
    assert!(!payload.contains("remoteRefreshIntervalMinutes"));
    assert!(!payload.contains("localScanIntervalMinutes"));
    assert_eq!(
        state.set_scan_interval(0).await,
        Err(scan_interval_range_message().to_owned())
    );
    assert_eq!(
        state.set_scan_interval(1_441).await,
        Err(scan_interval_range_message().to_owned())
    );
}

/// 验证自动清理天数经真实保存入口往返，非法值被拒绝且不立刻删数据。
#[tokio::test]
async fn persists_retention_days_without_pruning_on_save() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    assert_eq!(state.retention_days().await.get(), 90);
    state
        .set_retention_days(180)
        .await
        .expect("retention days are stored");
    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert_eq!(settings.retention_days, 180);
    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert_eq!(stored.retention_days.get(), 180);
    assert_eq!(
        state.set_retention_days(0).await,
        Err(retention_days_range_message().to_owned())
    );
    assert_eq!(
        state.set_retention_days(3_651).await,
        Err(retention_days_range_message().to_owned())
    );
}

/// 验证完成与重新初始化只修改独立门禁，不覆盖身份、间隔或首次扫描标记。
#[tokio::test]
async fn toggles_initialization_status_without_overwriting_other_settings() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    state
        .set_scan_interval(15)
        .await
        .expect("scan interval is stored");
    state
        .set_language_preference(LanguagePreferenceDto::EnUs)
        .await
        .expect("language preference is stored");
    assert!(state.initialization_status().await.initialization_completed);

    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert!(stored.initialization_completed);
    assert_eq!(stored.language_preference, LanguagePreferenceDto::EnUs);
    assert_eq!(stored.scan_interval.get(), 15);
    assert_eq!(
        stored.device_username.as_ref().map(DeviceUsername::as_str),
        Some("合成系统用户")
    );
    assert!(stored.device_unique_id.is_some());
    assert!(!stored.initial_scan_attempted);
}

/// 隐私 IPC 与落盘设置不再携带任何页面工作状态。
#[tokio::test]
async fn privacy_settings_exclude_page_session_state() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || None);
    state
        .set_scan_interval(7)
        .await
        .expect("a persistent setting creates the canonical file");

    let dto = state.privacy_settings(UsageClientKindDto::Codex).await;
    let dto_json = serde_json::to_value(dto).expect("privacy settings serialize");
    let stored = std::fs::read_to_string(temp.path().join("privacy-settings.json"))
        .expect("canonical settings exist");
    for field in [
        "overviewWindowCodex",
        "usageWindowCodex",
        "usageDimensionCodex",
        "chartPreferences",
        "leaderboardWindow",
        "leaderboardProviderId",
        "timeStandard",
        "lastSelectedAgent",
    ] {
        assert!(dto_json.get(field).is_none(), "IPC excludes {field}");
        assert!(!stored.contains(field), "disk settings exclude {field}");
    }
}

/// 验证语言切换只保存界面偏好，并保留身份、模式、间隔与初始化状态。
#[tokio::test]
async fn persists_language_preference_without_business_side_effects() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("合成系统用户".to_owned())
    });

    state.set_local_only(true).await.expect("mode is stored");
    state
        .set_scan_interval(3)
        .await
        .expect("scan interval is stored");
    state
        .set_initialization_completed(true)
        .await
        .expect("initialization state is stored");
    state
        .set_language_preference(LanguagePreferenceDto::ZhCn)
        .await
        .expect("language preference is stored");

    let status = state.initialization_status().await;
    assert!(status.initialization_completed);
    assert_eq!(status.language_preference, LanguagePreferenceDto::ZhCn);
    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert_eq!(settings.language_preference, LanguagePreferenceDto::ZhCn);
    assert!(settings.local_only);
    assert_eq!(settings.scan_interval_minutes, 3);

    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert_eq!(stored.language_preference, LanguagePreferenceDto::ZhCn);
    assert!(stored.local_only);
    assert!(stored.initialization_completed);
    assert_eq!(stored.scan_interval.get(), 3);
    assert_eq!(
        stored.device_username.as_ref().map(DeviceUsername::as_str),
        Some("合成系统用户")
    );
}

/// 验证首次扫描认领在任务前持久化，重复调用、重启与清空状态都不会重新认领。
#[tokio::test]
async fn claims_initial_scan_only_once_across_restarts() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || None);

    assert!(
        state
            .claim_initial_scan()
            .await
            .expect("first claim succeeds")
    );
    assert!(
        !state
            .claim_initial_scan()
            .await
            .expect("second claim is skipped")
    );
    state.mark_index_cleared(AgentClientKindDto::Codex).await;
    state
        .set_initialization_completed(false)
        .await
        .expect("fixture initialization resets");
    assert!(
        !state
            .claim_initial_scan()
            .await
            .expect("clear does not reset claim")
    );
    assert!(load_settings(temp.path()).unwrap().initial_scan_attempted);

    drop(state);
    let reloaded = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || None);
    assert!(
        !reloaded
            .claim_initial_scan()
            .await
            .expect("restart keeps reset and prior claim")
    );
}

/// 验证损坏设置走保守回退并跳过自动扫描，不能用磁盘副作用掩盖配置错误。
#[tokio::test]
async fn corrupt_settings_skip_initial_scan() {
    let temp = tempdir().expect("isolated app-data is available");
    std::fs::write(temp.path().join("privacy-settings.json"), b"not-json")
        .expect("corrupt fixture is written");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        panic!("corrupt settings must not read a username candidate")
    });

    assert!(
        !state
            .claim_initial_scan()
            .await
            .expect("safe fallback skips scan")
    );
    assert!(
        state
            .privacy_settings(AgentClientKindDto::Codex.into())
            .await
            .local_only
    );
}

/// 验证平台指定字段中的 Unicode 会话用户名可以作为初始化候选。
#[test]
fn accepts_unicode_session_username_candidate() {
    assert_eq!(
        unicode_username_candidate(Some(OsString::from("本机用户"))).as_deref(),
        Some("本机用户")
    );
    assert_eq!(unicode_username_candidate(None), None);
}

/// 验证 Unix 非 Unicode USER 保持空白，不读取其他未批准身份字段。
#[cfg(unix)]
#[test]
fn rejects_non_unicode_session_username() {
    use std::os::unix::ffi::OsStringExt;

    assert_eq!(
        unicode_username_candidate(Some(OsString::from_vec(vec![0xff]))),
        None
    );
}
