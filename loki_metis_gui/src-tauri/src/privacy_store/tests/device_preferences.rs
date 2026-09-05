use super::*;

/// 验证缺少间隔字段的旧设置折叠为默认 5 分钟单一扫描间隔。
#[test]
fn migrates_missing_scan_interval_to_five_minutes() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true}"#,
    )
    .expect("legacy fixture is written");

    let settings = load_settings(temp.path()).expect("legacy settings load");
    assert_eq!(settings.scan_interval.get(), 5);
    assert!(!settings.initialization_completed);
    assert!(!settings.initial_scan_attempted);
    assert!(settings.device_unique_id.is_none());
    assert!(settings.device_name.is_none());
    assert_eq!(settings.language_preference, LanguagePreferenceDto::System);
}

/// 验证旧双字段不同时只生效本机扫描分钟，且写回后只剩一个扫描间隔。
#[test]
fn folds_conflicting_legacy_intervals_to_one_persisted_scan_interval() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"remoteRefreshIntervalMinutes":15,"localScanIntervalMinutes":3}"#,
    )
    .expect("conflicting legacy fixture is written");

    let loaded =
        initialize_settings(temp.path(), || None, || None).expect("conflicting intervals fold");
    assert_eq!(loaded.scan_interval.get(), 3);
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("folded settings are readable");
    assert!(payload.contains("\"scanIntervalMinutes\":3"));
    assert!(!payload.contains("remoteRefreshIntervalMinutes"));
    assert!(!payload.contains("localScanIntervalMinutes"));
    let reloaded = load_settings(temp.path()).expect("folded settings reload");
    assert_eq!(reloaded.scan_interval.get(), 3);
}

/// 验证已有合法扫描间隔不会被默认 5 覆盖。
#[test]
fn preserves_existing_valid_scan_interval() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"scanIntervalMinutes":12}"#,
    )
    .expect("explicit scan interval fixture is written");

    let settings = load_settings(temp.path()).expect("explicit interval loads");
    assert_eq!(settings.scan_interval.get(), 12);
}

/// 验证三个批准语言偏好都能与其余设置一起完整往返。
#[test]
fn persists_every_language_preference() {
    for language_preference in [
        LanguagePreferenceDto::System,
        LanguagePreferenceDto::ZhCn,
        LanguagePreferenceDto::EnUs,
    ] {
        let temp = tempdir().expect("isolated app-data is available");
        let mut settings = sample_settings(false, None, true, true, true);
        settings.language_preference = language_preference;

        save_settings(temp.path(), &settings).expect("language preference is stored");

        assert_eq!(load_settings(temp.path()), Ok(settings));
    }
}

/// 验证非法语言偏好不会被静默解释为其他语言。
#[test]
fn rejects_unknown_persisted_language_preference() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"languagePreference":"fr-FR"}"#,
    )
    .expect("invalid language fixture is written");

    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}

/// 验证越界扫描间隔使完整设置进入安全错误路径。
#[test]
fn rejects_out_of_range_persisted_scan_interval() {
    for (field, minutes) in [
        ("scanIntervalMinutes", 0),
        ("scanIntervalMinutes", 1_441),
        ("localScanIntervalMinutes", 0),
        ("remoteRefreshIntervalMinutes", 1_441),
    ] {
        let temp = tempdir().expect("isolated app-data is available");
        fs::write(
            temp.path().join(SETTINGS_FILE_NAME),
            format!(
                r#"{{"localOnly":false,"deviceUsernameInitialized":true,"{field}":{minutes}}}"#
            ),
        )
        .expect("invalid scan interval fixture is written");

        assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
    }
}

/// 验证非法持久 UUID 使完整设置进入安全错误路径，避免静默换发新身份。
#[test]
fn rejects_invalid_persisted_device_unique_id() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"deviceUniqueId":"not-a-uuid"}"#,
    )
    .expect("invalid uuid fixture is written");

    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}

/// 验证旧版只有 localOnly 的设置可读取，并在初始化时保留模式且补入候选。
#[test]
fn migrates_legacy_local_only_settings_during_initialization() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true}"#,
    )
    .expect("legacy fixture is written");

    assert_eq!(
        load_settings(temp.path()),
        Ok(LocalPrivacySettings {
            language_preference: LanguagePreferenceDto::System,
            local_only: true,
            device_username: None,
            device_username_initialized: false,
            device_name: None,
            device_unique_id: None,
            scan_interval: ScanIntervalMinutes::default(),
            retention_days: RetentionDays::default(),
            collect_providers: CollectProviderConfigs::default(),
            initialization_completed: false,
            initial_scan_attempted: false,
            enabled_agents: EnabledAgents::empty(),
            workbuddy_stats_enabled: false,
        })
    );

    let initialized = initialize_settings(
        temp.path(),
        || Some("系统用户".to_owned()),
        || Some("legacy-host".to_owned()),
    )
    .expect("legacy settings initialize");
    assert!(initialized.local_only);
    assert!(initialized.device_username_initialized);
    assert_eq!(
        initialized
            .device_username
            .as_ref()
            .map(DeviceUsername::as_str),
        Some("系统用户")
    );
    assert_eq!(
        initialized.device_name.as_ref().map(DeviceName::as_str),
        Some("legacy-host")
    );
    assert!(initialized.device_unique_id.is_some());
}

/// 验证清除用户名后仍保留当前读取模式，不把空字符串写成身份值。
#[test]
fn clears_username_without_changing_local_only_mode() {
    let temp = tempdir().expect("isolated app-data is available");
    let settings = sample_settings(true, None, true, true, false);

    save_settings(temp.path(), &settings).expect("cleared setting is stored");

    assert_eq!(load_settings(temp.path()), Ok(settings.clone()));
    assert_eq!(
        initialize_settings(
            temp.path(),
            || panic!("explicit clear must not refill the username"),
            || Some("fixture-host".to_owned()),
        ),
        Ok(settings)
    );
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("stored settings are readable in the fixture");
    assert!(!payload.contains("\"deviceUsername\":"));
    assert!(payload.contains("\"deviceUsernameInitialized\":true"));
    assert!(payload.contains("\"deviceUniqueId\":"));
    assert!(payload.contains("\"deviceName\":\"fixture-host\""));
}

/// 验证磁盘中的非法用户名使整个设置进入保守错误路径，不能透传到 GUI。
#[test]
fn rejects_invalid_persisted_username() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsername":"first\nsecond"}"#,
    )
    .expect("invalid fixture is written");

    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}

/// 验证损坏设置不会保存远端偏好，调用方可保守降级到仅本机。
#[test]
fn rejects_corrupt_settings_for_safe_fallback() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(temp.path().join(SETTINGS_FILE_NAME), b"not-json")
        .expect("corrupt fixture is written");

    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
    assert_eq!(
        LocalPrivacySettings::safe_fallback(),
        LocalPrivacySettings {
            language_preference: LanguagePreferenceDto::System,
            local_only: true,
            device_username: None,
            device_username_initialized: true,
            device_name: None,
            device_unique_id: None,
            scan_interval: ScanIntervalMinutes::default(),
            retention_days: RetentionDays::default(),
            collect_providers: CollectProviderConfigs::default(),
            initialization_completed: false,
            initial_scan_attempted: true,
            enabled_agents: EnabledAgents::empty(),
            workbuddy_stats_enabled: false,
        }
    );
}

/// 验证首次无文件时只读取一次候选、规范化保存并在后续读取中不再回填。
#[test]
fn initializes_username_once_from_lazy_candidate() {
    let temp = tempdir().expect("isolated app-data is available");
    let mut candidate_calls = 0_u8;
    let mut hostname_calls = 0_u8;

    let initialized = initialize_settings(
        temp.path(),
        || {
            candidate_calls = candidate_calls.saturating_add(1);
            Some("  本机用户  ".to_owned())
        },
        || {
            hostname_calls = hostname_calls.saturating_add(1);
            Some("  init-host  ".to_owned())
        },
    )
    .expect("first initialization succeeds");
    let first_unique_id = initialized
        .device_unique_id
        .as_ref()
        .map(DeviceUniqueId::as_str)
        .expect("unique id is generated")
        .to_owned();
    let reloaded = initialize_settings(
        temp.path(),
        || panic!("persisted initialization must not query the system candidate again"),
        || {
            hostname_calls = hostname_calls.saturating_add(1);
            Some("init-host".to_owned())
        },
    )
    .expect("initialized settings reload");

    assert_eq!(candidate_calls, 1);
    assert_eq!(hostname_calls, 2);
    assert_eq!(
        initialized
            .device_username
            .as_ref()
            .map(DeviceUsername::as_str),
        Some("本机用户")
    );
    assert_eq!(
        initialized.device_name.as_ref().map(DeviceName::as_str),
        Some("init-host")
    );
    assert!(initialized.device_username_initialized);
    assert_eq!(
        reloaded
            .device_unique_id
            .as_ref()
            .map(DeviceUniqueId::as_str),
        Some(first_unique_id.as_str())
    );
    assert_eq!(reloaded.device_username, initialized.device_username);
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("initialized settings are readable");
    assert!(payload.contains("\"deviceUsernameInitialized\":true"));
    assert!(payload.contains(&format!("\"deviceUniqueId\":\"{first_unique_id}\"")));
}

/// 验证旧设置已有用户名时直接视为已初始化，不读取新的系统候选，但会补发唯一 ID。
#[test]
fn legacy_username_prevents_system_candidate_override() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        r#"{"localOnly":false,"deviceUsername":"已有标签"}"#,
    )
    .expect("legacy username fixture is written");

    let settings = initialize_settings(
        temp.path(),
        || panic!("existing username must not query the system candidate"),
        || Some("migrated-host".to_owned()),
    )
    .expect("legacy username loads");

    assert!(settings.device_username_initialized);
    assert_eq!(
        settings
            .device_username
            .as_ref()
            .map(DeviceUsername::as_str),
        Some("已有标签")
    );
    assert!(settings.device_unique_id.is_some());
    assert_eq!(
        settings.device_name.as_ref().map(DeviceName::as_str),
        Some("migrated-host")
    );
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("legacy username migration is readable");
    assert!(payload.contains("\"deviceUsernameInitialized\":true"));
    assert!(payload.contains("\"deviceUniqueId\":"));
}

/// 验证无效或缺失候选也完成初始化，避免每次启动重复探测。
#[test]
fn missing_or_invalid_candidate_is_not_retried() {
    for candidate in [None, Some("   ".to_owned()), Some("x".repeat(65))] {
        let temp = tempdir().expect("isolated app-data is available");
        let initialized = initialize_settings(temp.path(), || candidate.clone(), || None)
            .expect("initialization ends");

        assert!(initialized.device_username.is_none());
        assert!(initialized.device_username_initialized);
        assert!(initialized.device_unique_id.is_some());
        initialize_settings(
            temp.path(),
            || panic!("completed empty initialization must not retry"),
            || None,
        )
        .expect("empty initialized settings reload");
    }
}

/// 验证损坏设置不会触发系统候选读取，并由运行时选择安全回退。
#[test]
fn corrupt_settings_never_query_system_candidate() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(temp.path().join(SETTINGS_FILE_NAME), b"not-json")
        .expect("corrupt fixture is written");

    assert_eq!(
        initialize_settings(
            temp.path(),
            || panic!("corrupt settings must not query the system candidate"),
            || panic!("corrupt settings must not query the hostname"),
        ),
        Err(PrivacyStoreError)
    );
}

/// 验证主机名变更会刷新设置快照，但不会重新生成已有唯一 ID。
#[test]
fn refreshes_hostname_without_rotating_unique_id() {
    let temp = tempdir().expect("isolated app-data is available");
    let first = initialize_settings(
        temp.path(),
        || Some("用户".to_owned()),
        || Some("host-a".to_owned()),
    )
    .expect("first load");
    let unique_id = first
        .device_unique_id
        .as_ref()
        .map(DeviceUniqueId::as_str)
        .expect("id exists")
        .to_owned();

    let second = initialize_settings(
        temp.path(),
        || panic!("username already initialized"),
        || Some("host-b".to_owned()),
    )
    .expect("hostname refresh");

    assert_eq!(
        second.device_name.as_ref().map(DeviceName::as_str),
        Some("host-b")
    );
    assert_eq!(
        second.device_unique_id.as_ref().map(DeviceUniqueId::as_str),
        Some(unique_id.as_str())
    );
}

/// 已持久化的设备唯一 ID 必须原样保留，不得读取稳定候选或重新生成。
#[test]
fn keeps_persisted_unique_id_without_reading_stable_candidate() {
    let temp = tempdir().expect("isolated app-data is available");
    save_settings(temp.path(), &sample_settings(true, None, true, true, false))
        .expect("existing id fixture is stored");

    let loaded = load_or_initialize_settings(
        temp.path(),
        || panic!("username already initialized"),
        || Some("fixture-host".to_owned()),
        || panic!("existing unique id must not query the stable candidate"),
    )
    .expect("existing id reloads");

    assert_eq!(
        loaded.device_unique_id.as_ref().map(DeviceUniqueId::as_str),
        Some("11111111-2222-4333-8444-555555555555")
    );
}

/// 缺失唯一 ID 且稳定候选可解析时，必须写入该候选而不是另一次生成。
#[test]
fn writes_stable_candidate_when_unique_id_is_missing() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true}"#,
    )
    .expect("missing-id fixture is written");

    let initialized = load_or_initialize_settings(
        temp.path(),
        || None,
        || None,
        || Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee".to_owned()),
    )
    .expect("missing id uses stable candidate");

    assert_eq!(
        initialized
            .device_unique_id
            .as_ref()
            .map(DeviceUniqueId::as_str),
        Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
    );
    let reloaded = load_or_initialize_settings(
        temp.path(),
        || panic!("username already initialized"),
        || None,
        || panic!("persisted unique id must not query the stable candidate"),
    )
    .expect("stable candidate reload");
    assert_eq!(
        reloaded
            .device_unique_id
            .as_ref()
            .map(DeviceUniqueId::as_str),
        Some("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee")
    );
}

/// 缺失唯一 ID 且稳定候选不可用时才生成，落盘后再加载必须保持该值。
#[test]
fn generates_and_persists_when_stable_candidate_is_unavailable() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true}"#,
    )
    .expect("missing-id fixture is written");

    let initialized = load_or_initialize_settings(
        temp.path(),
        || None,
        || None,
        || Some("not-a-uuid".to_owned()),
    )
    .expect("invalid candidate falls back to generate");
    let generated = initialized
        .device_unique_id
        .as_ref()
        .map(DeviceUniqueId::as_str)
        .expect("generated id exists")
        .to_owned();
    assert_ne!(generated, "not-a-uuid");
    DeviceUniqueId::parse(&generated).expect("generated id is persistable");

    let reloaded = load_or_initialize_settings(
        temp.path(),
        || panic!("username already initialized"),
        || None,
        || panic!("persisted generated id must not query the stable candidate"),
    )
    .expect("generated id reload");
    assert_eq!(
        reloaded
            .device_unique_id
            .as_ref()
            .map(DeviceUniqueId::as_str),
        Some(generated.as_str())
    );
}

/// 历史「已完成」只有旧布尔或过期 Key 时必须拉回向导，且不得清掉其它已校验字段。
#[test]
fn stale_wizard_completion_reopens_wizard_without_clearing_other_settings() {
    let temp = tempdir().expect("isolated app-data is available");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    let historical = r#"{"localOnly":false,"deviceUsername":"历史用户","deviceUsernameInitialized":true,"deviceName":"legacy-host","deviceUniqueId":"11111111-2222-4333-8444-555555555555","scanIntervalMinutes":12,"initializationCompleted":true,"initialScanAttempted":true,"enabledAgents":["codex"],"collectProviders":[{"id":"11111111-1111-4111-8111-111111111111","baseUrl":"https://collector.example/","intervalMinutes":7}]}"#;
    fs::write(&settings_path, historical).expect("historical completed fixture is written");

    let loaded = load_settings(temp.path()).expect("historical settings load");
    assert!(!loaded.initialization_completed);
    assert!(!loaded.local_only);
    assert_eq!(
        loaded.device_username.as_ref().map(DeviceUsername::as_str),
        Some("历史用户")
    );
    assert_eq!(
        loaded.device_unique_id.as_ref().map(DeviceUniqueId::as_str),
        Some("11111111-2222-4333-8444-555555555555")
    );
    assert_eq!(loaded.scan_interval.get(), 12);
    assert!(
        loaded
            .enabled_agents
            .contains(loki_metis_core::SourceClientKind::Codex)
    );
    assert_eq!(loaded.collect_providers.as_slice().len(), 1);
    assert_eq!(
        loaded.collect_providers.as_slice()[0].base_url.as_str(),
        "https://collector.example/"
    );
    assert!(loaded.initial_scan_attempted);

    fs::write(
        &settings_path,
        r#"{"localOnly":true,"deviceUsernameInitialized":true,"initializationCompleted":true,"initializationWizardKey":"expired-wizard-key"}"#,
    )
    .expect("expired key fixture is written");
    let expired = load_settings(temp.path()).expect("expired key loads");
    assert!(!expired.initialization_completed);
    assert!(expired.local_only);
}

/// 本轮 Key 下完成并向真实文件保存后，再次加载必须仍视为已完成。
#[test]
fn current_wizard_completion_survives_save_and_reload() {
    let temp = tempdir().expect("isolated app-data is available");
    let mut settings = sample_settings(true, None, true, false, false);
    save_settings(temp.path(), &settings).expect("incomplete snapshot is stored");
    assert!(
        !load_settings(temp.path())
            .expect("incomplete snapshot reloads")
            .initialization_completed
    );

    settings.initialization_completed = true;
    save_settings(temp.path(), &settings).expect("completed snapshot is stored");
    let reloaded = load_settings(temp.path()).expect("completed snapshot reloads");
    assert!(reloaded.initialization_completed);

    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("saved settings are readable");
    assert!(payload.contains(&format!(
        "\"initializationWizardKey\":\"{CURRENT_INITIALIZATION_WIZARD_KEY}\""
    )));
    assert!(payload.contains("\"initializationCompleted\":true"));
}

/// 旧文件缺少保留天数时必须当作 90 天。
#[test]
fn missing_retention_days_defaults_to_ninety() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true,"deviceUsernameInitialized":true}"#,
    )
    .expect("legacy fixture is written");

    let loaded = load_settings(temp.path()).expect("missing field loads");
    assert_eq!(loaded.retention_days.get(), 90);
}

/// 合法保留天数经真实保存再加载仍是该值。
#[test]
fn persists_retention_days_across_reload() {
    let temp = tempdir().expect("isolated app-data is available");
    let mut settings = sample_settings(true, None, true, true, false);
    settings.retention_days = RetentionDays::new(180).expect("180 days is approved");
    save_settings(temp.path(), &settings).expect("retention days are stored");

    let loaded = load_settings(temp.path()).expect("retention days reload");
    assert_eq!(loaded.retention_days.get(), 180);
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("saved settings are readable");
    assert!(payload.contains("\"retentionDays\":180"));
}

/// 越界保留天数必须被真实加载入口拒绝，不能回退成 90。
#[test]
fn rejects_out_of_range_persisted_retention_days() {
    for days in [0_u16, 3_651] {
        let temp = tempdir().expect("isolated app-data is available");
        fs::write(
            temp.path().join(SETTINGS_FILE_NAME),
            format!(
                r#"{{"localOnly":true,"deviceUsernameInitialized":true,"retentionDays":{days}}}"#
            ),
        )
        .expect("invalid retention fixture is written");
        assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
    }
}
