//! 恢复包的具体文件I/O只在应用后台执行，导出不覆盖、恢复仍复用私有账本原子提交。

use super::store::{DeviceStore, save_file, load_file};
use loki_metris_core::devices::{DeviceBook, recovery::{RecoveryPlan, MAX_PACKAGE_BYTES}};
use std::{fs, io::{Read, Write}, path::Path};

impl DeviceStore {
    /// 创建新包后才返回强恢复码；同名文件、链接或写失败都不碰原文件。
    pub(super) fn export_recovery(&self, destination: &Path) -> Result<String, &'static str> {
        let book = self.load()?;
        let (bytes, code) = book.export_recovery(&random_bytes(), random_bytes()).map_err(|_| "device_recovery_invalid")?;
        let parent = destination.parent().filter(|path| !path.as_os_str().is_empty()).ok_or("device_recovery_file_failed")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| "device_recovery_file_failed")?;
        temporary.write_all(&bytes).map_err(|_| "device_recovery_file_failed")?;
        temporary.as_file().sync_all().map_err(|_| "device_recovery_file_failed")?;
        temporary.persist_noclobber(destination).map_err(|error| if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            "device_recovery_file_exists"
        } else { "device_recovery_file_failed" })?;
        Ok(code)
    }

    /// 仅打开用户所选普通文件，有界读取后交core认证和冲突预检；不写账本。
    pub(super) fn prepare_recovery(&self, source: &Path, code: &str) -> Result<RecoveryPlan, &'static str> {
        let metadata = fs::symlink_metadata(source).map_err(|_| "device_recovery_file_failed")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_PACKAGE_BYTES as u64 {
            return Err("device_recovery_invalid");
        }
        let mut bytes = Vec::new();
        fs::File::open(source).map_err(|_| "device_recovery_file_failed")?.take(MAX_PACKAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes).map_err(|_| "device_recovery_file_failed")?;
        self.load()?.prepare_recovery(&bytes, code).map_err(|error| match error {
            loki_metris_core::devices::recovery::RecoveryError::Conflict => "device_recovery_conflict",
            _ => "device_recovery_invalid",
        })
    }

    /// 同锁复核预览基线与原子落盘；失败不更改内存权威账本。
    pub(super) fn commit_recovery(&self, plan: RecoveryPlan) -> Result<DeviceBook, &'static str> {
        self.load()?;
        let mut book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = book.as_ref().ok_or("device_storage_failed")?;
        if load_file(&self.path)? != *current { return Err("device_recovery_conflict"); }
        let recovered = plan.commit(current).map_err(|_| "device_recovery_conflict")?;
        save_file(&self.path, &recovered)?;
        *book = Some(recovered.clone()); Ok(recovered)
    }
}

/// UUIDv4来自已声明依赖的系统随机源；跳过版本/变体字节，保留每个输出字节的完整随机性。
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N]; let mut count = 0;
    while count < N {
        for (index, byte) in uuid::Uuid::new_v4().as_bytes().iter().enumerate() {
            if index == 6 || index == 8 { continue; }
            bytes[count] = *byte; count += 1;
            if count == N { break; }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 隔离真实文件导出/预览/恢复后重开可读；覆盖、坏码和旧预览不损坏现有记录。
    #[test]
    fn recovery_file_roundtrip_and_conflicts_preserve_originals() {
        let temp = tempfile::tempdir().unwrap();
        let source = DeviceStore::new(&temp.path().join("source"));
        source.prepare_service().unwrap();
        let expected = source.load().unwrap(); let package = temp.path().join("pairing.lmr");
        let code = source.export_recovery(&package).unwrap(); let encrypted = fs::read(&package).unwrap();
        assert!(!encrypted.windows(expected.client_id().len()).any(|bytes| bytes == expected.client_id().as_bytes()));
        assert_eq!(source.export_recovery(&package).err(), Some("device_recovery_file_exists"));
        assert_eq!(fs::read(&package).unwrap(), encrypted);
        let target = DeviceStore::new(&temp.path().join("target")); target.prepare_service().unwrap();
        let original = fs::read(&target.path).unwrap();
        assert!(target.prepare_recovery(&package, "wrong").is_err());
        assert_eq!(fs::read(&target.path).unwrap(), original);
        let plan = target.prepare_recovery(&package, &code).unwrap();
        assert_eq!(plan.summary().client_id, expected.client_id());
        target.commit_recovery(plan).unwrap();
        assert_eq!(DeviceStore::new(&temp.path().join("target")).load().unwrap(), expected);
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&package).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
    /// 错误输入和最终落盘失败均保留已提交字节及内存，链接不能成为恢复包输入。
    #[test]
    fn malformed_files_and_failed_commit_preserve_existing_book() {
        let temp = tempfile::tempdir().unwrap();
        let source = DeviceStore::new(&temp.path().join("source")); source.prepare_service().unwrap();
        let package = temp.path().join("pairing.lmr"); let code = source.export_recovery(&package).unwrap();
        let target = DeviceStore::new(&temp.path().join("target")); target.prepare_service().unwrap();
        let original = target.load().unwrap(); let bytes = fs::read(&target.path).unwrap();
        let oversized = temp.path().join("large.lmr"); fs::write(&oversized, vec![0; MAX_PACKAGE_BYTES + 1]).unwrap();
        assert_eq!(target.prepare_recovery(&oversized, &code).err().is_some(), true);
        #[cfg(unix)] {
            let link = temp.path().join("link.lmr"); std::os::unix::fs::symlink(&package, &link).unwrap();
            assert!(target.prepare_recovery(&link, &code).is_err());
        }
        let plan = target.prepare_recovery(&package, &code).unwrap();
        let backup = target.path.with_extension("backup"); fs::rename(&target.path, &backup).unwrap(); fs::create_dir(&target.path).unwrap();
        assert!(target.commit_recovery(plan).is_err());
        assert_eq!(target.load().unwrap(), original); assert_eq!(fs::read(&backup).unwrap(), bytes);
        fs::remove_dir(&target.path).unwrap(); fs::rename(backup, &target.path).unwrap();
        let plan = target.prepare_recovery(&package, &code).unwrap();
        let changed = DeviceBook::new("dddddddd-dddd-4ddd-8ddd-dddddddddddd".into()).unwrap();
        fs::write(&target.path, changed.encode_private().unwrap()).unwrap();
        assert_eq!(target.commit_recovery(plan).err(), Some("device_recovery_conflict"));
        assert_eq!(load_file(&target.path).unwrap(), changed); assert_eq!(target.load().unwrap(), original);
    }
}
