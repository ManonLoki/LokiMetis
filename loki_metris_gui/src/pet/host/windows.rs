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
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
/// Win32工作区查询的固定C布局。
#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}
#[link(name = "user32")]
unsafe extern "system" {
    /// 有效窗口显示命令，不以返回旧可见性作为成功证据。
    fn ShowWindow(window: Handle, command: i32) -> i32;
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
unsafe fn style(window: Handle, index: i32, set: Option<isize>) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        if let Some(value) = set {
            unsafe { SetWindowLongPtrW(window, index, value) }
        } else {
            unsafe { GetWindowLongPtrW(window, index) }
        }
    }
    #[cfg(target_pointer_width = "32")]
    {
        if let Some(value) = set {
            unsafe { SetWindowLongW(window, index, value as i32) as isize }
        } else {
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
/// 活窗外框转换为全局逻辑单位。
pub fn bounds(window: &Window) -> Bounds<gpui_kit::Pixels> {
    let mut rect = Rect::default();
    if native(window).is_ok_and(|window| unsafe { GetWindowRect(window, &mut rect) != 0 }) {
        let scale = window.scale_factor();
        Bounds::new(
            point(px(rect.left as f32 / scale), px(rect.top as f32 / scale)),
            size(
                px((rect.right - rect.left) as f32 / scale),
                px((rect.bottom - rect.top) as f32 / scale),
            ),
        )
    } else {
        window.bounds()
    }
}
/// 使用无激活显示命令，隐藏不会隐藏同一进程的主窗口。
pub fn set_visible(window: &Window, visible: bool) -> anyhow::Result<()> {
    unsafe {
        ShowWindow(native(window)?, if visible { 4 } else { 0 });
    }
    Ok(())
}
/// 只修改位置，保持尺寸、Z序和焦点。
pub fn move_to(window: &Window, x: f64, y: f64) -> anyhow::Result<()> {
    let scale = f64::from(window.scale_factor());
    let ok = unsafe {
        SetWindowPos(
            native(window)?,
            std::ptr::null_mut(),
            (x * scale).round() as i32,
            (y * scale).round() as i32,
            0,
            0,
            0x0001 | 0x0004 | 0x0010,
        )
    };
    anyhow::ensure!(
        ok != 0,
        "move auxiliary window: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
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
                old & !0x40000
            } else {
                old | 0x40000
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
    let ok = unsafe {
        SetWindowPos(
            native(window)?,
            std::ptr::null_mut(),
            (f32::from(bounds.origin.x) * scale).round() as i32,
            (f32::from(bounds.origin.y) * scale).round() as i32,
            (f32::from(bounds.size.width) * scale).round() as i32,
            (f32::from(bounds.size.height) * scale).round() as i32,
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
