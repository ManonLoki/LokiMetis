//! 展示档案与图库共享提交边界；文件驱动在锁内读取最新事实再原子提交。

use loki_metris_core::HookError;
use std::{
    io::Read,
    path::Path,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

/// 进程内所有档案和图库读取/修改使用同一把锁，避免引用检查与删除穿插。
static CATALOG_LOCK: RwLock<()> = RwLock::new(());

/// 取得读取快照许可；锁损坏不能当作空数据继续。
pub(super) fn read_lock() -> Result<RwLockReadGuard<'static, ()>, HookError> {
    CATALOG_LOCK
        .read()
        .map_err(|_| HookError::new("error.monitor.imagesReadFailed"))
}

/// 取得完整读改写许可；任务在后台 owner 中等待，不占用 UI 线程。
pub(super) fn write_lock() -> Result<RwLockWriteGuard<'static, ()>, HookError> {
    CATALOG_LOCK
        .write()
        .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))
}

/// 有界读取普通文件；缺失由调用方决定默认，链接和非常规对象均拒绝。
pub(super) fn read_regular(
    path: &Path,
    limit: u64,
    key: &'static str,
) -> Result<Vec<u8>, HookError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| HookError::new(key))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(HookError::new(key));
    }
    let file = std::fs::File::open(path).map_err(|_| HookError::new(key))?;
    if !file.metadata().is_ok_and(|value| value.is_file()) {
        return Err(HookError::new(key));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HookError::new(key))?;
    if bytes.len() as u64 > limit {
        return Err(HookError::new(key));
    }
    Ok(bytes)
}
