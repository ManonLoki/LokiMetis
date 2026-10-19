//! Win32辅助窗属性；所有调用重新捕获活窗，始终在GPUI线程执行。

use gpui_kit::{Bounds, Window, point, px, size};
use loki_metris_core::PetWindowPreferences;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::c_void;
/// Win32句柄只在当前调用栈借用。
type Handle = *mut c_void;
/// MONITORINFO工作区使用物理桌面坐标。
#[repr(C)]
#[derive(Default)]
struct Rect {
    /// 左边界（物理像素）。
    left: i32,
    /// 上边界（物理像素）。
    top: i32,
    /// 右边界，开区间。
    right: i32,
    /// 下边界，开区间。
    bottom: i32,
}
/// Win32 客户区原点转换所需的物理点。
#[repr(C)]
#[derive(Default)]
struct NativePoint {
    /// 横坐标（物理像素）。
    x: i32,
    /// 纵坐标（物理像素）。
    y: i32,
}

/// 同一活窗读回的内容区与四边框，避免将画布尺寸当作外框尺寸。
struct ClientFrame {
    /// 含非客户区的窗口外框。
    frame: Rect,
    /// 客户区矩形。
    client: Rect,
    /// 客户区原点的屏幕坐标。
    origin: NativePoint,
}
impl ClientFrame {
    /// 同步读取客户区和屏幕原点，所有指针仅在当前栈有效。
    fn read(hwnd: Handle) -> anyhow::Result<Self> {
        let mut value = Self { frame: Rect::default(), client: Rect::default(), origin: NativePoint::default() };
        // SAFETY: 调用方持有本线程活窗，三个输出为正确 C 布局的独占可写对象。
        anyhow::ensure!(unsafe { GetWindowRect(hwnd, &mut value.frame) != 0
            && GetClientRect(hwnd, &mut value.client) != 0 && ClientToScreen(hwnd, &mut value.origin) != 0 },
            "read auxiliary client geometry: {}", std::io::Error::last_os_error());
        Ok(value)
    }

    /// 输入为物理内容矩形；当前实际四边框只在这个原生边界加回一次。
    fn outer(&self, x: i32, y: i32, width: i32, height: i32) -> (i32, i32, i32, i32) {
        (x - (self.origin.x - self.frame.left), y - (self.origin.y - self.frame.top),
            width + (self.frame.right - self.frame.left) - (self.client.right - self.client.left),
            height + (self.frame.bottom - self.frame.top) - (self.client.bottom - self.client.top))
    }
}
/// Win32工作区查询的固定C布局。
#[repr(C)]
struct MonitorInfo {
    /// 结构体字节数，调用前必须设置。
    size: u32,
    /// 显示器整体矩形。
    monitor: Rect,
    /// 去除任务栏后的工作区矩形。
    work: Rect,
    /// 显示器标志；含主显示器位。
    flags: u32,
}
#[link(name = "user32")]
// SAFETY: 以下声明与 user32 导出的 Win32 原型逐项一致（system ABI、参数与返回类型），调用方负责传入有效 HWND。
unsafe extern "system" {
    /// 调整位置及Z序，返回0时读取系统错误。
    fn SetWindowPos(
        window: Handle,
        after: Handle,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    /// 查询当前窗口所属的最近显示器。
    fn MonitorFromWindow(window: Handle, flags: u32) -> Handle;
    /// 向正确大小的结构体写入显示器和工作区。
    fn GetMonitorInfoW(monitor: Handle, info: *mut MonitorInfo) -> i32;
    /// 查询活窗全局物理外框。
    fn GetWindowRect(window: Handle, rect: *mut Rect) -> i32;
    /// 返回客户区尺寸，坐标原点为零。
    fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    /// 把客户区左上角转换为桌面物理坐标。
    fn ClientToScreen(window: Handle, point: *mut NativePoint) -> i32;
    /// 释放现有鼠标捕获，使系统接管窗口拖动。
    fn ReleaseCapture() -> i32;
    /// 只释放当前浮窗自己拥有的捕获，不干扰其它窗口。
    fn GetCapture() -> Handle;
    /// 同步发送左键标题区按下，参数不携带外部指针。
    fn SendMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> isize;
    #[cfg(target_pointer_width = "64")]
    /// 读取 64 位窗口样式或指针属性。
    fn GetWindowLongPtrW(window: Handle, index: i32) -> isize;
    #[cfg(target_pointer_width = "64")]
    /// 更新 64 位窗口样式或指针属性。
    fn SetWindowLongPtrW(window: Handle, index: i32, value: isize) -> isize;
    #[cfg(target_pointer_width = "32")]
    /// 读取 32 位窗口样式属性。
    fn GetWindowLongW(window: Handle, index: i32) -> i32;
    #[cfg(target_pointer_width = "32")]
    /// 更新 32 位窗口样式属性。
    fn SetWindowLongW(window: Handle, index: i32, value: i32) -> i32;
}
#[link(name = "kernel32")]
// SAFETY: 以下声明与 kernel32 导出的线程错误码原型一致，均无指针参数。
unsafe extern "system" {
    /// Win32样式API的零返回需要结合线程错误码解释。
    fn SetLastError(error: u32);
    /// 读取本次原生调用错误码，不把合法旧样式0误判失败。
    fn GetLastError() -> u32;
}

/// 穿透只拥有两个扩展样式位；其它原生属性在每次提交时重新合并。
const MOUSE_STYLE_BITS: isize = 0x80000 | 0x20;

/// checked样式调用兼容32/64位，返回0本身不代表失败。
fn checked_style(hwnd: Handle, set: Option<isize>) -> anyhow::Result<isize> {
    // SAFETY: hwnd 由 `native` 从仍存活的 GPUI 窗口取得且在 UI 线程使用；清零与读取线程错误码只影响当前线程。
    unsafe {
        SetLastError(0);
        let value = style(hwnd, -20, set);
        let error = GetLastError();
        anyhow::ensure!(value != 0 || error == 0, "mouse passthrough style: {}", std::io::Error::from_raw_os_error(error as i32));
        Ok(value)
    }
}

/// 只刷新样式缓存，不改变位置、尺寸、Z序或激活状态。
fn write_mouse_bits(hwnd: Handle, bits: isize) -> anyhow::Result<()> {
    let current = checked_style(hwnd, None)?;
    checked_style(hwnd, Some((current & !MOUSE_STYLE_BITS) | bits))?;
    // SAFETY: hwnd 来自当前 UI 线程拥有的活窗；只刷新框架（不移动、不改尺寸与 Z 序），不转移句柄所有权。
    anyhow::ensure!(unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0004 | 0x0010 | 0x0020) } != 0,
        "mouse passthrough style refresh: {}", std::io::Error::last_os_error());
    anyhow::ensure!(checked_style(hwnd, None)? & MOUSE_STYLE_BITS == bits, "mouse passthrough style readback mismatch");
    Ok(())
}

/// 开启时记住原位，关闭只还原自己拥有的位；失败保留恢复凭据。
pub fn set_mouse_passthrough(window: &Window, enabled: bool, state: &mut super::MousePassthroughState) -> anyhow::Result<()> {
    let hwnd = native(window)?;
    let before = checked_style(hwnd, None)? & MOUSE_STYLE_BITS;
    if enabled && state.original_bits.is_none() { state.original_bits = Some(before); }
    // 显式关闭始终撤销TRANSPARENT；LAYERED恢复首次进入前的所有权。
    let target = if enabled { MOUSE_STYLE_BITS } else { state.original_bits.unwrap_or(before) & !0x20 };
    match write_mouse_bits(hwnd, target) {
        Ok(()) => {
            state.confirmed = Some(target & 0x20 != 0);
            anyhow::ensure!(state.confirmed == Some(enabled), "mouse passthrough original style is still transparent");
            if !enabled { state.original_bits = None; }
            // SAFETY: HWND是当前所属线程的活窗，只释放本窗捕获。
            if enabled && unsafe { GetCapture() } == hwnd { unsafe { ReleaseCapture(); } }
            Ok(())
        }
        Err(error) => {
            // 回滚本次进入前的位，而非首次进入前的位；重新读取保留其它字段的更新。
            let rollback = write_mouse_bits(hwnd, before);
            let actual = checked_style(hwnd, None).ok().map(|style| style & MOUSE_STYLE_BITS);
            state.confirmed = if rollback.is_ok() { actual.map(|bits| bits & 0x20 != 0) } else { None };
            anyhow::bail!("mouse passthrough apply failed: {error}; rollback={rollback:?}; actual={actual:?}")
        }
    }
}
/// 不存储HWND，避免已销毁窗口的句柄被另一个原生窗口复用。
fn native(window: &Window) -> anyhow::Result<Handle> {
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .as_raw()
    else {
        anyhow::bail!("HWND unavailable")
    };
    Ok(handle.hwnd.get() as Handle)
}
/// 查询并修改所属线程活窗的原生样式。
/// # Safety
/// 调用方须保证 `window` 是当前线程拥有、仍然存活的窗口句柄，`index` 是合法的窗口长整型索引。
unsafe fn style(window: Handle, index: i32, set: Option<isize>) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        if let Some(value) = set {
            // SAFETY: 由本函数的调用约定保证句柄与索引有效。
            unsafe { SetWindowLongPtrW(window, index, value) }
        } else {
            // SAFETY: 由本函数的调用约定保证句柄与索引有效；只读取。
            unsafe { GetWindowLongPtrW(window, index) }
        }
    }
    #[cfg(target_pointer_width = "32")]
    {
        if let Some(value) = set {
            // SAFETY: 由本函数的调用约定保证句柄与索引有效；32 位目标的样式值可无损截断为 i32。
            unsafe { SetWindowLongW(window, index, value as i32) as isize }
        } else {
            // SAFETY: 由本函数的调用约定保证句柄与索引有效；只读取。
            unsafe { GetWindowLongW(window, index) as isize }
        }
    }
}
/// 工作区按窗口DPI换算为GPUI逻辑单位，实际跨屏行为单独验证。
pub fn work_area(window: &Window) -> Bounds<gpui_kit::Pixels> {
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: Rect::default(),
        work: Rect::default(),
        flags: 0,
    };
    // SAFETY: window 来自仍存活的 GPUI 窗口；info 是本栈上已按结构体大小初始化的可写缓冲。
    let ok = native(window).is_ok_and(|window| unsafe {
        GetMonitorInfoW(MonitorFromWindow(window, 2), &mut info) != 0
    });
    if ok {
        let scale = window.scale_factor();
        Bounds::new(
            point(
                px(info.work.left as f32 / scale),
                px(info.work.top as f32 / scale),
            ),
            size(
                px((info.work.right - info.work.left) as f32 / scale),
                px((info.work.bottom - info.work.top) as f32 / scale),
            ),
        )
    } else {
        Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)))
    }
}
/// 客户区位置与画布尺寸转换为全局逻辑单位，避免锁定边框被计入持久尺寸。
pub fn bounds(window: &Window) -> Bounds<gpui_kit::Pixels> {
    if let Ok(frame) = native(window).and_then(ClientFrame::read) {
        let scale = window.scale_factor();
        Bounds::new(
            point(px(frame.origin.x as f32 / scale), px(frame.origin.y as f32 / scale)),
            size(
                px((frame.client.right - frame.client.left) as f32 / scale),
                px((frame.client.bottom - frame.client.top) as f32 / scale),
            ),
        )
    } else {
        window.bounds()
    }
}
/// 工具窗无任务栏入口，锁定关闭边缘缩放，置顶不激活窗口。
pub fn apply(window: &Window, prefs: PetWindowPreferences, settings: bool) -> anyhow::Result<()> {
    let hwnd = native(window)?;
    // SAFETY: HWND来自当前GPUI活窗，只在所属UI线程调用。
    unsafe {
        let ex = style(hwnd, -20, None);
        style(hwnd, -20, Some((ex | 0x80) & !0x40000));
        let old = style(hwnd, -16, None);
        style(
            hwnd,
            -16,
            Some(if prefs.locked || settings {
                old & !(0x40000 | 0x10000)
            } else {
                old | 0x40000 | 0x10000
            }),
        );
        let after = if prefs.always_on_top || settings {
            -1isize
        } else {
            -2isize
        } as Handle;
        anyhow::ensure!(
            SetWindowPos(hwnd, after, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010 | 0x0020) != 0,
            "apply auxiliary window: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}
/// GPUI当前Windows后端没有实现start_window_move，显式交给系统拖动。
pub fn start_drag(window: &Window, cx: &gpui_kit::App) {
    if let Ok(hwnd) = native(window) {
        cx.foreground_executor()
            .spawn(async move {
                // SAFETY: 在前台执行器（UI 线程）调用；hwnd 由同一窗口在本次按下时取得，标题栏拖动消息不涉及指针参数。
                unsafe {
                    ReleaseCapture();
                    SendMessageW(hwnd, 0x00a1, 2, 0);
                }
            })
            .detach();
    }
}

/// 原子提交位置和尺寸，不激活、不改变Z序，避免单独resize晚到。
pub fn set_geometry(window: &mut Window, bounds: Bounds<gpui_kit::Pixels>) -> anyhow::Result<()> {
    let scale = window.scale_factor();
    let hwnd = native(window)?;
    let frame = ClientFrame::read(hwnd)?;
    let (x, y, width, height) = frame.outer(
        (f32::from(bounds.origin.x) * scale).round() as i32,
        (f32::from(bounds.origin.y) * scale).round() as i32,
        (f32::from(bounds.size.width) * scale).round() as i32,
        (f32::from(bounds.size.height) * scale).round() as i32);
    // SAFETY: hwnd 来自当前 UI 线程拥有的活窗；只移动并改尺寸，不改变 Z 序与激活状态。
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x, y, width, height,
            0x0004 | 0x0010,
        )
    };
    anyhow::ensure!(
        ok != 0,
        "resize auxiliary window: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

/// Windows现有工作区先验证保存位置，跨屏DPI候选仍由实际Windows验收补齐。
pub fn restoration_area(
    window: &Window,
    stored: Option<loki_metris_core::PetOverlayPosition>,
    prefs: PetWindowPreferences,
) -> (Bounds<gpui_kit::Pixels>, f64, bool) {
    let area = work_area(window);
    let scale = f64::from(window.scale_factor());
    let range = super::super::geometry::range(area, prefs.layout);
    let (width, height) =
        loki_metris_core::pet_window_dimensions(prefs.layout, range.constrain(prefs.pet_size));
    let physical = loki_metris_core::PetOverlayWorkArea {
        x: (f64::from(f32::from(area.origin.x)) * scale).round() as i32,
        y: (f64::from(f32::from(area.origin.y)) * scale).round() as i32,
        width: (f64::from(f32::from(area.size.width)) * scale).round() as u32,
        height: (f64::from(f32::from(area.size.height)) * scale).round() as u32,
    };
    (
        area,
        scale,
        loki_metris_core::resolve_pet_overlay_position(
            stored,
            (
                (width * scale).round() as u32,
                (height * scale).round() as u32,
            ),
            &[physical],
        )
        .is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{core::w, Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow,
        WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW}};

    /// 真实 Win32 边框切换后内容原点和画布尺寸保持一致，不能累计非客户区。
    #[test]
    fn client_size_survives_lock_style_changes() {
        // SAFETY: 测试窗口归当前线程独占，隐藏创建，所有调用在销毁前同步完成。
        unsafe {
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("LokiMetris client geometry"),
                WS_OVERLAPPEDWINDOW, 100, 100, 180, 200, None, None, None, None).unwrap();
            for locked in [false, true, false] {
                let old = style(hwnd.0, -16, None);
                style(hwnd.0, -16, Some(if locked { old & !0x40000 } else { old | 0x40000 }));
                assert_ne!(SetWindowPos(hwnd.0, std::ptr::null_mut(), 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0004 | 0x0010 | 0x0020), 0);
                let frame = ClientFrame::read(hwnd.0).unwrap();
                let (x, y, width, height) = frame.outer(120, 130, 128, 160);
                assert_ne!(SetWindowPos(hwnd.0, std::ptr::null_mut(), x, y, width, height, 0x0004 | 0x0010), 0);
                let actual = ClientFrame::read(hwnd.0).unwrap();
                assert_eq!((actual.origin.x, actual.origin.y), (120, 130));
                assert_eq!((actual.client.right, actual.client.bottom), (128, 160));
            }
            DestroyWindow(hwnd).unwrap();
        }
    }
}
