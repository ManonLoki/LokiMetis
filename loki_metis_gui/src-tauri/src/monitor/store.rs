//! 本机 Hook 配置目录探测与原子写入。

use std::path::{Path, PathBuf};

use loki_metis_core::{
    AiTool, HookConfigDirectories, HookConfigLocation, HookConfigWriteResult, HookError,
    generate_hook_config, hook_config_filename, hook_config_write_result, merge_hook_config,
};

use super::settings::MonitorSettings;

/// 探测用户主目录。
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 读取环境变量覆盖的绝对配置目录。
fn detected_config_directory(variable: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or(fallback)
}

/// 四项 Agent 的默认配置根目录。
fn default_directory(tool: AiTool, home: &Path) -> PathBuf {
    match tool {
        AiTool::Codex => detected_config_directory("CODEX_HOME", home.join(".codex")),
        AiTool::ClaudeCode => detected_config_directory("CLAUDE_CONFIG_DIR", home.join(".claude")),
        AiTool::Grok => detected_config_directory("GROK_HOME", home.join(".grok")),
        AiTool::WorkBuddy => home.join(".workbuddy"),
    }
}

/// 解析某工具最终配置定位。
fn location_for(tool: AiTool, directories: &HookConfigDirectories) -> HookConfigLocation {
    let custom = directories.get(tool).trim();
    let (directory, is_custom) = if custom.is_empty() {
        (default_directory(tool, &home_dir()), false)
    } else {
        (PathBuf::from(custom), true)
    };
    let filename = hook_config_filename(tool);
    let config_path = directory.join(filename);
    HookConfigLocation {
        tool,
        directory: directory.to_string_lossy().into_owned(),
        config_path: config_path.to_string_lossy().into_owned(),
        is_custom,
    }
}

/// 列出四项 Agent 的 Hook 配置定位。
pub fn list_hook_config_locations(settings: &MonitorSettings) -> Vec<HookConfigLocation> {
    AiTool::ALL
        .into_iter()
        .map(|tool| location_for(tool, &settings.hook_directories))
        .collect()
}

/// 为指定工具生成、合并并写入本机 Hook 配置。
pub fn write_hook_config(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    let location = location_for(tool, &settings.hook_directories);
    let config_path = PathBuf::from(&location.config_path);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
    }
    let existing = if config_path.exists() {
        Some(std::fs::read_to_string(&config_path).map_err(|error| {
            HookError::new("error.hooks.existingReadFailed").param("detail", error.to_string())
        })?)
    } else {
        None
    };
    let generated = generate_hook_config(tool, relay_executable)?;
    let merged = merge_hook_config(existing.as_deref(), &generated, tool)?;
    let changed = existing.as_deref() != Some(merged.content.as_str());
    if changed {
        std::fs::write(&config_path, &merged.content).map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
    }
    Ok(hook_config_write_result(tool, merged.filename, changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::MonitorSettings;
    use tempfile::tempdir;

    #[test]
    fn write_codex_hook_merges_managed_marker_and_rejects_cursor() {
        let root = tempdir().expect("temp");
        let mut settings = MonitorSettings::default();
        settings
            .hook_directories
            .set(AiTool::Codex, root.path().join("codex").to_string_lossy().into_owned());
        let first = write_hook_config(&settings, AiTool::Codex, Path::new("/opt/LokiMetis/loki_metis_gui"))
            .expect("write");
        assert!(first.config_changed);
        assert_eq!(first.outcome, loki_metis_core::HookWriteOutcome::CodexReviewRequired);
        let location = list_hook_config_locations(&settings)
            .into_iter()
            .find(|item| item.tool == AiTool::Codex)
            .expect("location");
        let content = std::fs::read_to_string(&location.config_path).expect("read");
        assert!(content.contains("LokiMetis:tool=codex"));
        assert!(content.contains("--loki-metis-hook-relay"));
        assert!(!content.to_lowercase().contains("cursor"));
        let second = write_hook_config(&settings, AiTool::Codex, Path::new("/opt/LokiMetis/loki_metis_gui"))
            .expect("rewrite");
        assert!(!second.config_changed);
    }
}
