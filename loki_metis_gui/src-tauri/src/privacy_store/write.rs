//! 平台相关的设置文件写入：Unix 原子替换，Windows 事务文件 + 原位提交。

use std::fs;
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use atomic_write_file::AtomicWriteFile;

use super::PrivacyStoreError;
#[cfg(windows)]
use super::metadata_is_link_like;

/// Windows 两阶段写入使用的临时事务文件名，与主设置文件同目录。
#[cfg(windows)]
const SETTINGS_TRANSACTION_FILE_NAME: &str = "privacy-settings.transaction.json";

/// Unix 先写同目录私有临时文件；只有显式 commit 才替换旧快照。
// “原子写入”模式：直接对目标文件 write 有风险——如果写到一半进程崩溃或
// 断电，文件会处于半写坏损状态。做法是先把新内容完整写进同目录的临时
// 文件，写完并设置好权限后，再用一次原子的文件系统重命名操作
// （AtomicWriteFile::commit 内部就是这样）替换旧文件；重命名在同一
// 文件系统内是原子的，不存在“重命名到一半”的中间状态。
#[cfg(unix)]
pub(super) fn write_settings_payload(path: &Path, payload: &[u8]) -> Result<(), PrivacyStoreError> {
    let mut file = open_settings_writer(path)?;
    file.write_all(payload).map_err(|_| PrivacyStoreError)?;
    set_private_permissions(file.as_file())?;
    file.commit().map_err(|_| PrivacyStoreError)
}

/// Windows 先同步同目录事务载荷，再原位提交主文件以保留既有 DACL；失败时保留恢复源。
// Windows 平台的重命名/替换语义与文件权限（ACL/DACL）交互比较特殊，
// 不能直接照搬 Unix 的“临时文件 + rename”方案，所以这里采用两阶段策略：
//   1. 先把新内容完整写入一个独立的“事务文件”（transaction file）并 fsync；
//   2. 再原地覆盖真正的设置文件内容（而不是替换整个文件），这样可以保留
//      文件原本已经设置好的访问控制列表（DACL）；
//   3. 只有原地覆盖也成功后，才删除事务文件。
// 如果进程在步骤 2 中途崩溃，下次启动时 read_stored_settings 发现主文件
// 损坏，会退回读取尚未删除的事务文件来恢复（见 load_settings_with_migration_state）。
#[cfg(windows)]
pub(super) fn write_settings_payload(path: &Path, payload: &[u8]) -> Result<(), PrivacyStoreError> {
    let parent = path.parent().ok_or(PrivacyStoreError)?;
    let transaction_path = parent.join(SETTINGS_TRANSACTION_FILE_NAME);
    if let Ok(metadata) = fs::symlink_metadata(&transaction_path)
        && (metadata_is_link_like(&metadata) || !metadata.is_file())
    {
        return Err(PrivacyStoreError);
    }

    let mut transaction = tempfile::NamedTempFile::new_in(parent).map_err(|_| PrivacyStoreError)?;
    transaction
        .write_all(payload)
        .map_err(|_| PrivacyStoreError)?;
    transaction
        .as_file_mut()
        .sync_all()
        .map_err(|_| PrivacyStoreError)?;
    transaction
        .persist(&transaction_path)
        .map_err(|_| PrivacyStoreError)?;

    let mut options = fs::OpenOptions::new();
    options.create(true).write(true);
    use std::os::windows::fs::OpenOptionsExt;
    /// 打开句柄时不跟随 reparse point，避免原位写入穿透到链接目标。
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let mut file = options.open(path).map_err(|_| PrivacyStoreError)?;
    file.set_len(0).map_err(|_| PrivacyStoreError)?;
    file.write_all(payload).map_err(|_| PrivacyStoreError)?;
    file.sync_all().map_err(|_| PrivacyStoreError)?;
    set_private_permissions(&file)?;
    fs::remove_file(transaction_path).map_err(|_| PrivacyStoreError)
}

/// 其他非 Unix 平台保留原位写入；当前批准目标不会进入此兼容分支。
#[cfg(not(any(unix, windows)))]
pub(super) fn write_settings_payload(path: &Path, payload: &[u8]) -> Result<(), PrivacyStoreError> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    let mut file = options.open(path).map_err(|_| PrivacyStoreError)?;
    file.write_all(payload).map_err(|_| PrivacyStoreError)?;
    file.sync_all().map_err(|_| PrivacyStoreError)?;
    set_private_permissions(&file)
}

/// 创建 Unix 同目录私有临时文件；只有显式 commit 才替换现有完整设置快照。
#[cfg(unix)]
pub(super) fn open_settings_writer(path: &Path) -> Result<AtomicWriteFile, PrivacyStoreError> {
    let mut options = AtomicWriteFile::options();
    use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
    use std::os::unix::fs::OpenOptionsExt as StandardOpenOptionsExt;

    AtomicOpenOptionsExt::preserve_mode(&mut options, false);
    AtomicOpenOptionsExt::preserve_owner(&mut options, true);
    StandardOpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path).map_err(|_| PrivacyStoreError)?;
    set_private_permissions(file.as_file())?;
    Ok(file)
}

/// 在 Unix 平台修正已存在设置文件的权限，其他平台依赖 app-data ACL。
fn set_private_permissions(_file: &fs::File) -> Result<(), PrivacyStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = _file
            .metadata()
            .map_err(|_| PrivacyStoreError)?
            .permissions();
        permissions.set_mode(0o600);
        _file
            .set_permissions(permissions)
            .map_err(|_| PrivacyStoreError)?;
    }
    Ok(())
}
