//! 标准 Win32 注册表桥只读写当前用户同名登录项，避免上游删除所有用户项。

use std::{io, ptr};

/// Windows 预定义 HKCU 为有符号扩展的伪句柄；不接收调用方提供的 root。
const CURRENT_USER: isize = -2147483647;
/// 当前用户正常登录命令位置。
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// 任务管理器可能单独禁用同名登录项。
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[link(name = "advapi32")]
unsafe extern "system" {
    /// 按精确权限打开当前用户键，不创建系统范围状态。
    fn RegOpenKeyExW(root: isize, name: *const u16, options: u32, access: u32, result: *mut isize) -> i32;
    /// 读取有界值，返回类型和实际字节数。
    fn RegQueryValueExW(key: isize, name: *const u16, reserved: *mut u32, kind: *mut u32, bytes: *mut u8, size: *mut u32) -> i32;
    /// 只删除当前已打开键的单个值。
    fn RegDeleteValueW(key: isize, name: *const u16) -> i32;
    /// 每个打开的真实句柄都在所属作用域结束时关闭。
    fn RegCloseKey(key: isize) -> i32;
}

/// 实际注册表句柄由当前作用域独占。
struct Key(isize);
impl Drop for Key {
    /// 错误路径也关闭句柄，不关闭预定义 root。
    fn drop(&mut self) { unsafe { RegCloseKey(self.0); } }
}
/// 固定键名和应用 ID 均不包含 NUL；UTF-16 终止由本函数拥有。
fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
/// 未找到视为不存在，其余 OS 错误保留为失败。
fn open(path: &str, access: u32) -> io::Result<Option<Key>> {
    let mut key = 0;
    let status = unsafe { RegOpenKeyExW(CURRENT_USER, wide(path).as_ptr(), 0, access, &mut key) };
    match status { 0 => Ok(Some(Key(key))), 2 => Ok(None), error => Err(io::Error::from_raw_os_error(error)) }
}
/// 单个登录项读取上限 16KiB，拒绝类型错误或无界数据。
fn value(path: &str, id: &str) -> io::Result<Option<(u32, Vec<u8>)>> {
    let Some(key) = open(path, 1)? else { return Ok(None) };
    let mut bytes = vec![0; 16 * 1024];
    let mut size = bytes.len() as u32;
    let mut kind = 0;
    let status = unsafe { RegQueryValueExW(key.0, wide(id).as_ptr(), ptr::null_mut(), &mut kind, bytes.as_mut_ptr(), &mut size) };
    match status {
        0 => { bytes.truncate(size as usize); Ok(Some((kind, bytes))) },
        2 => Ok(None),
        error => Err(io::Error::from_raw_os_error(error)),
    }
}
/// 系统事实来自 HKCU Run 和现有任务管理器开关；不读取 HKLM。
pub(super) fn is_enabled(id: &str) -> io::Result<bool> {
    let Some((kind, bytes)) = value(RUN, id)? else { return Ok(false) };
    if kind != 1 || !super::valid_windows_command(&bytes) { return Err(io::ErrorKind::InvalidData.into()) }
    match value(APPROVED, id)? {
        None => Ok(true),
        Some((3, bytes)) => super::windows_override(&bytes),
        Some(_) => Err(io::ErrorKind::InvalidData.into()),
    }
}
/// 关闭只删除本应用 HKCU Run 值；保留 OS 管理的历史开关和其他应用值。
pub(super) fn disable(id: &str) -> io::Result<()> {
    let Some(key) = open(RUN, 2)? else { return Ok(()) };
    let status = unsafe { RegDeleteValueW(key.0, wide(id).as_ptr()) };
    match status { 0 | 2 => Ok(()), error => Err(io::Error::from_raw_os_error(error)) }
}
