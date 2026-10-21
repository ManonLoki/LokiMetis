//! 设备凭据仅写产品私有子目录；坏文件和写入失败均保留此前已提交记录。

use loki_metris_core::devices::{DeviceBook, VerifiedPairing};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, sync::Mutex};

/// 单锁覆盖设备读取、领域变更、原子落盘与内存提交，不把秘密交给页面。
pub(super) struct DeviceStore {
    path: PathBuf,
    book: Mutex<Option<DeviceBook>>,
}

impl DeviceStore {
    /// 构造不访问磁盘；应用启动或显式操作在后台读取。
    pub(super) fn new(directory: &Path) -> Self {
        Self { path: directory.join("devices").join("paired.json"), book: Mutex::new(None) }
    }

    /// 懒读取完整账本，首次缺文件只创建内存身份；其它失败不按空列表处理。
    pub(super) fn load(&self) -> Result<DeviceBook, &'static str> {
        let mut book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(value) = book.as_ref() { return Ok(value.clone()); }
        let value = load_file(&self.path)?;
        *book = Some(value.clone()); Ok(value)
    }

    /// 应用设备服务启动前真实保存稳定本机身份，广播不能使用重启即变的临时ID。
    pub(super) fn prepare_service(&self) -> Result<DeviceBook, &'static str> {
        self.load()?;
        let book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = book.as_ref().ok_or("device_storage_failed")?;
        save_file(&self.path, value)?; Ok(value.clone())
    }

    /// 在任何网络尝试前保存稳定本机身份；已有设备不受新失败配对影响。
    pub(super) fn prepare_pairing(&self, id: &str) -> Result<String, &'static str> {
        self.load()?;
        let book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = book.as_ref().ok_or("device_storage_failed")?;
        if value.summaries().iter().any(|device| device.id == id) { return Err("device_already_saved"); }
        if value.summaries().len() >= loki_metris_core::devices::MAX_SAVED_DEVICES { return Err("device_limit"); }
        save_file(&self.path, value)?;
        Ok(value.client_id().into())
    }

    /// 认证完成仍须真实落盘成功；失败不更新缓存或页面保存列表。
    pub(super) fn paired(&self, proof: VerifiedPairing) -> Result<DeviceBook, &'static str> {
        self.load()?;
        let mut book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = book.as_ref().ok_or("device_storage_failed")?.paired(proof).map_err(|_| "device_pairing_failed")?;
        save_file(&self.path, &value)?; *book = Some(value.clone()); Ok(value)
    }

    /// 移除只提交本机新快照，不请求接收端或递归清理任何目录。
    pub(super) fn remove(&self, id: &str) -> Result<DeviceBook, &'static str> {
        self.load()?;
        let mut book = self.book.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let value = book.as_ref().ok_or("device_storage_failed")?.without(id).map_err(|_| "device_not_saved")?;
        save_file(&self.path, &value)?; *book = Some(value.clone()); Ok(value)
    }
}

/// 专属子目录和文件拒绝链接；Unix只允许当前用户访问凭据。
fn validate_private(path: &Path, directory: bool) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "device_storage_failed")?;
    if metadata.file_type().is_symlink() || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err("device_storage_failed");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 { return Err("device_storage_permissions"); }
    }
    Ok(())
}

/// 新安装缺文件允许创建内存身份；已存在损坏快照不得自动覆盖。
fn load_file(path: &Path) -> Result<DeviceBook, &'static str> {
    let parent = path.parent().ok_or("device_storage_failed")?;
    match fs::symlink_metadata(parent) {
        Ok(_) => validate_private(parent, true)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return DeviceBook::new(uuid::Uuid::new_v4().to_string()).map_err(|_| "device_storage_failed"),
        Err(_) => return Err("device_storage_failed"),
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.len() <= 64 * 1024 => validate_private(path, false)?,
        Ok(_) => return Err("device_storage_failed"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return DeviceBook::new(uuid::Uuid::new_v4().to_string()).map_err(|_| "device_storage_failed"),
        Err(_) => return Err("device_storage_failed"),
    }
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|_| "device_storage_failed")?.take(64 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "device_storage_failed")?;
    DeviceBook::decode(&bytes).map_err(|_| "device_storage_corrupt")
}

/// 先同步同目录0600临时文件再原子替换，失败保留旧文件并由tempfile清理临时物。
fn save_file(path: &Path, book: &DeviceBook) -> Result<(), &'static str> {
    let parent = path.parent().ok_or("device_storage_failed")?;
    let root = parent.parent().ok_or("device_storage_failed")?;
    fs::create_dir_all(root).map_err(|_| "device_storage_failed")?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
    match builder.create(parent) {
        Ok(()) => {},
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(_) => return Err("device_storage_failed"),
    }
    validate_private(parent, true)?;
    match fs::symlink_metadata(path) {
        Ok(_) => validate_private(path, false)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
        Err(_) => return Err("device_storage_failed"),
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|_| "device_storage_failed")?;
    temporary.write_all(&book.encode_private().map_err(|_| "device_storage_failed")?).map_err(|_| "device_storage_failed")?;
    temporary.as_file().sync_all().map_err(|_| "device_storage_failed")?;
    temporary.persist(path).map_err(|_| "device_storage_failed")?;
    Ok(())
}
