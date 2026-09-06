//! Hook 配置生命周期的定向回归测试。

use super::*;
use tempfile::tempdir;

/// 非阻塞检查常驻 Hook 锁当前是否已由 worker 释放。
fn hook_config_lock_is_available(directory: &Path) -> bool {
    let path = directory.join(".lokimetis-hook-config.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            true
        }
        Err(std::fs::TryLockError::WouldBlock | std::fs::TryLockError::Error(_)) => false,
    }
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

#[test]
/// 已有管理标识不能掩盖缺失事件或移动后的旧可执行路径。
fn managed_config_repairs_stale_executable_and_missing_event() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Codex, &root.path().join("codex"));
    let first = write_hook_config(
        &settings,
        AiTool::Codex,
        Path::new("/old/LokiMetis"),
        root.path(),
    )
    .expect("first write");
    assert!(first.config_changed);
    let config_path = PathBuf::from(&first.filename);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).expect("read first"))
            .expect("valid JSON");
    value["hooks"]
        .as_object_mut()
        .expect("hooks object")
        .remove("Stop");
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&value).expect("serialize stale config"),
    )
    .expect("write stale config");

    let repaired = write_hook_config(
        &settings,
        AiTool::Codex,
        Path::new("/new/LokiMetis"),
        root.path(),
    )
    .expect("repair");
    assert!(repaired.config_changed);
    let repaired_content = std::fs::read_to_string(config_path).expect("read repaired");
    assert!(repaired_content.contains("/new/LokiMetis"));
    assert!(!repaired_content.contains("/old/LokiMetis"));
    let repaired_value: serde_json::Value =
        serde_json::from_str(&repaired_content).expect("repaired JSON");
    assert!(repaired_value["hooks"].get("Stop").is_some());
}

#[test]
/// 自动补写只处理启用项，并保留已取消选择工具的现有文件。
fn automatic_repair_only_touches_enabled_tools_and_never_deletes_disabled_config() {
    let root = tempdir().expect("temp");
    let codex_dir = root.path().join("codex");
    let claude_dir = root.path().join("claude");
    let mut settings = settings_for(AiTool::Codex, &codex_dir);
    settings.hook_directories.set(
        AiTool::ClaudeCode,
        claude_dir.to_string_lossy().into_owned(),
    );
    std::fs::create_dir_all(&claude_dir).expect("claude dir");
    let claude_path = claude_dir.join(hook_config_filename(AiTool::ClaudeCode));
    std::fs::write(&claude_path, "user-owned-disabled-config").expect("claude config");

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetis"), root.path());

    assert!(codex_dir.join(hook_config_filename(AiTool::Codex)).exists());
    assert_eq!(
        std::fs::read_to_string(claude_path).expect("disabled config"),
        "user-owned-disabled-config"
    );
}

#[test]
/// 自动补写边界不能信任内存设置直传值，隐藏协议即使被塞入集合也不得创建配置。
fn automatic_repair_ignores_direct_hidden_tool_input() {
    let root = tempdir().expect("temp");
    let open_code_dir = root.path().join("opencode");
    let settings = settings_for(AiTool::OpenCode, &open_code_dir);

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetis"), root.path());

    assert!(!open_code_dir.exists());
}

#[test]
/// 显式空选择不会产生写入，也不会清理此前配置。
fn empty_enabled_set_performs_no_write_or_delete() {
    let root = tempdir().expect("temp");
    let codex_dir = root.path().join("codex");
    let mut settings = settings_for(AiTool::Codex, &codex_dir);
    std::fs::create_dir_all(&codex_dir).expect("codex dir");
    let config_path = codex_dir.join(hook_config_filename(AiTool::Codex));
    std::fs::write(&config_path, "leave-me-alone").expect("existing config");
    settings.enabled_ai_tools.clear();

    repair_enabled_hook_configs(&settings, Path::new("/opt/LokiMetis"), root.path());

    assert_eq!(
        std::fs::read_to_string(config_path).expect("preserved config"),
        "leave-me-alone"
    );
}

#[test]
/// 自定义目录只接受空重置或指向目录的绝对路径。
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

#[test]
/// 即使相对路径来自旧持久设置，真正写入前仍必须拒绝。
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

#[test]
/// 相同 relay 与事件集合的重复写入保持幂等。
fn repeated_write_is_idempotent() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Codex, root.path());
    let executable = Path::new("/opt/LokiMetis/loki_metis_gui");
    assert!(
        write_hook_config(&settings, AiTool::Codex, executable, root.path())
            .expect("first")
            .config_changed
    );
    assert!(
        !write_hook_config(&settings, AiTool::Codex, executable, root.path())
            .expect("second")
            .config_changed
    );
}

#[test]
/// 独立插件的主文件和协议声明的辅助文件必须作为完整集合写入。
fn standalone_tools_write_every_declared_auxiliary_config() {
    for tool in [AiTool::Hermes, AiTool::OpenClaw] {
        let root = tempdir().expect("temp");
        let settings = settings_for(tool, root.path());
        let auxiliary = generate_hook_auxiliary_configs(tool);
        assert!(!auxiliary.is_empty(), "{tool:?} auxiliary contract");

        let result = write_hook_config(&settings, tool, Path::new("/opt/LokiMetis"), root.path())
            .expect("write standalone config set");

        assert!(result.config_changed);
        assert!(root.path().join(hook_config_filename(tool)).exists());
        for preview in auxiliary {
            assert!(
                root.path().join(preview.filename).exists(),
                "missing auxiliary file for {tool:?}"
            );
        }
    }
}

#[test]
/// 自动补写线程能够处理请求，并由应用生命周期显式回收。
fn owned_worker_processes_enabled_request_and_shuts_down() {
    let root = tempdir().expect("temp");
    let config_path = root.path().join(hook_config_filename(AiTool::Codex));
    let settings = settings_for(AiTool::Codex, root.path());
    let writer = HookConfigWriter::start(PathBuf::from("/opt/LokiMetis"), root.path().to_owned());

    writer.request_enabled(settings);
    for _ in 0..100 {
        if config_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    writer.shutdown();

    assert!(config_path.exists());
    assert!(
        std::fs::read_to_string(config_path)
            .expect("worker config")
            .contains("/opt/LokiMetis")
    );
}

#[test]
/// 外部 writer 在一次校正完全结束后最终覆盖时，低频自愈仍会恢复 marker 并保留外部内容。
fn owned_worker_eventually_repairs_a_post_verification_external_overwrite() {
    let root = tempdir().expect("temp");
    let config_path = root.path().join(hook_config_filename(AiTool::Codex));
    let settings = settings_for(AiTool::Codex, root.path());
    let writer = HookConfigWriter::start_with_interval(
        PathBuf::from("/opt/LokiMetis"),
        root.path().to_owned(),
        std::time::Duration::from_millis(200),
    );

    writer.request_enabled(settings);
    for _ in 0..200 {
        let initial_completed = std::fs::read_to_string(&config_path)
            .is_ok_and(|content| content.contains("LokiMetis:tool=codex"))
            && hook_config_lock_is_available(root.path());
        if initial_completed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        hook_config_lock_is_available(root.path()),
        "initial repair must release its file lock"
    );

    let external = r#"{
      "hooks": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "external-final-write"}]}]
      }
    }"#;
    std::fs::write(&config_path, external).expect("external final replacement");
    assert!(
        !std::fs::read_to_string(&config_path)
            .expect("external config")
            .contains("LokiMetis:tool=codex")
    );

    for _ in 0..400 {
        if std::fs::read_to_string(&config_path).is_ok_and(|content| {
            content.contains("LokiMetis:tool=codex")
                && content.contains("external-final-write")
                && hook_config_lock_is_available(root.path())
        }) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    writer.shutdown();

    let repaired = std::fs::read_to_string(config_path).expect("periodically repaired config");
    assert!(repaired.contains("LokiMetis:tool=codex"));
    assert!(repaired.contains("external-final-write"));
}

#[test]
/// 更新设置快照后只自愈新目录，旧目录即使丢失 marker 也不再被后台改写。
fn owned_worker_replaces_the_previous_directory_snapshot() {
    let root = tempdir().expect("temp");
    let old_directory = root.path().join("old");
    let new_directory = root.path().join("new");
    let old_path = old_directory.join(hook_config_filename(AiTool::Codex));
    let new_path = new_directory.join(hook_config_filename(AiTool::Codex));
    let writer = HookConfigWriter::start_with_interval(
        PathBuf::from("/opt/LokiMetis"),
        root.path().to_owned(),
        std::time::Duration::from_millis(100),
    );

    writer.request_enabled(settings_for(AiTool::Codex, &old_directory));
    for _ in 0..200 {
        if old_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(old_path.exists(), "old snapshot should be processed first");

    writer.request_enabled(settings_for(AiTool::Codex, &new_directory));
    for _ in 0..200 {
        if new_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        new_path.exists(),
        "new snapshot should be processed immediately"
    );

    let external_only = r#"{"hooks":{"SessionStart":[]},"owner":"external"}"#;
    std::fs::write(&old_path, external_only).expect("replace old config");
    std::thread::sleep(std::time::Duration::from_millis(350));
    writer.shutdown();

    assert_eq!(
        std::fs::read_to_string(old_path).expect("old config remains external"),
        external_only
    );
    assert!(
        std::fs::read_to_string(new_path)
            .expect("new config remains managed")
            .contains("LokiMetis:tool=codex")
    );
}

#[test]
/// 默认路径只基于调用方注入的 Tauri 主目录，而不是 store 自行猜测 HOME。
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
        cursor.directory,
        root.path().join(".cursor").to_string_lossy()
    );
    assert_eq!(
        cursor.config_path,
        root.path().join(".cursor/hooks.json").to_string_lossy()
    );
}

#[test]
/// 正常 Grok 写入与自动修复路径都会迁移旧共享文件，并把迁移计入变化结果。
fn grok_write_migrates_legacy_aimonitor_file_and_is_idempotent() {
    let root = tempdir().expect("temp");
    let settings = settings_for(AiTool::Grok, root.path());
    let executable = Path::new("/opt/LokiMetis");
    write_hook_config(&settings, AiTool::Grok, executable, root.path())
        .expect("seed current config");

    let legacy_path = root.path().join("hooks/aimonitor.json");
    let mut legacy: serde_json::Value = serde_json::from_str(
        &generate_hook_config(AiTool::Grok, Path::new("/opt/old-LokiMetis"))
            .expect("old config")
            .content,
    )
    .expect("old JSON");
    legacy["aimonitorOwner"] = serde_json::json!(true);
    legacy["hooks"]["SessionStart"]
        .as_array_mut()
        .expect("SessionStart groups")
        .push(serde_json::json!({
            "hooks": [{"type": "command", "command": "keep-aimonitor-handler"}]
        }));
    std::fs::write(
        &legacy_path,
        serde_json::to_string_pretty(&legacy).expect("legacy JSON"),
    )
    .expect("legacy config");

    let migrated = write_hook_config(&settings, AiTool::Grok, executable, root.path())
        .expect("migrate legacy config");
    assert!(migrated.config_changed);
    let migrated_content = std::fs::read_to_string(&legacy_path).expect("migrated legacy config");
    assert!(!migrated_content.contains("LokiMetis:tool=grok"));
    assert!(migrated_content.contains("keep-aimonitor-handler"));
    assert!(
        !write_hook_config(&settings, AiTool::Grok, executable, root.path())
            .expect("idempotent rewrite")
            .config_changed
    );
    assert_eq!(
        std::fs::read_to_string(legacy_path).expect("stable legacy config"),
        migrated_content
    );
}
