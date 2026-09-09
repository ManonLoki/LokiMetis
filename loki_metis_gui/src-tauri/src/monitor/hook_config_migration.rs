//! 旧 Hook 配置迁移；只移除 LokiMetis 自己的受管条目并保留外部内容。

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use loki_metis_core::{AiTool, HookError, remove_managed_hook_entries};

use super::{
    hook_config_io::HookConfigFileLock,
    wsl::{WslDirectory, WslFile},
};

/// 旧 Loki 版本曾与 AIMonitor 共用的 Grok 配置相对路径。
const LEGACY_GROK_CONFIG: &str = "hooks/aimonitor.json";
/// 纯 Loki 旧文件清理后保留明确有效的空 JSON，避免 read 后 unlink 误删外部替换。
const EMPTY_LEGACY_CONFIG: &str = "{}";
/// 外部 writer 竞争时最多从最新内容重新清理四轮。
const MIGRATION_ROUNDS: usize = 4;
/// 每次替换前后观察短暂静默，缩小未采用 Loki 锁的外部 writer 丢更新窗口。
const MIGRATION_SETTLE: Duration = Duration::from_millis(25);

/// 旧配置文件的最小读写接口，本机路径与 WSL 文件共享同一校正算法。
trait LegacyConfigFile {
    /// 读取当前内容；文件不存在表示无需迁移。
    fn read_optional(&self) -> Result<Option<String>, HookError>;

    /// 用同目录原子替换提交已清理内容。
    fn write_atomic(
        &self,
        content: &str,
        cancellation: Option<&AtomicBool>,
    ) -> Result<(), HookError>;
}

impl LegacyConfigFile for PathBuf {
    /// 读取本机旧文件并区分正常缺失与其他 I/O 错误。
    fn read_optional(&self) -> Result<Option<String>, HookError> {
        match fs::read_to_string(self) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(HookError::new("error.hooks.existingReadFailed")
                    .param("detail", error.to_string()))
            }
        }
    }

    /// 原子替换本机旧文件；其父目录通常已由当前 Grok 配置写入创建。
    fn write_atomic(
        &self,
        content: &str,
        cancellation: Option<&AtomicBool>,
    ) -> Result<(), HookError> {
        ensure_migration_not_cancelled(cancellation)?;
        if let Some(parent) = self.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
            })?;
        }
        super::atomic_file::write_monitor_file_atomically_with_cancellation(
            self,
            content.as_bytes(),
            "error.hooks.writeFailed",
            cancellation,
        )
    }
}

impl LegacyConfigFile for WslFile {
    /// 复用 WSL 窄适配层读取旧文件。
    fn read_optional(&self) -> Result<Option<String>, HookError> {
        WslFile::read_optional(self)
    }

    /// 复用 WSL 同目录临时文件加 mv 的原子替换。
    fn write_atomic(
        &self,
        content: &str,
        _cancellation: Option<&AtomicBool>,
    ) -> Result<(), HookError> {
        WslFile::write_atomic(self, content)
    }
}

/// 在配置根锁内迁移本机 Grok 旧文件，并把迁移变化合并进写入结果。
#[cfg(test)]
pub(super) fn migrate_legacy_grok_local(directory: &Path) -> Result<bool, HookError> {
    migrate_legacy_grok_local_with_cancellation(directory, None)
}

/// 本机 Grok 迁移在锁等待、每轮读写与原子提交前观察显式写入取消。
pub(super) fn migrate_legacy_grok_local_with_cancellation(
    directory: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, HookError> {
    let file = directory.join(LEGACY_GROK_CONFIG);
    ensure_migration_not_cancelled(cancellation)?;
    if !legacy_grok_file_needs_cleanup(&file, cancellation)? {
        return Ok(false);
    }
    let _lock = HookConfigFileLock::acquire_with_cancellation(directory, cancellation)?;
    reconcile_legacy_grok_file_with_cancellation(&file, cancellation, |_, _| {})
}

/// 在 WSL 中迁移 Grok 旧文件；跨进程保护由有限写后重读提供。
pub(super) fn migrate_legacy_grok_wsl(directory: &WslDirectory) -> Result<bool, HookError> {
    let file = directory.join(LEGACY_GROK_CONFIG);
    if !legacy_grok_file_needs_cleanup(&file, None)? {
        return Ok(false);
    }
    reconcile_legacy_grok_file_with_cancellation(&file, None, |_, _| {})
}

/// 无 marker 的稳定旧文件无需进入锁、等待或原子替换路径。
fn legacy_grok_file_needs_cleanup(
    file: &impl LegacyConfigFile,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, HookError> {
    ensure_migration_not_cancelled(cancellation)?;
    let Some(content) = file.read_optional()? else {
        return Ok(false);
    };
    ensure_migration_not_cancelled(cancellation)?;
    Ok(cleaned_grok_content(&content)? != content)
}

/// 每轮从最新内容重新移除 Grok marker；observer 仅用于确定性并发回归。
#[cfg(test)]
fn reconcile_legacy_grok_file(
    file: &impl LegacyConfigFile,
    observer: impl FnMut(usize, &str),
) -> Result<bool, HookError> {
    reconcile_legacy_grok_file_with_cancellation(file, None, observer)
}

/// 迁移实际循环在读、静默和提交边界停止已取消请求。
fn reconcile_legacy_grok_file_with_cancellation(
    file: &impl LegacyConfigFile,
    cancellation: Option<&AtomicBool>,
    mut observer: impl FnMut(usize, &str),
) -> Result<bool, HookError> {
    let mut wrote_any = false;
    for round in 0..MIGRATION_ROUNDS {
        ensure_migration_not_cancelled(cancellation)?;
        let Some(before) = file.read_optional()? else {
            return Ok(wrote_any);
        };
        ensure_migration_not_cancelled(cancellation)?;
        let desired = cleaned_grok_content(&before)?;
        if desired == before {
            return Ok(wrote_any);
        }
        settle_migration_with_cancellation(cancellation)?;
        if file.read_optional()?.as_deref() != Some(before.as_str()) {
            continue;
        }

        ensure_migration_not_cancelled(cancellation)?;
        file.write_atomic(&desired, cancellation)?;
        ensure_migration_not_cancelled(cancellation)?;
        wrote_any = true;
        observer(round, &desired);
        settle_migration_with_cancellation(cancellation)?;

        let Some(after) = file.read_optional()? else {
            return Ok(wrote_any);
        };
        if cleaned_grok_content(&after)? == after {
            return Ok(wrote_any);
        }
    }
    Err(HookError::new("error.hooks.writeFailed").param(
        "detail",
        "legacy Grok hook config kept changing during reconciliation",
    ))
}

/// 使用 core 契约仅清除 Grok 受管条目；纯 Loki 文件归一为空对象而不删除。
fn cleaned_grok_content(existing: &str) -> Result<String, HookError> {
    Ok(remove_managed_hook_entries(AiTool::Grok, existing)?
        .unwrap_or_else(|| EMPTY_LEGACY_CONFIG.to_owned()))
}

/// 把迁移静默等待拆成短轮询，使超时取消不会继续下一次提交。
fn settle_migration_with_cancellation(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    let started = std::time::Instant::now();
    loop {
        ensure_migration_not_cancelled(cancellation)?;
        let remaining = MIGRATION_SETTLE.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5).min(remaining));
    }
}

/// 返回与主配置校正一致的取消错误，不泄漏本机配置路径。
fn ensure_migration_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    if cancellation.is_some_and(|token| token.load(Ordering::Acquire)) {
        return Err(HookError::new("error.hooks.writeFailed").param(
            "detail",
            "hook config write was cancelled before completion",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "hook_config_migration_tests.rs"]
mod tests;
