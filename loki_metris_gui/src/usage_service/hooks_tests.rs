//! 隔离Hooks宿主回归；所有工具都使用测试自定义目录，不依赖真实用户环境。

use super::*;
use crate::monitor::settings::{load_monitor_settings, update_monitor_settings};
use loki_metris_core::{HookWriteOutcome, hook_config_filename, public_monitor_ai_tools};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// 失败和正常结束都先关闭服务，再让临时目录释放。
struct Fixture {
    service: Arc<UsageService>,
    root: tempfile::TempDir,
}
impl Drop for Fixture {
    /// 释放临时目录前关闭测试服务。
    fn drop(&mut self) {
        self.service.finish();
    }
}
impl Fixture {
    /// 先关闭启用集合并覆盖全部公开根，再绑定真实串行writer。
    fn new() -> Self {
        Self::with_executable("/fixture/LokiMetris")
    }
    /// 以指定中继可执行路径绑定writer，用于核对写入配置里的命令形态。
    fn with_executable(executable: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("data");
        update_monitor_settings(&directory.join("monitor"), |settings| {
            settings.enabled_ai_tools.clear();
            for tool in public_monitor_ai_tools() {
                settings.hook_directories.set(
                    tool,
                    root.path()
                        .join(loki_metris_core::ai_tool_name(tool))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
            settings.pet_window.locked = true;
        })
        .unwrap();
        let service = UsageService::in_directory(directory).unwrap();
        service
            .start_hook_writer_at(
                PathBuf::from(executable),
                root.path().join("home"),
            )
            .unwrap();
        Self { service, root }
    }
    /// 外层回执在当前服务runtime中等待，文件工作仍由worker拥有。
    fn receive<T>(&self, receiver: oneshot::Receiver<T>) -> T {
        self.service
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .block_on(receiver)
            .unwrap()
    }
}

/// 保存仅替换对应目录；非法路径不发布，空值恢复真实默认定位。
#[test]
fn hook_directory_save_preserves_settings_and_rejects_hidden_tools() {
    let fixture = Fixture::new();
    let service = &fixture.service;
    let initial = fixture
        .receive(service.list_monitor_hook_locations().unwrap())
        .unwrap();
    assert_eq!(initial.len(), 5);
    let new_directory = fixture.root.path().join("new-codex");
    let saved = fixture
        .receive(
            service
                .save_hook_config_directory(AiTool::Codex, format!(" {} ", new_directory.display()))
                .unwrap(),
        )
        .unwrap();
    assert!(saved.is_custom);
    assert_eq!(saved.directory, new_directory.to_string_lossy());
    assert!(
        fixture
            .receive(
                service
                    .save_hook_config_directory(AiTool::Codex, "relative".into())
                    .unwrap()
            )
            .is_err()
    );
    assert!(
        fixture
            .receive(
                service
                    .save_hook_config_directory(
                        AiTool::OpenCode,
                        new_directory.to_string_lossy().into()
                    )
                    .unwrap()
            )
            .is_err()
    );
    let settings = load_monitor_settings(&service.context.directory.join("monitor")).unwrap();
    assert!(settings.pet_window.locked);
    assert!(settings.enabled_ai_tools.is_empty());
    assert_eq!(settings.hook_directories.codex, saved.directory);
    assert_eq!(service.agent_settings_revision(), 1);
    let reset = fixture
        .receive(
            service
                .save_hook_config_directory(AiTool::Cursor, String::new())
                .unwrap(),
        )
        .unwrap();
    assert!(!reset.is_custom);
    assert_eq!(
        Path::new(&reset.directory),
        fixture.root.path().join("home/.cursor")
    );
    assert!(!Path::new(&reset.config_path).exists());
}

/// 实际写入保留用户和原版受管内容，重复提交返回精确unchanged。
#[test]
fn explicit_hook_write_merges_and_reports_real_outcome() {
    let fixture = Fixture::new();
    let service = &fixture.service;
    let locations = fixture
        .receive(service.list_monitor_hook_locations().unwrap())
        .unwrap();
    let codex = locations
        .iter()
        .find(|location| location.tool == AiTool::Codex)
        .unwrap();
    let file = Path::new(&codex.config_path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file,r#"{"user":true,"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"old LokiMetis:tool=codex"}]}]}}"#).unwrap();
    let result = fixture
        .receive(service.write_monitor_hook_config(AiTool::Codex).unwrap())
        .unwrap();
    assert_eq!(result.outcome, HookWriteOutcome::CodexReviewRequired);
    let content = std::fs::read_to_string(file).unwrap();
    assert!(content.contains("LokiMetis:tool=codex"));
    assert!(content.contains("LokiMetris:tool=codex"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&content).unwrap()["user"],
        true
    );
    assert_eq!(
        fixture
            .receive(service.write_monitor_hook_config(AiTool::Codex).unwrap())
            .unwrap()
            .outcome,
        HookWriteOutcome::Unchanged
    );
    assert!(
        fixture
            .receive(service.write_monitor_hook_config(AiTool::OpenCode).unwrap())
            .is_err()
    );
}

/// 绑定后启用与改目录都发布完整快照；未提交路径不会被自动writer看到。
#[test]
fn automatic_hook_writer_observes_committed_directory_and_enabled_snapshot() {
    let fixture = Fixture::new();
    let service = &fixture.service;
    fixture
        .receive(
            service
                .save_enabled_ai_tools(vec![AiTool::ClaudeCode])
                .unwrap(),
        )
        .unwrap();
    let location = fixture
        .receive(service.list_monitor_hook_locations().unwrap())
        .unwrap()
        .into_iter()
        .find(|location| location.tool == AiTool::ClaudeCode)
        .unwrap();
    wait_file(Path::new(&location.config_path));
    let next = fixture.root.path().join("new-claude");
    let saved = fixture
        .receive(
            service
                .save_hook_config_directory(AiTool::ClaudeCode, next.to_string_lossy().into())
                .unwrap(),
        )
        .unwrap();
    wait_file(Path::new(&saved.config_path));
    let settings = fixture
        .receive(service.monitor_settings().unwrap())
        .unwrap();
    assert_eq!(
        settings.hook_directories.get(AiTool::ClaudeCode),
        saved.directory
    );
    assert_eq!(settings.enabled_ai_tools, vec![AiTool::ClaudeCode]);
    assert!(
        !fixture
            .root
            .path()
            .join(loki_metris_core::ai_tool_name(AiTool::Codex))
            .join(hook_config_filename(AiTool::Codex))
            .exists()
    );
}

/// 接受后立即退出仍由writer返回终态，不等待35秒期限或留下未登记写入。
#[test]
fn hook_write_followed_by_shutdown_has_owned_terminal_receipt() {
    let fixture = Fixture::new();
    let service = &fixture.service;
    let receipt = service.write_monitor_hook_config(AiTool::Grok).unwrap();
    let before = Instant::now();
    service.finish();
    assert!(before.elapsed() < Duration::from_secs(5));
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(receipt)
        .unwrap();
    if let Err(error) = result {
        assert_eq!(error.code, "error.hooks.writeFailed");
    }
    assert!(service.write_monitor_hook_config(AiTool::Codex).is_err());
    assert!(
        service
            .save_hook_config_directory(AiTool::Codex, String::new())
            .is_err()
    );
    assert!(service.list_monitor_hook_locations().is_err());
}

/// 最多等待真实文件工作五秒，超时必须失败。
fn wait_file(file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if std::fs::read_to_string(file).is_ok_and(|value| value.contains("LokiMetris:tool=")) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("hook file did not commit: {}", file.display());
}

/// 磁盘映像卷与 Gatekeeper 临时目录不能作为长期 Hook 命令的可执行路径。
#[test]
fn ephemeral_executable_paths_are_rejected_for_hook_commands() {
    for path in [
        "/Volumes/LokiMetris/LokiMetris.app/Contents/MacOS/loki_metris_gui",
        "/private/var/folders/ab/T/AppTranslocation/1234/d/LokiMetris.app/Contents/MacOS/loki_metris_gui",
    ] {
        assert!(is_ephemeral_executable(Path::new(path)), "{path}");
    }
    for path in [
        "/Applications/LokiMetris.app/Contents/MacOS/loki_metris_gui",
        "/Users/someone/Applications/LokiMetris.app/Contents/MacOS/loki_metris_gui",
        "C:\\Program Files\\LokiMetris\\loki_metris_gui.exe",
    ] {
        assert!(!is_ephemeral_executable(Path::new(path)), "{path}");
    }
}

/// 安装位置含中文与空格时，写入真实配置的命令必须带引号、使用本产品的中继参数与管理标识。
#[test]
fn installed_app_path_is_written_as_quoted_relay_command() {
    let executable = if cfg!(windows) { r"C:\Program Files\诡秘 神谕\loki_metris_gui.exe" }
        else { "/Applications/诡秘 神谕.app/Contents/MacOS/loki_metris_gui" };
    let fixture = Fixture::with_executable(executable);
    let service = &fixture.service;
    let receipt = service.write_monitor_hook_config(AiTool::ClaudeCode).unwrap();
    fixture.receive(receipt).unwrap();
    let file = fixture.root.path().join(loki_metris_core::ai_tool_name(AiTool::ClaudeCode)).join(hook_config_filename(AiTool::ClaudeCode));
    wait_file(&file);
    let content = std::fs::read_to_string(&file).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&content).unwrap();
    let commands: Vec<&str> = settings["hooks"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|groups| groups.as_array().unwrap())
        .flat_map(|group| group["hooks"].as_array().unwrap())
        .filter_map(|hook| hook["command"].as_str())
        .collect();
    assert!(!commands.is_empty());
    for command in commands {
        if cfg!(windows) {
            assert!(command.starts_with(&format!("cmd.exe /d /s /c \"\"{executable}\" --loki-metris-hook-relay \"claude-code\" ")), "{command}");
            assert!(command.ends_with("--managed-by \"LokiMetris:tool=claude-code\"\""), "{command}");
        } else {
            assert!(command.starts_with(&format!("'{executable}' --loki-metris-hook-relay 'claude-code' ")), "{command}");
            assert!(command.ends_with("--managed-by 'LokiMetris:tool=claude-code'"), "{command}");
        }
        assert!(!command.contains("/Volumes/"), "{command}");
    }
}
