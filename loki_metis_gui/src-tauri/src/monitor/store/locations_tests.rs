//! Hook 配置目录定位与校验的定向回归测试。

use std::path::{Path, PathBuf};

use loki_metis_core::ai_tool_name;
use tempfile::tempdir;

use super::*;

/// 为指定工具生成、校验并原子写入本机配置，供目录定位回归复用。
fn write_hook_config(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<loki_metis_core::HookConfigWriteResult, HookError> {
    super::super::write_hook_config_with_cancellation(
        settings,
        tool,
        relay_executable,
        home_directory,
        None,
    )
}

/// 构造把指定工具隔离到临时目录的监控设置。
fn settings_for(tool: AiTool, directory: &Path) -> MonitorSettings {
    let mut settings = MonitorSettings::default();
    settings.enabled_ai_tools = vec![tool];
    settings
        .hook_directories
        .set(tool, directory.to_string_lossy().into_owned());
    settings
}

/// 自定义目录只接受空重置或指向目录的绝对路径。
#[test]
fn custom_directory_requires_an_absolute_directory_or_empty_reset() {
    let root = tempdir().expect("temp");
    assert_eq!(validate_hook_config_directory("  ").expect("empty"), "");
    assert_eq!(
        validate_hook_config_directory(&format!("  {}  ", root.path().display()))
            .expect("absolute directory"),
        root.path().to_string_lossy()
    );
    assert_eq!(
        validate_hook_config_directory("relative/hooks")
            .expect_err("relative path")
            .code,
        "error.hooks.directoryNotAbsolute"
    );
    let file = root.path().join("not-a-directory");
    std::fs::write(&file, "file").expect("file");
    assert_eq!(
        validate_hook_config_directory(&file.to_string_lossy())
            .expect_err("file path")
            .code,
        "error.hooks.directoryNotAFolder"
    );
}

/// 即使相对路径来自旧持久设置，真正写入前仍必须拒绝。
#[test]
fn write_rejects_relative_directory_loaded_from_persisted_settings() {
    let mut settings = MonitorSettings::default();
    settings
        .hook_directories
        .set(AiTool::Codex, "relative/from-old-settings".to_owned());
    let error = write_hook_config(
        &settings,
        AiTool::Codex,
        Path::new("/opt/LokiMetis"),
        Path::new("/home/test"),
    )
    .expect_err("relative persisted path");
    assert_eq!(error.code, "error.hooks.directoryNotAbsolute");
    assert!(!Path::new("relative/from-old-settings/hooks.json").exists());
}

/// 默认路径只基于调用方注入的 Tauri 主目录，而不是 store 自行猜测 HOME。
#[test]
fn default_locations_use_the_injected_tauri_home_directory() {
    let root = tempdir().expect("temp");
    let settings = MonitorSettings::default();
    let locations = list_hook_config_locations(&settings, root.path());
    assert_eq!(
        locations
            .iter()
            .map(|location| location.tool)
            .collect::<Vec<_>>(),
        vec![
            AiTool::Codex,
            AiTool::ClaudeCode,
            AiTool::Cursor,
            AiTool::Grok,
            AiTool::WorkBuddy,
        ]
    );
    assert!(
        locations
            .iter()
            .all(|location| location.tool != AiTool::OpenCode)
    );
    let cursor = locations
        .into_iter()
        .find(|location| location.tool == AiTool::Cursor)
        .expect("cursor location");
    assert_eq!(
        PathBuf::from(&cursor.directory),
        root.path().join(".cursor")
    );
    assert_eq!(
        PathBuf::from(&cursor.config_path),
        root.path().join(".cursor").join("hooks.json")
    );
}

/// WorkBuddy 国内版与国际版是两个固定安装目录，默认写入必须同时维护两份配置。
#[test]
fn workbuddy_default_write_owns_both_fixed_edition_roots() {
    let root = tempdir().expect("temp");
    let settings = MonitorSettings::default();
    let location = list_hook_config_locations(&settings, root.path())
        .into_iter()
        .find(|location| location.tool == AiTool::WorkBuddy)
        .expect("workbuddy location");
    assert!(!location.is_custom);
    assert_eq!(
        PathBuf::from(&location.directory),
        root.path().join(".workbuddy")
    );
    assert_eq!(
        location.additional_config_paths,
        vec![
            root.path()
                .join(".workbuddy-ai")
                .join(hook_config_filename(AiTool::WorkBuddy))
                .to_string_lossy()
                .into_owned()
        ]
    );

    let result = write_hook_config(
        &settings,
        AiTool::WorkBuddy,
        Path::new("/opt/LokiMetis"),
        root.path(),
    )
    .expect("workbuddy write");
    assert!(result.config_changed);

    for directory in [
        root.path().join(".workbuddy"),
        root.path().join(".workbuddy-ai"),
    ] {
        let written =
            std::fs::read_to_string(directory.join(hook_config_filename(AiTool::WorkBuddy)))
                .expect("managed config exists in every fixed edition root");
        assert!(written.contains("LokiMetis:tool=workbuddy"));
    }
}

/// 用户为 WorkBuddy 指定自定义目录后只写入该位置，不再附带任何固定根。
#[test]
fn workbuddy_custom_directory_replaces_both_fixed_roots() {
    let root = tempdir().expect("temp");
    let custom = root.path().join("custom-workbuddy");
    let settings = settings_for(AiTool::WorkBuddy, &custom);
    let location = list_hook_config_locations(&settings, root.path())
        .into_iter()
        .find(|location| location.tool == AiTool::WorkBuddy)
        .expect("workbuddy location");
    assert!(location.is_custom);
    assert_eq!(PathBuf::from(&location.directory), custom);
    assert!(location.additional_config_paths.is_empty());

    write_hook_config(
        &settings,
        AiTool::WorkBuddy,
        Path::new("/opt/LokiMetis"),
        root.path(),
    )
    .expect("custom workbuddy write");

    assert!(
        custom
            .join(hook_config_filename(AiTool::WorkBuddy))
            .exists()
    );
    assert!(!root.path().join(".workbuddy").exists());
    assert!(!root.path().join(".workbuddy-ai").exists());
}

/// 只有 WorkBuddy 有两个固定配置根，其它工具仍只有一个且没有附加路径。
#[test]
fn other_tools_keep_a_single_fixed_config_root() {
    let root = tempdir().expect("temp");
    let settings = MonitorSettings::default();
    let locations = list_hook_config_locations(&settings, root.path());
    assert_eq!(
        locations
            .iter()
            .filter(|location| location.tool == AiTool::WorkBuddy)
            .count(),
        1
    );
    for location in locations {
        if location.tool == AiTool::WorkBuddy {
            continue;
        }
        assert!(
            location.additional_config_paths.is_empty(),
            "{} unexpectedly declares additional config paths",
            ai_tool_name(location.tool)
        );
    }
}
