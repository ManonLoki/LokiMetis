//! 使用一次有界的 Windows 内核查询读取已验证进程命令行。

use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use super::install_identity::paths_equal_ignore_ascii_case;
use super::{OwnedHandle, query_process_path_from_handle};

const PROCESS_COMMAND_LINE_INFORMATION: i32 = 60;
const MAX_COMMAND_LINE_BYTES: usize = 128 * 1024;

#[repr(C)]
/// 描述 `NtQueryInformationProcess` 返回缓冲区开头的 UTF-16 字符串。
struct NativeUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    /// 调用 Windows 原生的有界进程信息查询，不启动 COM/WMI 工作线程。
    fn NtQueryInformationProcess(
        process_handle: HANDLE,
        process_information_class: i32,
        process_information: *mut c_void,
        process_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

/// 读取同一进程句柄的路径与命令行，PID 重用或身份变化时拒绝结果。
pub(super) fn query_verified_process_command_line(
    pid: u32,
    expected_path: &Path,
) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .ok()
        .map(OwnedHandle)?;
    let current_path = query_process_path_from_handle(handle.0)?;
    if !paths_equal_ignore_ascii_case(&current_path, expected_path) {
        return None;
    }
    query_process_command_line_from_handle(handle.0)
}

/// 用固定上限缓冲区执行一次内核查询，并严格验证返回指针与 UTF-16 长度。
fn query_process_command_line_from_handle(handle: HANDLE) -> Option<String> {
    let word_count = MAX_COMMAND_LINE_BYTES.div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; word_count];
    let base = storage.as_mut_ptr().cast::<u8>();
    let capacity = storage.len().checked_mul(size_of::<usize>())?;
    let mut returned = 0_u32;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            base.cast::<c_void>(),
            u32::try_from(capacity).ok()?,
            &mut returned,
        )
    };
    if status < 0
        || (returned as usize) > capacity
        || (returned as usize) < size_of::<NativeUnicodeString>()
    {
        return None;
    }
    let header = unsafe { &*base.cast::<NativeUnicodeString>() };
    let byte_length = usize::from(header.length);
    if byte_length == 0
        || byte_length % size_of::<u16>() != 0
        || byte_length > usize::from(header.maximum_length)
    {
        return None;
    }
    let buffer_start = header.buffer.cast::<u8>() as usize;
    let allocation_start = base as usize;
    let allocation_end = allocation_start.checked_add(capacity)?;
    let buffer_end = buffer_start.checked_add(byte_length)?;
    if buffer_start < allocation_start
        || buffer_end > allocation_end
        || buffer_start % std::mem::align_of::<u16>() != 0
    {
        return None;
    }
    let units = unsafe {
        std::slice::from_raw_parts(header.buffer.cast_const(), byte_length / size_of::<u16>())
    };
    String::from_utf16(units).ok()
}
