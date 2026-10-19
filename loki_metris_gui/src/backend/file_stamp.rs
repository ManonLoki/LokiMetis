//! 文件内容未变的低成本判据，供读取侧缓存决定能否省略重复解析。

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// 长度、修改时间与宿主文件身份/状态变更时间同时相同才视为未改变。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FileStamp {
    /// 文件字节长度。
    len: u64,
    /// 修改时间；读取失败时为 None，此时不可缓存。
    modified: Option<SystemTime>,
    /// 宿主文件身份：Unix 为 (设备, inode, ctime 秒, ctime 纳秒)；Windows 为 (卷序列号, 128 位文件 ID, ChangeTime)，读取失败时为 None。
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(windows)]
    identity: Option<(u64, [u8; 16], i64)>,
}

impl FileStamp {
    /// 从元数据取得文件戳；已打开句柄应在读取内容之前调用。
    pub(crate) fn of(file: &File, metadata: &Metadata) -> Self {
        #[cfg(not(windows))]
        let _ = file;
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (metadata.dev(), metadata.ino(), metadata.ctime(), metadata.ctime_nsec())
            },
            #[cfg(windows)]
            identity: windows_identity(file),
        }
    }

    /// 按路径取得普通文件的文件戳；不存在返回 `None`，其余错误原样传播。
    pub(crate) fn of_path(path: &Path) -> io::Result<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => {
                let file = File::open(path)?;
                let opened = file.metadata()?;
                if !opened.is_file() { return Err(io::ErrorKind::InvalidInput.into()); }
                let stamp = Self::of(&file, &opened);
                if !stamp.is_cacheable() { return Err(io::ErrorKind::Unsupported.into()); }
                Ok(Some(stamp))
            }
            Ok(_) => Err(io::ErrorKind::InvalidInput.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 戳记录时的文件长度，同时是 JSONL 读取预算使用的字节数。
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// 无法读取可靠身份时仍允许解析文件，但禁止复用旧内容。
    pub(crate) fn is_cacheable(&self) -> bool {
        #[cfg(windows)]
        { self.modified.is_some() && self.identity.is_some() }
        #[cfg(not(windows))]
        { self.modified.is_some() }
    }
}

/// 同一打开句柄的卷号、文件 ID 与 ChangeTime 同时防止替换和保留 mtime 的原地改写。
#[cfg(windows)]
fn windows_identity(file: &File) -> Option<(u64, [u8; 16], i64)> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows::Win32::{Foundation::HANDLE, Storage::FileSystem::{
        GetFileInformationByHandleEx, FileIdInfo, FILE_ID_INFO, FileBasicInfo, FILE_BASIC_INFO,
        ReOpenFile, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_DELETE, FILE_FLAGS_AND_ATTRIBUTES,
    }};
    let mut id = FILE_ID_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: File 在调用期间持有句柄，两个输出结构大小及信息类别符合 Win32 ABI。
    unsafe {
        let handle = HANDLE(file.as_raw_handle());
        // Windows 写入句柄关闭前可能延迟更新戳；仍有写者时禁用 metadata 快速复用。
        // 只读同对象重开检查数据共享约束；零权限属性句柄会绕过此检查，不能用于此处。
        let stable = ReOpenFile(handle, FILE_GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_DELETE, FILE_FLAGS_AND_ATTRIBUTES(0)).ok()?;
        let _stable = File::from_raw_handle(stable.0);
        GetFileInformationByHandleEx(handle, FileIdInfo, std::ptr::from_mut(&mut id).cast(),
            std::mem::size_of_val(&id) as u32).ok()?;
        GetFileInformationByHandleEx(handle, FileBasicInfo, std::ptr::from_mut(&mut basic).cast(),
            std::mem::size_of_val(&basic) as u32).ok()?;
    }
    Some((id.VolumeSerialNumber, id.FileId.Identifier, basic.ChangeTime))
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    /// 路径缓存与打开句柄缓存均不能把同长度、同 mtime 的改写或替换视为未变。
    #[test]
    fn path_stamp_detects_preserved_modified_time_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("index.json");
        std::fs::write(&path, b"one").unwrap();
        let first = FileStamp::of_path(&path).unwrap().unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"two").unwrap();
        File::options().write(true).open(&path).unwrap().set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        let rewritten = FileStamp::of_path(&path).unwrap().unwrap();
        assert_ne!(first, rewritten);
        let replacement = root.path().join("replacement");
        std::fs::write(&replacement, b"new").unwrap();
        File::options().write(true).open(&replacement).unwrap().set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
        std::fs::rename(&path, root.path().join("old")).unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert_ne!(rewritten, FileStamp::of_path(&path).unwrap().unwrap());
        let unreliable = FileStamp { identity: None, ..first };
        assert!(!unreliable.is_cacheable());
    }

    /// 长期持有的写入句柄可能尚未刷新时间戳，读取侧必须放弃缓存并在关闭后恢复。
    #[test]
    fn open_writer_disables_timestamp_reuse() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("active.jsonl");
        std::fs::write(&path, b"one").unwrap();
        let reader = File::open(&path).unwrap();
        assert!(FileStamp::of(&reader, &reader.metadata().unwrap()).is_cacheable());
        let writer = File::options().write(true).open(&path).unwrap();
        assert!(!FileStamp::of(&reader, &reader.metadata().unwrap()).is_cacheable());
        drop(writer);
        assert!(FileStamp::of(&reader, &reader.metadata().unwrap()).is_cacheable());
    }
}
