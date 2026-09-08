use super::*;
use crate::dto::{AgentClientKindDto, LanguagePreferenceDto, UsageClientKindDto};
use loki_metis_core::{
    ScanIntervalMinutes, retention_days_range_message, scan_interval_range_message,
};
use tempfile::tempdir;

/// 验证持久化规范化快照沿用单一扫描间隔的默认五分钟 TTL。
#[test]
fn persisted_snapshot_uses_scan_interval_ttl() {
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
async fn privacy_settings_persist_as_local_preferences() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    let settings = state.privacy_settings(UsageClientKindDto::Codex).await;
    assert_eq!(settings.scan_interval_minutes, 5);
    assert!(*state.privacy_settings_loaded.lock().await);
    state
        .set_scan_interval(9)
        .await
        .expect("scan interval is persisted without network");
}

/// 验证扫描分钟间隔完整持久化且不会覆盖初始化或首次扫描标记。
#[tokio::test]
async fn persists_scan_interval_without_overwriting_other_settings() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    assert!(state.set_initialization_completed(true).await.is_ok());
    assert!(
        state
            .claim_initial_scan()
            .await
            .expect("initial scan is claimed")
    );
    state
        .set_scan_interval(15)
        .await
        .expect("scan interval is stored");

    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
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

/// 验证单一本机扫描间隔在 IPC、调度和磁盘中保持一致。
#[tokio::test]
async fn persists_one_local_scan_interval() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

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
    let state = AppRuntimeState::new(temp.path().to_path_buf());

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

/// 验证完成与重新初始化只修改独立门禁，不覆盖间隔或首次扫描标记。
#[tokio::test]
async fn toggles_initialization_status_without_overwriting_other_settings() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

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
    assert!(!stored.initial_scan_attempted);
}

/// 本机设置 IPC 与落盘快照不再携带退休能力或页面工作状态。
#[tokio::test]
async fn privacy_settings_exclude_page_session_state() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());
    state
        .set_scan_interval(7)
        .await
        .expect("a persistent setting creates the canonical file");

    let dto = state.privacy_settings(UsageClientKindDto::Codex).await;
    let dto_json = serde_json::to_value(dto).expect("privacy settings serialize");
    let stored = std::fs::read_to_string(temp.path().join("privacy-settings.json"))
        .expect("canonical settings exist");
    for field in [
        "localOnly",
        "deviceUsername",
        "deviceName",
        "deviceUniqueId",
        "deviceTimeZone",
        "remoteRefreshIntervalMinutes",
        "collectProviders",
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

/// 隐私 IPC 从统一目录公开看板可映射的四项，并保持既有 wire 值与固定顺序。
#[tokio::test]
async fn privacy_settings_publish_the_dashboard_ai_catalog() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    let dto = state.privacy_settings(UsageClientKindDto::Codex).await;
    let dto_json = serde_json::to_value(dto).expect("privacy settings serialize");

    assert_eq!(
        dto_json.get("availableAiTypes"),
        Some(&serde_json::json!([
            {"name": "Codex", "value": "codex"},
            {"name": "Claude Code", "value": "claudeCode"},
            {"name": "Grok", "value": "grokBuildCli"},
            {"name": "WorkBuddy", "value": "workbuddy"}
        ]))
    );
}

/// 验证语言切换只保存界面偏好，并保留间隔与初始化状态。
#[tokio::test]
async fn persists_language_preference_without_business_side_effects() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

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
    assert_eq!(settings.scan_interval_minutes, 3);

    let stored = load_settings(temp.path()).expect("complete settings reload");
    assert_eq!(stored.language_preference, LanguagePreferenceDto::ZhCn);
    assert!(stored.initialization_completed);
    assert_eq!(stored.scan_interval.get(), 3);
}

/// 验证首次扫描认领在任务前持久化，重复调用、重启与清空状态都不会重新认领。
#[tokio::test]
async fn claims_initial_scan_only_once_across_restarts() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

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
    let reloaded = AppRuntimeState::new(temp.path().to_path_buf());
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
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    assert!(
        !state
            .claim_initial_scan()
            .await
            .expect("safe fallback skips scan")
    );
    let settings = state
        .privacy_settings(AgentClientKindDto::Codex.into())
        .await;
    assert_eq!(settings.scan_interval_minutes, 5);
}
