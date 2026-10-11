//! 文件内容未变的低成本判据，供读取侧缓存决定能否省略重复解析。

use std::fs::Metadata;
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// 长度、修改时间与（Unix）设备/inode/状态变更时间同时相同才视为未改变。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl FileStamp {
    /// 从元数据取得文件戳；已打开句柄应在读取内容之前调用。
    pub(crate) fn of(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (metadata.dev(), metadata.ino(), metadata.ctime(), metadata.ctime_nsec())
            },
        }
    }

    /// 按路径取得普通文件的文件戳；不存在返回 `None`，其余错误原样传播。
    pub(crate) fn of_path(path: &Path) -> io::Result<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(Self::of(&metadata))),
            Ok(_) => Err(io::ErrorKind::InvalidInput.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 戳记录时的文件长度，同时是 JSONL 读取预算使用的字节数。
    pub(crate) fn len(&self) -> u64 {
        self.len
    }
}
