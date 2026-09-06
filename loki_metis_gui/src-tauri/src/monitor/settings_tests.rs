//! 监控设置持久化与并发更新的定向回归测试。

use super::*;
use tempfile::tempdir;

#[test]
/// 已废弃的兔耳开关字段不会重新进入持久设置。
fn legacy_pet_close_control_setting_is_ignored_and_removed() {
    let root = tempdir().expect("temp");
    let config = root.path();
    std::fs::write(
        settings_path(config),
        r#"{"enabledAiTools":["codex"],"hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""},"petCloseControlVisible":false}"#,
    )
    .expect("write");
    let settings = load_monitor_settings(config).expect("load");
    assert_eq!(settings.enabled_ai_tools, vec![AiTool::Codex]);
    save_monitor_settings(config, &settings).expect("save");
    let saved = std::fs::read_to_string(settings_path(config)).expect("read");
    assert!(!saved.contains("petCloseControlVisible"));
}

#[test]
/// 旧设置缺少桌宠位置时继续使用运行时默认几何。
fn missing_pet_overlay_position_stays_unset() {
    let root = tempdir().expect("temp");
    let config = root.path();
    std::fs::write(
        settings_path(config),
        r#"{"enabledAiTools":["codex"],"hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""}}"#,
    )
    .expect("write");
    let settings = load_monitor_settings(config).expect("load");
    assert_eq!(settings.pet_overlay_position, None);
    assert_eq!(settings.pet_window, PetWindowPreferences::default());
}

#[test]
/// 用户显式清空启用项后，保存与重载都必须保持空集合。
fn explicitly_empty_enabled_tools_remain_empty_after_save_and_reload() {
    let root = tempdir().expect("temp");
    let config = root.path();
    let mut settings = MonitorSettings::default();
    settings.enabled_ai_tools.clear();
    let saved = save_monitor_settings(config, &settings).expect("save");
    assert!(saved.enabled_ai_tools.is_empty());
    assert!(
        load_monitor_settings(config)
            .expect("reload")
            .enabled_ai_tools
            .is_empty()
    );
}

#[test]
/// 旧文件缺失监控字段时使用源程序的三项默认启用集合和空目录覆盖。
fn missing_enabled_tools_and_directories_use_defaults() {
    let root = tempdir().expect("temp");
    std::fs::write(settings_path(root.path()), r#"{"petOverlayPosition":null}"#).expect("write");
    let settings = load_monitor_settings(root.path()).expect("load");
    assert_eq!(
        settings.enabled_ai_tools,
        vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
    );
    assert_eq!(
        serde_json::to_value(&settings.hook_directories).expect("directories"),
        serde_json::to_value(HookConfigDirectories::default()).expect("default directories")
    );
}

#[test]
/// 合法桌宠位置可以稳定落盘并重载。
fn saved_pet_overlay_position_round_trips() {
    let root = tempdir().expect("temp");
    let config = root.path();
    let mut settings = MonitorSettings::default();
    settings.pet_overlay_position = Some(PetOverlayPosition { x: 240, y: 90 });
    save_monitor_settings(config, &settings).expect("save");
    let loaded = load_monitor_settings(config).expect("load");
    assert_eq!(
        loaded.pet_overlay_position,
        Some(PetOverlayPosition { x: 240, y: 90 })
    );
}

#[test]
/// 桌宠窗口偏好落盘前按产品边界规范化。
fn pet_window_preferences_round_trip_and_normalize() {
    let root = tempdir().expect("temp");
    let config = root.path();
    let mut settings = MonitorSettings::default();
    settings.pet_window = PetWindowPreferences {
        layout: PetLayout::Row3,
        focused_slot: 200,
        pet_size: 1,
        always_on_top: false,
        locked: true,
    };
    let saved = save_monitor_settings(config, &settings).expect("save");
    assert_eq!(saved.pet_window.focused_slot, 11);
    assert_eq!(saved.pet_window.pet_size, 32);
    let loaded = load_monitor_settings(config).expect("load");
    assert_eq!(loaded.pet_window.layout, PetLayout::Row3);
    assert!(!loaded.pet_window.always_on_top);
    assert!(loaded.pet_window.locked);
}

#[test]
/// 单个损坏的桌宠字段只恢复自身默认，不连坐其他合法字段。
fn malformed_pet_window_fields_use_individual_defaults() {
    let root = tempdir().expect("temp");
    let config = root.path();
    std::fs::write(
        settings_path(config),
        r#"{
            "enabledAiTools":["codex"],
            "hookDirectories":{"codex":"","claudeCode":"","grok":"","workBuddy":""},
            "petWindow":{"layout":"row3","focusedSlot":"bad","petSize":"bad","alwaysOnTop":"bad","locked":true}
        }"#,
    )
    .expect("write");
    let settings = load_monitor_settings(config).expect("load");
    assert_eq!(settings.pet_window.layout, PetLayout::Row3);
    assert_eq!(settings.pet_window.focused_slot, 0);
    assert_eq!(settings.pet_window.pet_size, 64);
    assert!(settings.pet_window.always_on_top);
    assert!(settings.pet_window.locked);
}

#[test]
/// 整个设置文件损坏时主界面仍收到错误，而桌宠运行时可用默认偏好恢复显示。
fn malformed_settings_keep_pet_runtime_recoverable_without_hiding_the_error() {
    let root = tempdir().expect("temp");
    std::fs::write(settings_path(root.path()), b"{").expect("write");

    assert!(load_monitor_settings(root.path()).is_err());
    assert_eq!(
        load_pet_runtime_settings(root.path()).pet_window,
        PetWindowPreferences::default()
    );
}

/// 第二个偏好更新必须等待第一个完整提交，并在其结果上继续修改。
#[test]
fn concurrent_updates_preserve_fields_written_by_both_callers() {
    use std::{
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    let root = tempdir().expect("temp");
    let config = Arc::new(root.path().to_path_buf());
    save_monitor_settings(&config, &MonitorSettings::default()).expect("initial settings");
    let (first_entered_tx, first_entered_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let (second_ready_tx, second_ready_rx) = mpsc::channel();
    let (second_entered_tx, second_entered_rx) = mpsc::channel();
    let first_config = Arc::clone(&config);
    let first = thread::spawn(move || {
        update_monitor_settings(&first_config, |settings| {
            first_entered_tx.send(()).expect("first entered");
            release_first_rx.recv().expect("release first");
            settings.pet_window.locked = true;
        })
        .expect("first update");
    });
    first_entered_rx.recv().expect("first writer entered");
    let second_config = Arc::clone(&config);
    let second = thread::spawn(move || {
        second_ready_tx.send(()).expect("second ready");
        update_monitor_settings(&second_config, |settings| {
            second_entered_tx.send(()).expect("second entered");
            settings.pet_window.always_on_top = false;
        })
        .expect("second update");
    });
    second_ready_rx.recv().expect("second writer started");
    assert_eq!(
        second_entered_rx.recv_timeout(Duration::from_millis(200)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    release_first_tx.send(()).expect("release first writer");
    first.join().expect("first thread");
    second_entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second writer entered after first commit");
    second.join().expect("second thread");

    let saved = load_monitor_settings(&config).expect("saved settings");
    assert!(saved.pet_window.locked);
    assert!(!saved.pet_window.always_on_top);
}
