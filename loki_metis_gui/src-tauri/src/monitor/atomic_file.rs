//! 监控配置文件的同目录原子替换边界。

use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use loki_metis_core::HookError;
use tempfile::NamedTempFile;

/// 完整写入临时文件并原子替换目标，避免读者观察到半份 JSON。
pub(super) fn write_monitor_file_atomically(
    path: &Path,
    payload: &[u8],
    error_key: &'static str,
) -> Result<(), HookError> {
    write_monitor_file_atomically_with_cancellation(path, payload, error_key, None)
}

/// 写入临时文件时观察取消，并在不可逆的原子替换前执行最后一次提交门禁。
pub(super) fn write_monitor_file_atomically_with_cancellation(
    path: &Path,
    payload: &[u8],
    error_key: &'static str,
    cancellation: Option<&AtomicBool>,
) -> Result<(), HookError> {
    write_monitor_file_atomically_with_commit_observer(
        path,
        payload,
        error_key,
        cancellation,
        || {},
    )
}

/// 在最终提交门禁前调用 observer，供回归精确模拟超时竞态。
fn write_monitor_file_atomically_with_commit_observer(
    path: &Path,
    payload: &[u8],
    error_key: &'static str,
    cancellation: Option<&AtomicBool>,
    before_commit: impl FnOnce(),
) -> Result<(), HookError> {
    ensure_atomic_write_not_cancelled(cancellation, error_key)?;
    let parent = path.parent().ok_or_else(|| {
        HookError::new(error_key).param("detail", "monitor file has no parent directory")
    })?;
    let mut file = NamedTempFile::new_in(parent)
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    file.write_all(payload)
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    ensure_atomic_write_not_cancelled(cancellation, error_key)?;
    file.as_file_mut()
        .sync_all()
        .map_err(|error| HookError::new(error_key).param("detail", error.to_string()))?;
    before_commit();
    ensure_atomic_write_not_cancelled(cancellation, error_key)?;
    file.persist(path)
        .map(|_| ())
        .map_err(|error| HookError::new(error_key).param("detail", error.error.to_string()))
}

/// 取消后的临时文件由 `NamedTempFile` Drop 清理，目标文件保持提交前内容。
fn ensure_atomic_write_not_cancelled(
    cancellation: Option<&AtomicBool>,
    error_key: &'static str,
) -> Result<(), HookError> {
    if cancellation.is_some_and(|token| token.load(Ordering::Acquire)) {
        return Err(HookError::new(error_key)
            .param("detail", "monitor file write was cancelled before commit"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 最终门禁观察到取消时只能删除临时文件，不能替换既有目标。
    #[test]
    fn cancellation_at_commit_boundary_preserves_target() {
        let root = tempdir().expect("temp directory");
        let target = root.path().join("hooks.json");
        std::fs::write(&target, b"before").expect("seed target");
        let cancellation = AtomicBool::new(false);

        let error = write_monitor_file_atomically_with_commit_observer(
            &target,
            b"after",
            "error.hooks.writeFailed",
            Some(&cancellation),
            || cancellation.store(true, Ordering::Release),
        )
        .expect_err("cancelled commit must fail closed");

        assert_eq!(
            error.params.get("detail").map(String::as_str),
            Some("monitor file write was cancelled before commit")
        );
        assert_eq!(std::fs::read(&target).expect("preserved target"), b"before");
        assert_eq!(
            std::fs::read_dir(root.path())
                .expect("directory listing")
                .count(),
            1,
            "temporary file must be removed on cancellation"
        );
    }
}
