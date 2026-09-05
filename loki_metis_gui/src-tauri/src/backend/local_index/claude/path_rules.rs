//! Claude transcript 相对路径与文件名规则。

use std::path::Path;

use loki_metis_core::{LocalError, LocalErrorKind};

/// 只允许 Claude 官方 transcript 的主会话与 subagent 两种相对层级。
pub(super) fn validate_transcript_path(
    root_path: &Path,
    file_path: &Path,
) -> Result<(), LocalError> {
    let relative = file_path.strip_prefix(root_path).map_err(|_| {
        LocalError::new(
            LocalErrorKind::InvalidPath,
            "Claude transcript escaped its root",
        )
    })?;
    let parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    let valid_main = parts.len() == 3 && parts[0] == "projects" && is_uuid_jsonl_name(&parts[2]);
    let valid_subagent = parts.len() == 5
        && parts[0] == "projects"
        && is_uuid(&parts[2])
        && parts[3] == "subagents"
        && is_subagent_jsonl_name(&parts[4]);
    if valid_main || valid_subagent {
        Ok(())
    } else {
        Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "Claude transcript layout is unsupported",
        ))
    }
}

/// 判定文件名是否为 `<UUID>.jsonl` 形式的主 transcript 命名。
pub(super) fn is_uuid_jsonl_name(value: &str) -> bool {
    value.strip_suffix(".jsonl").is_some_and(is_uuid)
}

/// Claude 子代理 transcript 只接受官方 `agent-<安全标识>.jsonl` 文件名。
pub(super) fn is_subagent_jsonl_name(value: &str) -> bool {
    value
        .strip_prefix("agent-")
        .and_then(|value| value.strip_suffix(".jsonl"))
        .is_some_and(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

/// 判定字符串是否符合标准 UUID（含连字符）格式。
pub(super) fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
