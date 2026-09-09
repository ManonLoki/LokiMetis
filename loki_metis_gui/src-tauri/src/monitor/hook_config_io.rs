//! 共享 Hook 配置的跨进程协调与有限写后重读。

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use loki_metis_core::{AiTool, HookConfigPreview, HookError, merge_hook_config};

use super::wsl::WslFile;

/// 同一配置根中的 LokiMetis writer 共用一个可恢复锁文件。
const LOCK_FILENAME: &str = ".lokimetis-hook-config.lock";
/// 竞争锁最多等待两秒，避免损坏或长任务锁死调用方。
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
/// 锁竞争的有界轮询间隔。
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(20);
/// 与未采用本锁的外部 writer 最多进行四轮合并校正。
const RECONCILIATION_ROUNDS: usize = 4;
/// 每轮提交前后等待短暂静默，用于观察另一个原子替换 writer。
const RECONCILIATION_SETTLE: Duration = Duration::from_millis(25);
/// 持有一个配置根的操作系统文件锁；进程退出或句柄 Drop 时由系统自动释放。
pub(super) struct HookConfigFileLock {
    _file: File,
}

impl HookConfigFileLock {
    /// 在固定期限内获取独占文件锁；常驻锁文件本身不包含状态，也永不删除。
    #[cfg(test)]
    pub(super) fn acquire(directory: &Path) -> Result<Self, HookError> {
        Self::acquire_with_cancellation(directory, None)
    }

    /// 获取锁时轮询调用方取消令牌，取消后不再等待或进入后续提交。
    pub(super) fn acquire_with_cancellation(
        directory: &Path,
        cancellation: Option<&AtomicBool>,
    ) -> Result<Self, HookError> {
        ensure_reconcile_not_cancelled(cancellation)?;
        fs::create_dir_all(directory).map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
        ensure_reconcile_not_cancelled(cancellation)?;
        let path = directory.join(LOCK_FILENAME);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| {
                HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
            })?;
        let started = Instant::now();
        loop {
            ensure_reconcile_not_cancelled(cancellation)?;
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= LOCK_WAIT_TIMEOUT {
                        return Err(HookError::new("error.hooks.writeFailed")
                            .param("detail", "timed out waiting for hook config lock"));
                    }
                    thread::sleep(LOCK_RETRY_DELAY);
                }
                Err(TryLockError::Error(error)) => {
                    return Err(HookError::new("error.hooks.writeFailed")
                        .param("detail", error.to_string()));
                }
            }
        }
    }
}

/// 在配置根锁内写入一组文件，并与不采用本锁的外部原子 writer 有限校正。
#[cfg(test)]
pub(super) fn reconcile_local_config_set(
    directory: &Path,
    tool: AiTool,
    generated_files: Vec<(PathBuf, HookConfigPreview)>,
) -> Result<bool, HookError> {
    reconcile_local_config_set_with_cancellation(directory, tool, generated_files, None)
}

/// 本机校正的每个阻塞和提交边界都观察同一请求取消令牌。
pub(super) fn reconcile_local_config_set_with_cancellation(
    directory: &Path,
    tool: AiTool,
    generated_files: Vec<(PathBuf, HookConfigPreview)>,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, HookError> {
    if generated_files.is_empty() {
        return Ok(false);
    }
    ensure_reconcile_not_cancelled(cancellation)?;
    let current = read_config_set_with_cancellation(&generated_files, cancellation)?;
    if config_set_matches(
        &merge_config_set(tool, &generated_files, &current)?,
        &current,
    ) {
        return Ok(false);
    }
    reconcile_local_config_set_with_cancellation_and_observer(
        directory,
        tool,
        generated_files,
        cancellation,
        |_, _| {},
    )
}

/// 对 WSL 配置执行有界写前稳定检查和写后校正，兼容未采用 Loki 锁的外部 writer。
pub(super) fn reconcile_wsl_config_set(
    tool: AiTool,
    generated_files: Vec<(WslFile, HookConfigPreview)>,
) -> Result<bool, HookError> {
    if generated_files.is_empty() {
        return Ok(false);
    }
    let current = read_wsl_config_set(&generated_files)?;
    if config_set_matches(
        &merge_wsl_config_set(tool, &generated_files, &current)?,
        &current,
    ) {
        return Ok(false);
    }
    let mut wrote_any = false;
    for _ in 0..RECONCILIATION_ROUNDS {
        let before = read_wsl_config_set(&generated_files)?;
        let desired = merge_wsl_config_set(tool, &generated_files, &before)?;
        thread::sleep(RECONCILIATION_SETTLE);
        if read_wsl_config_set(&generated_files)? != before {
            continue;
        }

        for (((path, _), existing), content) in generated_files.iter().zip(&before).zip(&desired) {
            if existing.as_deref() != Some(content.as_str()) {
                path.write_atomic(content).map_err(|error| {
                    HookError::new("error.hooks.writeFailed")
                        .param("path", path.display())
                        .param("detail", error.to_string())
                })?;
                wrote_any = true;
            }
        }
        thread::sleep(RECONCILIATION_SETTLE);

        let after = read_wsl_config_set(&generated_files)?;
        if config_set_matches(
            &merge_wsl_config_set(tool, &generated_files, &after)?,
            &after,
        ) {
            return Ok(wrote_any);
        }
    }
    Err(HookError::new("error.hooks.writeFailed").param(
        "detail",
        "WSL hook config kept changing during reconciliation",
    ))
}

/// 实际校正循环；observer 只供隔离测试在确定提交点模拟外部 writer。
#[cfg(test)]
fn reconcile_local_config_set_with_observer(
    directory: &Path,
    tool: AiTool,
    generated_files: Vec<(PathBuf, HookConfigPreview)>,
    observer: impl FnMut(usize, &[PathBuf]),
) -> Result<bool, HookError> {
    reconcile_local_config_set_with_cancellation_and_observer(
        directory,
        tool,
        generated_files,
        None,
        observer,
    )
}

/// 本机实际校正循环；每轮读、静默、临时写和 persist 前均遵守取消门禁。
fn reconcile_local_config_set_with_cancellation_and_observer(
    directory: &Path,
    tool: AiTool,
    generated_files: Vec<(PathBuf, HookConfigPreview)>,
    cancellation: Option<&AtomicBool>,
    mut observer: impl FnMut(usize, &[PathBuf]),
) -> Result<bool, HookError> {
    if generated_files.is_empty() {
        return Ok(false);
    }
    let _lock = HookConfigFileLock::acquire_with_cancellation(directory, cancellation)?;
    let mut wrote_any = false;
    for round in 0..RECONCILIATION_ROUNDS {
        ensure_reconcile_not_cancelled(cancellation)?;
        let before = read_config_set_with_cancellation(&generated_files, cancellation)?;
        let desired = merge_config_set(tool, &generated_files, &before)?;
        settle_with_cancellation(cancellation)?;
        if read_config_set_with_cancellation(&generated_files, cancellation)? != before {
            continue;
        }

        let mut written_paths = Vec::new();
        for (((path, _), existing), content) in generated_files.iter().zip(&before).zip(&desired) {
            ensure_reconcile_not_cancelled(cancellation)?;
            if existing.as_deref() != Some(content.as_str()) {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
                    })?;
                }
                ensure_reconcile_not_cancelled(cancellation)?;
                super::atomic_file::write_monitor_file_atomically_with_cancellation(
                    path,
                    content.as_bytes(),
                    "error.hooks.writeFailed",
                    cancellation,
                )?;
                ensure_reconcile_not_cancelled(cancellation)?;
                written_paths.push(path.clone());
                wrote_any = true;
            }
        }
        observer(round, &written_paths);
        settle_with_cancellation(cancellation)?;

        let after = read_config_set_with_cancellation(&generated_files, cancellation)?;
        if config_set_matches(&merge_config_set(tool, &generated_files, &after)?, &after) {
            return Ok(wrote_any);
        }
    }
    Err(HookError::new("error.hooks.writeFailed")
        .param("detail", "hook config kept changing during reconciliation"))
}

/// 判断整组磁盘内容是否已经逐份等于本轮合并目标。
fn config_set_matches(desired: &[String], existing: &[Option<String>]) -> bool {
    desired
        .iter()
        .zip(existing)
        .all(|(desired, existing)| existing.as_deref() == Some(desired.as_str()))
}

/// 逐份读取本机目标，并在文件边界停止已经取消的校正。
fn read_config_set_with_cancellation(
    generated_files: &[(PathBuf, HookConfigPreview)],
    cancellation: Option<&AtomicBool>,
) -> Result<Vec<Option<String>>, HookError> {
    let mut current = Vec::with_capacity(generated_files.len());
    for (path, _) in generated_files {
        ensure_reconcile_not_cancelled(cancellation)?;
        current.push(read_optional_config(path)?);
    }
    ensure_reconcile_not_cancelled(cancellation)?;
    Ok(current)
}

/// 读取一组 WSL 目标配置，保持与生成文件相同的稳定顺序。
fn read_wsl_config_set(
    generated_files: &[(WslFile, HookConfigPreview)],
) -> Result<Vec<Option<String>>, HookError> {
    generated_files
        .iter()
        .map(|(path, _)| path.read_optional())
        .collect()
}

/// 把每份当前内容与对应协议预览合并成下一次完整提交内容。
fn merge_config_set(
    tool: AiTool,
    generated_files: &[(PathBuf, HookConfigPreview)],
    existing: &[Option<String>],
) -> Result<Vec<String>, HookError> {
    generated_files
        .iter()
        .zip(existing)
        .map(|((_, generated), current)| {
            merge_hook_config(current.as_deref(), generated, tool).map(|merged| merged.content)
        })
        .collect()
}

/// 把 WSL 中每份当前内容与对应协议预览合并成下一次完整提交内容。
fn merge_wsl_config_set(
    tool: AiTool,
    generated_files: &[(WslFile, HookConfigPreview)],
    existing: &[Option<String>],
) -> Result<Vec<String>, HookError> {
    generated_files
        .iter()
        .zip(existing)
        .map(|((_, generated), current)| {
            merge_hook_config(current.as_deref(), generated, tool).map(|merged| merged.content)
        })
        .collect()
}

/// 读取可能不存在的配置文件，并区分正常缺失与其他 I/O 错误。
fn read_optional_config(path: &Path) -> Result<Option<String>, HookError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(HookError::new("error.hooks.existingReadFailed").param("detail", error.to_string()))
        }
    }
}

/// 把短暂静默拆成可取消轮询，避免超时后继续进入下一次读写轮次。
fn settle_with_cancellation(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    let deadline = Instant::now() + RECONCILIATION_SETTLE;
    loop {
        ensure_reconcile_not_cancelled(cancellation)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(LOCK_RETRY_DELAY.min(remaining));
    }
}

/// 统一生成取消错误，使锁等待、读写和提交前检查保持相同语义。
fn ensure_reconcile_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    if cancellation.is_some_and(|token| token.load(Ordering::Acquire)) {
        return Err(HookError::new("error.hooks.writeFailed").param(
            "detail",
            "hook config write was cancelled before completion",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "hook_config_io_tests.rs"]
mod tests;
