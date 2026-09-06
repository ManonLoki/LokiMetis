//! 监控配置文件的同目录原子替换边界。

use std::{io::Write, path::Path};

use loki_metis_core::HookError;
use tempfile::NamedTempFile;

/// 完整写入临时文件并原子替换目标，避免读者观察到半份 JSON。
pub(super) fn write_monitor_file_atomically(
    path: &Path,
    payload: &[u8],
    error_key: &'static str,
) -> Result<(), HookError> {
    let parent = path.parent().ok_or_else(|| {
        HookError::new(error_key).param("detail", "monitor file has no parent directory")
    })?;
    let mut file = NamedTempFile::new_in(parent)
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    file.write_all(payload)
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    file.as_file_mut()
        .sync_all()
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    file.persist(path)
        .map(|_| ())
        .map_err(|error| HookError::new(error_key).param("detail", error.error.to_string()))
}
