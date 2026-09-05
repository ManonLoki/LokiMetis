use super::*;

/// 验证原始读取保持未初始化状态，并能往返保存完整设置快照。
#[test]
fn persists_complete_settings_snapshot() {
    let temp = tempdir().expect("isolated app-data is available");
    assert!(LocalPrivacySettings::default().local_only);
    assert_eq!(
        load_settings(temp.path()),
        Ok(LocalPrivacySettings::default())
    );

    let username = DeviceUsername::from_setting_input("示例用户的工作设备")
        .expect("fixture is valid")
        .expect("fixture is non-empty");
    let settings = sample_settings(true, Some(username), true, true, false);
    save_settings(temp.path(), &settings).expect("complete settings are stored");

    assert_eq!(load_settings(temp.path()), Ok(settings.clone()));
}

/// 旧文件缺字段视为三个 Agent 全关；合法集合往返保存。
#[test]
fn missing_enabled_agents_field_stays_empty_and_valid_set_round_trips() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true}"#,
    )
    .expect("legacy fixture is written");
    assert!(
        load_settings(temp.path())
            .expect("legacy settings load")
            .enabled_agents
            .is_empty()
    );

    let mut settings = sample_settings(true, None, true, true, false);
    settings.enabled_agents = EnabledAgents::empty().with(
        loki_metis_core::SourceClientKind::Codex,
        true,
    );
    save_settings(temp.path(), &settings).expect("enabled agents persist");
    let loaded = load_settings(temp.path()).expect("enabled agents reload");
    assert!(
        loaded
            .enabled_agents
            .contains(loki_metis_core::SourceClientKind::Codex)
    );
    assert!(
        !loaded
            .enabled_agents
            .contains(loki_metis_core::SourceClientKind::ClaudeCode)
    );
    let payload =
        fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME)).expect("payload readable");
    assert!(payload.contains("\"enabledAgents\""));
    assert!(payload.contains("\"codex\""));
}

/// 磁盘里的未知 Agent 标识拒绝加载，不得伪装成已开放。
#[test]
fn rejects_unknown_enabled_agent_labels() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":true,"enabledAgents":["cursor"]}"#,
    )
    .expect("invalid fixture is written");
    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}

/// 旧版的页面工作状态无论形状是否合法都只被忽略，规范写回后全部移除。
#[test]
fn legacy_page_state_is_ignored_and_removed_on_canonical_rewrite() {
    let temp = tempdir().expect("isolated app-data is available");
    let path = temp.path().join(SETTINGS_FILE_NAME);
    fs::write(
        &path,
        br#"{"localOnly":true,"overviewWindowCodex":"thirtyDays","overviewWindowClaudeCode":{"bad":true},"overviewWindowGrokBuildCli":17,"usageWindowCodex":"yesterday","usageWindowClaudeCode":[],"usageWindowGrokBuildCli":false,"usageDimensionCodex":"project","usageDimensionClaudeCode":{"bad":true},"usageDimensionGrokBuildCli":null,"chartPreferences":"corrupt","leaderboardWindow":"sevenDays","leaderboardProviderId":42,"timeStandard":"remote","customTimeZone":"Asia/Shanghai","lastSelectedAgent":{"bad":true}}"#,
    )
    .expect("legacy page-state fixture is written");

    let loaded = load_settings(temp.path()).expect("legacy page state is ignored");
    assert_eq!(loaded, LocalPrivacySettings::default());

    initialize_settings(temp.path(), || None, || None)
        .expect("legacy page state triggers canonical rewrite");
    let canonical = fs::read_to_string(path).expect("canonical settings are readable");
    for removed_key in [
        "overviewWindowCodex",
        "overviewWindowClaudeCode",
        "overviewWindowGrokBuildCli",
        "usageWindowCodex",
        "usageWindowClaudeCode",
        "usageWindowGrokBuildCli",
        "usageDimensionCodex",
        "usageDimensionClaudeCode",
        "usageDimensionGrokBuildCli",
        "chartPreferences",
        "leaderboardWindow",
        "leaderboardProviderId",
        "timeStandard",
        "customTimeZone",
        "lastSelectedAgent",
    ] {
        assert!(
            !canonical.contains(removed_key),
            "removed key: {removed_key}"
        );
    }
}

/// 验证没有设置文件的新安装默认仅本机，并把该选择写入首次设置快照。
#[test]
fn initializes_new_install_with_local_only_enabled() {
    let temp = tempdir().expect("isolated app-data is available");

    let initialized =
        initialize_settings(temp.path(), || None, || None).expect("new settings initialize");

    assert!(initialized.local_only);
    assert!(
        load_settings(temp.path())
            .expect("stored settings reload")
            .local_only
    );
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("initialized settings are readable");
    assert!(payload.contains("\"localOnly\":true"));
}

/// 验证已有安装保存的远端偏好不会被新默认覆盖；ADR-091 当前仍阻止实际读取。
#[test]
fn preserves_persisted_remote_reading_opt_in() {
    let temp = tempdir().expect("isolated app-data is available");
    let settings = sample_settings(false, None, true, true, false);
    save_settings(temp.path(), &settings).expect("explicit opt-in is stored");

    let initialized = initialize_settings(
        temp.path(),
        || panic!("initialized settings must not query username"),
        || Some("fixture-host".to_owned()),
    )
    .expect("persisted settings initialize");

    assert!(!initialized.local_only);
    assert!(
        !load_settings(temp.path())
            .expect("stored settings reload")
            .local_only
    );
}

/// 验证旧单 Provider 配置迁移成最小条目，丢弃启停并把稳定 ID 写回新格式。
#[test]
fn migrates_legacy_collect_provider_to_minimal_collection() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"deviceName":"fixture-host","deviceUniqueId":"11111111-2222-4333-8444-555555555555","collectProviderEnabled":true,"collectProviderBaseUrl":"https://collector.example/base","collectProviderIntervalMinutes":17}"#,
    )
    .expect("legacy provider fixture is written");

    let migrated = initialize_settings(
        temp.path(),
        || panic!("initialized fixture must not query username"),
        || Some("fixture-host".to_owned()),
    )
    .expect("legacy provider migrates");
    let provider = migrated
        .collect_providers
        .as_slice()
        .first()
        .expect("one migrated provider exists");
    assert_eq!(
        provider.base_url.as_str(),
        "https://collector.example/base/"
    );
    assert_eq!(provider.interval.get(), 17);
    let migrated_id = provider.id.clone();

    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("migrated settings are readable");
    assert!(payload.contains("\"collectProviders\""));
    assert!(!payload.contains("collectProviderEnabled"));
    assert!(!payload.contains("collectProviderBaseUrl"));
    assert!(!payload.contains("\"name\""));
    assert!(!payload.contains("\"enabled\""));
    let reloaded = load_settings(temp.path()).expect("new provider collection reloads");
    assert_eq!(reloaded.collect_providers.as_slice()[0].id, migrated_id);
}

/// 验证没有旧 BaseURL 时迁移为空集合，绝不因旧默认字段产生隐含目标。
#[test]
fn migrates_unconfigured_legacy_collect_provider_to_empty_collection() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"collectProviderEnabled":false}"#,
    )
    .expect("legacy empty provider fixture is written");

    let settings = load_settings(temp.path()).expect("empty legacy provider migrates");
    assert!(settings.collect_providers.as_slice().is_empty());
}

/// 验证旧多项配置即使曾停用也迁移为已保存目标，并在写回时删除名称与启停字段。
#[test]
fn migrates_disabled_multi_provider_shape_to_saved_configuration() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"deviceName":"fixture-host","deviceUniqueId":"11111111-2222-4333-8444-555555555555","collectProviders":[{"id":"11111111-1111-4111-8111-111111111111","name":"Paused","enabled":false,"baseUrl":"https://collector.example","intervalMinutes":23}]}"#,
    )
    .expect("old multi-provider fixture is written");

    let migrated = initialize_settings(
        temp.path(),
        || panic!("initialized fixture must not query username"),
        || Some("fixture-host".to_owned()),
    )
    .expect("old multi-provider shape migrates");
    let provider = &migrated.collect_providers.as_slice()[0];
    assert_eq!(provider.id.as_str(), "11111111-1111-4111-8111-111111111111");
    assert_eq!(provider.base_url.as_str(), "https://collector.example/");
    assert_eq!(provider.interval.get(), 23);

    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("migrated settings are readable");
    assert!(!payload.contains("\"name\""));
    assert!(!payload.contains("\"enabled\""));
    assert!(!payload.contains("\"includeClientVersion\""));
}

/// 验证旧版本开关的 true、false 与缺失都迁移为同一当前配置并在写回时移除。
#[test]
fn discards_legacy_client_version_choices_during_canonical_rewrite() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"deviceUsernameInitialized":true,"deviceName":"fixture-host","deviceUniqueId":"11111111-2222-4333-8444-555555555555","collectProviders":[{"id":"11111111-1111-4111-8111-111111111111","baseUrl":"https://one.example","intervalMinutes":10,"includeClientVersion":true},{"id":"22222222-2222-4222-8222-222222222222","baseUrl":"https://two.example","intervalMinutes":20,"includeClientVersion":false},{"id":"33333333-3333-4333-8333-333333333333","baseUrl":"https://three.example","intervalMinutes":30}]}"#,
    )
    .expect("legacy version-choice fixture is written");

    let migrated = initialize_settings(
        temp.path(),
        || panic!("initialized fixture must not query username"),
        || Some("fixture-host".to_owned()),
    )
    .expect("all legacy version choices migrate");
    assert_eq!(migrated.collect_providers.as_slice().len(), 3);
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("canonical settings are readable");
    assert!(!payload.contains("\"includeClientVersion\""));
}

/// 验证新格式可以往返多个独立 Provider，且重复目标的磁盘集合被保守拒绝。
#[test]
fn persists_multiple_collect_providers_and_rejects_duplicate_destination() {
    let temp = tempdir().expect("isolated app-data is available");
    let mut settings = sample_settings(false, None, true, true, false);
    settings
        .collect_providers
        .add("https://one.example", 10, Some("团队别名"))
        .expect("first provider is valid");
    settings
        .collect_providers
        .add("http://two.example", 20, None)
        .expect("second provider is valid");
    save_settings(temp.path(), &settings).expect("multiple providers are stored");
    assert_eq!(load_settings(temp.path()), Ok(settings));
    let payload = fs::read_to_string(temp.path().join(SETTINGS_FILE_NAME))
        .expect("provider settings are readable");
    assert!(!payload.contains("\"includeClientVersion\""));
    assert_eq!(
        payload.matches("\"userAlias\"").count(),
        1,
        "只有设置了别名的 Provider 才写出 userAlias 字段"
    );
    assert!(payload.contains("\"userAlias\":\"团队别名\""));

    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"collectProviders":[{"id":"11111111-1111-4111-8111-111111111111","name":"One","enabled":false,"baseUrl":"https://same.example","intervalMinutes":10},{"id":"22222222-2222-4222-8222-222222222222","name":"Two","enabled":true,"baseUrl":"https://same.example/","intervalMinutes":20}]}"#,
    )
    .expect("duplicate destination fixture is written");
    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}

/// 验证磁盘上的 Provider 别名沿用设备用户名边界：合法别名恢复为强类型，超长别名保守拒绝整份设置。
#[test]
fn restores_provider_user_alias_and_rejects_oversized_alias_on_disk() {
    let temp = tempdir().expect("isolated app-data is available");
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        br#"{"localOnly":false,"collectProviders":[{"id":"11111111-1111-4111-8111-111111111111","baseUrl":"https://one.example","intervalMinutes":10,"userAlias":"  alias-one  "}]}"#,
    )
    .expect("aliased provider fixture is written");
    let loaded = load_settings(temp.path()).expect("aliased provider loads");
    assert_eq!(
        loaded.collect_providers.as_slice()[0]
            .user_alias
            .as_ref()
            .map(|alias| alias.as_str()),
        Some("alias-one")
    );

    let oversized = "x".repeat(65);
    fs::write(
        temp.path().join(SETTINGS_FILE_NAME),
        format!(
            r#"{{"localOnly":false,"collectProviders":[{{"id":"11111111-1111-4111-8111-111111111111","baseUrl":"https://one.example","intervalMinutes":10,"userAlias":"{oversized}"}}]}}"#
        ),
    )
    .expect("oversized alias fixture is written");
    assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
}
