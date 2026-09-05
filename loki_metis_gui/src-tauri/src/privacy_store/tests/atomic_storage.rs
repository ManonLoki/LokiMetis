use super::*;

/// 验证 Windows 主文件半写时从完整事务恢复，并由启动初始化重新提交主快照。
#[cfg(windows)]
#[test]
fn recovers_truncated_windows_settings_from_transaction() {
    let temp = tempdir().expect("isolated app-data is available");
    let settings = sample_settings(true, None, true, true, true);
    save_settings(temp.path(), &settings).expect("baseline settings are stored");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    let transaction_path = temp.path().join(SETTINGS_TRANSACTION_FILE_NAME);
    fs::copy(&settings_path, &transaction_path).expect("complete transaction is preserved");
    fs::write(&settings_path, b"{\"localOnly\":").expect("primary is truncated");

    let recovered = initialize_settings(
        temp.path(),
        || panic!("transaction recovery must not reinitialize username"),
        || Some("fixture-host".to_owned()),
    )
    .expect("transaction recovers the settings");

    assert_eq!(recovered, settings);
    assert_eq!(load_settings(temp.path()), Ok(settings));
    assert!(!transaction_path.exists());
}

/// 验证有效 Windows 主文件始终优先，旧事务不能回滚较新的设置。
#[cfg(windows)]
#[test]
fn valid_windows_settings_win_over_stale_transaction() {
    let temp = tempdir().expect("isolated app-data is available");
    let stale = sample_settings(true, None, true, false, false);
    save_settings(temp.path(), &stale).expect("stale fixture is stored");
    let stale_payload =
        fs::read(temp.path().join(SETTINGS_FILE_NAME)).expect("stale payload remains readable");
    let current = sample_settings(false, None, true, true, true);
    save_settings(temp.path(), &current).expect("current settings are stored");
    fs::write(
        temp.path().join(SETTINGS_TRANSACTION_FILE_NAME),
        stale_payload,
    )
    .expect("stale transaction fixture is written");

    assert_eq!(load_settings(temp.path()), Ok(current));
}

/// 验证 Windows 主文件不可用时不会接受损坏或非普通事务载荷。
#[cfg(windows)]
#[test]
fn rejects_invalid_windows_transaction_without_valid_primary() {
    for transaction_is_directory in [false, true] {
        let temp = tempdir().expect("isolated app-data is available");
        fs::write(temp.path().join(SETTINGS_FILE_NAME), b"not-json")
            .expect("primary fixture is corrupt");
        let transaction_path = temp.path().join(SETTINGS_TRANSACTION_FILE_NAME);
        if transaction_is_directory {
            fs::create_dir(&transaction_path).expect("transaction directory fixture is created");
        } else {
            fs::write(&transaction_path, b"also-not-json")
                .expect("corrupt transaction fixture is written");
        }

        assert_eq!(load_settings(temp.path()), Err(PrivacyStoreError));
    }
}

/// 验证 Windows 主文件提交失败时保留完整事务，使旧快照和恢复源都不丢失。
#[cfg(windows)]
#[test]
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Windows-only fixture cleanup cannot make Unix files world-writable"
)]
fn windows_commit_failure_retains_recovery_transaction() {
    let temp = tempdir().expect("isolated app-data is available");
    let baseline = sample_settings(false, None, true, false, false);
    save_settings(temp.path(), &baseline).expect("baseline settings are stored");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    let mut permissions = fs::metadata(&settings_path)
        .expect("baseline metadata is readable")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&settings_path, permissions).expect("primary is made read-only");
    let replacement = sample_settings(true, None, true, true, true);

    assert_eq!(
        save_settings(temp.path(), &replacement),
        Err(PrivacyStoreError)
    );
    assert!(temp.path().join(SETTINGS_TRANSACTION_FILE_NAME).is_file());

    let mut permissions = fs::metadata(&settings_path)
        .expect("read-only metadata is readable")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(&settings_path, permissions).expect("fixture permissions are restored");

    let retry_username = DeviceUsername::from_setting_input("retry")
        .expect("retry fixture is valid")
        .expect("retry fixture is non-empty");
    let retry = sample_settings(false, Some(retry_username), true, true, false);
    save_settings(temp.path(), &retry).expect("retry replaces the recovery transaction");
    assert_eq!(load_settings(temp.path()), Ok(retry));
    assert!(!temp.path().join(SETTINGS_TRANSACTION_FILE_NAME).exists());
}

/// 验证提交前放弃临时写入不会截断或替换磁盘上的旧完整快照。
#[cfg(unix)]
#[test]
fn abandoned_atomic_write_preserves_previous_snapshot() {
    let temp = tempdir().expect("isolated app-data is available");
    let settings = sample_settings(false, None, true, true, true);
    save_settings(temp.path(), &settings).expect("baseline settings are stored");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);

    {
        let mut writer =
            super::write::open_settings_writer(&settings_path).expect("temporary writer opens");
        writer
            .write_all(br#"{"localOnly":true"#)
            .expect("partial replacement is written only to the temporary file");
    }

    assert_eq!(load_settings(temp.path()), Ok(settings));
    assert_eq!(
        fs::read_dir(temp.path())
            .expect("app-data directory remains readable")
            .count(),
        1,
        "discarded temporary file must be cleaned up"
    );
}

/// 验证每次原子替换都会把 Unix 设置权限收敛为当前用户读写。
#[cfg(unix)]
#[test]
fn atomic_update_restores_private_unix_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("isolated app-data is available");
    let mut settings = sample_settings(false, None, true, true, false);
    save_settings(temp.path(), &settings).expect("baseline settings are stored");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    fs::set_permissions(&settings_path, fs::Permissions::from_mode(0o644))
        .expect("fixture permissions are broadened");

    settings.local_only = true;
    save_settings(temp.path(), &settings).expect("settings update is atomically committed");

    let mode = fs::metadata(&settings_path)
        .expect("committed settings metadata is readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(load_settings(temp.path()), Ok(settings));
}

/// 验证稳定目标符号链接会在创建临时文件前被拒绝，且链接目标不被修改。
#[cfg(unix)]
#[test]
fn rejects_symlink_settings_target_without_touching_destination() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("isolated app-data is available");
    let sentinel = temp.path().join("sentinel.json");
    fs::write(&sentinel, b"sentinel remains unchanged").expect("sentinel is written");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    symlink(&sentinel, &settings_path).expect("settings symlink fixture is created");

    let settings = sample_settings(false, None, true, true, true);
    assert_eq!(
        save_settings(temp.path(), &settings),
        Err(PrivacyStoreError)
    );
    assert_eq!(
        fs::read(&sentinel).expect("sentinel remains readable"),
        b"sentinel remains unchanged"
    );
    assert!(
        fs::symlink_metadata(&settings_path)
            .expect("settings link remains present")
            .file_type()
            .is_symlink()
    );
}

/// 验证检查后发生的目标链接替换仍只会被原子 rename 覆盖，不会写入链接目的地。
#[cfg(unix)]
#[test]
fn atomic_commit_does_not_follow_racing_destination_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("isolated app-data is available");
    let settings_path = temp.path().join(SETTINGS_FILE_NAME);
    fs::write(&settings_path, b"old snapshot").expect("baseline path is created");
    let mut writer =
        super::write::open_settings_writer(&settings_path).expect("temporary writer opens");
    writer
        .write_all(b"new snapshot")
        .expect("replacement is written to the temporary file");

    let sentinel = temp.path().join("sentinel.json");
    fs::write(&sentinel, b"sentinel remains unchanged").expect("sentinel is written");
    fs::remove_file(&settings_path).expect("baseline path is removed for race fixture");
    symlink(&sentinel, &settings_path).expect("racing symlink is installed");
    writer.commit().expect("atomic replacement commits");

    assert_eq!(
        fs::read(&sentinel).expect("sentinel remains readable"),
        b"sentinel remains unchanged"
    );
    assert_eq!(
        fs::read(&settings_path).expect("replacement path is readable"),
        b"new snapshot"
    );
    assert!(
        !fs::symlink_metadata(&settings_path)
            .expect("replacement metadata is readable")
            .file_type()
            .is_symlink()
    );
}
