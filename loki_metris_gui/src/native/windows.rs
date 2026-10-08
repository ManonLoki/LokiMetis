//! GPUI 主窗口的原生显示/隐藏：关闭到托盘需要真正隐藏 HWND，并在恢复时处理最小化与前台限制。
//!
//! 只声明用到的 Win32 ABI；托盘由 `tray-icon` 提供，这里不再有消息循环。

use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{ffi::c_void, io, mem::size_of};

/// 借用的 Win32 窗口句柄 ABI；不转移所有权，调用方不得解引用此指针。
type Handle = *mut c_void;

const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_RESTORE: i32 = 9;
const FLASHW_ALL: u32 = 3;

/// 与 Win32 FLASHWINFO 保持 C 布局的闪烁参数；窗口句柄须在调用期间有效。
/// size 是结构体字节数，flags 选择闪烁区域，count 为次数；timeout 以毫秒计，零使用系统默认间隔。
#[repr(C)]
struct FlashInfo {
    size: u32,
    window: Handle,
    flags: u32,
    count: u32,
    timeout: u32,
}

// 布局不一致会让原生代码越界读取结构体。
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<FlashInfo>() == 32);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<FlashInfo>() == 20);

#[link(name = "kernel32")]
unsafe extern "system" {
    /// 返回调用线程的系统标识，用于确认窗口操作仍在捕获句柄的 UI 线程执行。
    fn GetCurrentThreadId() -> u32;
    /// 返回当前进程的系统标识，用于复核窗口仍属于本进程。
    fn GetCurrentProcessId() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    /// 请求把有效窗口设为前台；受系统前台限制，非零表示成功，零表示未取得前台。
    fn SetForegroundWindow(window: Handle) -> i32;
    /// 激活调用线程消息队列所属的有效窗口；返回之前的活动窗口句柄，失败可返回空。
    fn SetActiveWindow(window: Handle) -> Handle;
    /// 将焦点交给调用线程输入队列关联的有效窗口；返回之前的焦点窗口句柄，失败可返回空。
    fn SetFocus(window: Handle) -> Handle;
    /// 将有效窗口移至所属 Z 序顶端并请求激活；非零表示调用成功。
    fn BringWindowToTop(window: Handle) -> i32;
    /// 按有效 FLASHWINFO 指针闪烁窗口；返回调用前窗口是否活动，不是成功状态。
    fn FlashWindowEx(info: *const FlashInfo) -> i32;
    /// 按 Win32 显示命令隐藏、显示或恢复有效窗口；返回调用前是否可见，不是成功状态。
    fn ShowWindow(window: Handle, command: i32) -> i32;
    /// 查询窗口是否最小化；非零表示最小化，零表示未最小化。
    fn IsIconic(window: Handle) -> i32;
    /// 查询句柄当前是否标识窗口；结果不能保证其后仍有效，也不能证明进程或线程归属。
    fn IsWindow(window: Handle) -> i32;
    /// 返回创建窗口的线程标识，失败返回零；非空 process 指针须可写，用于接收所属进程标识。
    fn GetWindowThreadProcessId(window: Handle, process: *mut u32) -> u32;
}

/// 捕捉已经创建的 GPUI 主窗；原始指针使此类型不能跨线程移动。
#[derive(Clone, Copy)]
pub(crate) struct MainWindow {
    window: Handle,
    thread: u32,
}

impl MainWindow {
    /// 从仍然存活的 GPUI 窗口提取 HWND，不依赖后端的窗口类名。
    pub(crate) fn capture(window: &Window) -> io::Result<Self> {
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "GPUI window did not provide a Win32 handle",
            ));
        };
        // SAFETY: 窗口仍由 GPUI 持有；保存的 HWND 仅在 UI 线程上使用，
        // 每次操作前均重新校验线程、进程与句柄有效性。
        let window = handle.hwnd.get() as Handle;
        let thread = unsafe { GetCurrentThreadId() };
        let native = Self { window, thread };
        if !native.is_valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GPUI window handle is not owned by this UI thread and process",
            ));
        }
        Ok(native)
    }

    /// 只隐藏仍属于当前进程和 UI 线程的主窗口。
    pub(crate) fn hide(&self) {
        if !self.is_valid() {
            return;
        }
        // SAFETY: validated GPUI HWND is accessed from its owning UI thread.
        unsafe {
            ShowWindow(self.window, SW_HIDE);
        }
    }

    /// 恢复最小化并请求聚焦；Windows 拒绝前台时以闪烁提示。
    pub(crate) fn show(&self) {
        if !self.is_valid() {
            return;
        }
        // SAFETY: all calls operate on the validated HWND on its UI thread.
        // SW_SHOW preserves maximization; SW_RESTORE also undoes minimization.
        unsafe {
            ShowWindow(
                self.window,
                if IsIconic(self.window) != 0 {
                    SW_RESTORE
                } else {
                    SW_SHOW
                },
            );
            BringWindowToTop(self.window);
            SetActiveWindow(self.window);
            SetFocus(self.window);
            if SetForegroundWindow(self.window) == 0 {
                let info = FlashInfo {
                    size: size_of::<FlashInfo>() as u32,
                    window: self.window,
                    flags: FLASHW_ALL,
                    count: 3,
                    timeout: 0,
                };
                FlashWindowEx(&info);
            }
        }
    }

    /// 每次访问前复核线程、进程与句柄，拒绝已销毁或复用窗口。
    fn is_valid(&self) -> bool {
        // SAFETY: HWND queries safely reject a destroyed handle. Also confirm
        // thread and process ownership so an accidentally recycled HWND is not
        // used to hide another application's window.
        unsafe {
            if GetCurrentThreadId() != self.thread || IsWindow(self.window) == 0 {
                return false;
            }
            let mut process = 0;
            GetWindowThreadProcessId(self.window, &mut process) == self.thread
                && process == GetCurrentProcessId()
        }
    }
}
