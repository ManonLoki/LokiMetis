//! 把数据根路径编码为数据库 BLOB 列使用的平台原生字节序列。

use std::path::{Path, PathBuf};

// 下面两组函数把路径存成数据库里的 BLOB（原始字节）而不是 TEXT：
// Unix 文件名允许任意非 `\0` 字节（不保证是合法 UTF-8），Windows 路径
// 则是 UTF-16 单元序列，两者都不能安全地无损转换成 Rust `String`
// （String 要求合法 UTF-8）。用各平台的“原生编码”存成字节，能保证
// 100% 还原原始路径，不会因为极端文件名（如包含非法编码片段）而丢数据
// 或 panic；对应地，这些字节永远不会被当作可展示文本使用。
/// 以平台原生单元编码路径，避免把非 UTF-8 路径误写成展示文本。
#[cfg(unix)]
pub(crate) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

/// 从 Unix 原生字节恢复仅供 adapter 访问的路径。
#[cfg(unix)]
pub(crate) fn decode_path(value: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Some(std::ffi::OsString::from_vec(value.to_vec()).into())
}

/// 以 UTF-16 小端编码 Windows 路径，避免假定路径为 UTF-8。
#[cfg(windows)]
pub(crate) fn encode_path(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// 从 UTF-16 小端恢复仅供 adapter 访问的 Windows 路径。
#[cfg(windows)]
pub(crate) fn decode_path(value: &[u8]) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    let chunks = value.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return None;
    }
    let units = chunks
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Some(std::ffi::OsString::from_wide(&units).into())
}
