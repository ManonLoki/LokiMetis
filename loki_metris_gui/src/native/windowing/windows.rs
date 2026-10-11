//! Win32 物理坐标只在宿主边界转换；core 始终接收屏内外框 DIP 与客户区尺寸。

use super::{DesktopSnapshot, NativeDisplay, NativeWindowState};
use gpui_kit::{App, DisplayId, Window};
use loki_metris_core::window_placement::*;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{core::PCWSTR, Win32::{Foundation::{HWND, POINT, RECT}, Graphics::Gdi::*,
    UI::{HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI}, WindowsAndMessaging::*}}};

/// 捕获时只接受当前 UI 线程及本进程拥有的活窗，后续变更由显示租约再次复核。
pub(super) fn native(window: &Window) -> anyhow::Result<HWND> {
    let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
        .map_err(|_| anyhow::anyhow!("window_handle_unavailable"))?.as_raw() else {
        anyhow::bail!("window_win32_handle_unavailable");
    };
    let hwnd = HWND(handle.hwnd.get() as _);
    anyhow::ensure!(valid(hwnd), "window_owner_invalid");
    Ok(hwnd)
}

/// 每次原生读写都拒绝失效、跨线程或被其它进程复用的句柄。
pub(super) fn valid(hwnd: HWND) -> bool {
    use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    // SAFETY: 查询 API 可接收失效 HWND；输出进程字段在本栈中有效。
    unsafe {
        let mut process = 0;
        IsWindow(Some(hwnd)).as_bool() && GetWindowThreadProcessId(hwnd, Some(&mut process)) == GetCurrentThreadId()
            && process == GetCurrentProcessId()
    }
}

/// 同一入口只捕获一次指针；拓扑刷新不会改变已经冻结的目标。
pub(crate) fn capture_desktop(cx: &App) -> anyhow::Result<DesktopSnapshot> { snapshot(cx, true) }

/// 迁屏后刷新有效显示器和真实 DPI，保留调用方持有的入口指针身份。
pub(crate) fn capture_topology(cx: &App) -> anyhow::Result<DesktopSnapshot> { snapshot(cx, false) }

/// GPUI DisplayId 在 Windows 是 HMONITOR；稳定身份来自设备接口而非 DISPLAYn 编号。
fn snapshot(cx: &App, pointer: bool) -> anyhow::Result<DesktopSnapshot> {
    let pointer_monitor = if pointer {
        let mut point = POINT::default();
        // SAFETY: POINT 输出有效；不修改光标位置或系统配置。
        unsafe { GetCursorPos(&mut point).ok().map(|_| MonitorFromPoint(point, MONITOR_DEFAULTTONULL)) }
    } else { None };
    let mut displays = Vec::new();
    let mut pointer_key = None;
    for display in cx.displays() {
        let id = display.id();
        let monitor = HMONITOR(u64::from(id) as _);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of_val(&info) as u32;
        let mut device = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        let (mut dpi_x, mut dpi_y) = (0, 0);
        // SAFETY: 所有输出均匹配 Win32 ABI；句柄只在本次拓扑快照中使用。
        unsafe {
            anyhow::ensure!(GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool(), "window_monitor_unavailable");
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).map_err(|error| anyhow::anyhow!(error.to_string()))?;
            anyhow::ensure!(EnumDisplayDevicesW(PCWSTR(info.szDevice.as_ptr()), 0, &mut device,
                EDD_GET_DEVICE_INTERFACE_NAME).as_bool(), "window_display_identity_unavailable");
        }
        let key = String::from_utf16(&device.DeviceID[..device.DeviceID.iter().position(|value| *value == 0).unwrap_or(device.DeviceID.len())])?;
        anyhow::ensure!(!key.is_empty() && !displays.iter().any(|display: &NativeDisplay| display.area.key == key), "window_display_identity_ambiguous");
        anyhow::ensure!(dpi_x > 0 && dpi_x == dpi_y, "window_display_dpi_invalid");
        let scale = f64::from(dpi_x) / 96.;
        let monitor_rect = info.monitorInfo.rcMonitor;
        let work = info.monitorInfo.rcWork;
        if pointer_monitor == Some(monitor) { pointer_key = Some(key.clone()); }
        displays.push(NativeDisplay { id, global_origin: WindowDipPoint { x: f64::from(monitor_rect.left), y: f64::from(monitor_rect.top) },
            area: WindowDisplayArea { key, primary: info.monitorInfo.dwFlags & 1 != 0, scale,
                work_area: WindowDipRect { origin: WindowDipPoint { x: f64::from(work.left - monitor_rect.left) / scale,
                    y: f64::from(work.top - monitor_rect.top) / scale },
                    size: WindowDipSize { width: f64::from(work.right - work.left) / scale, height: f64::from(work.bottom - work.top) / scale } } } });
    }
    anyhow::ensure!(!displays.is_empty(), "window_displays_unavailable");
    Ok(DesktopSnapshot { displays, pointer: pointer_key, global_top: 0. })
}

/// 活窗所属屏以原生 HWND 决定，不把保存屏或鼠标屏冒充当前屏。
pub(crate) fn window_display_key(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<String> {
    display(native(window)?, desktop).map(|display| display.area.key.clone())
}

/// 当前原生屏必须仍存在于本次拓扑，拔屏的旧句柄不能直接复用。
pub(super) fn display(hwnd: HWND, desktop: &DesktopSnapshot) -> anyhow::Result<&NativeDisplay> {
    // SAFETY: 调用方已经核对 HWND 所属线程与活窗。
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let id = DisplayId::new(monitor.0 as u64);
    desktop.displays.iter().find(|display| display.id == id).ok_or_else(|| anyhow::anyhow!("window_display_removed"))
}

/// 原生客户区和外框作为同一快照读取，真实四边距不依赖主题或 DPI 猜测。
pub(super) fn geometry(hwnd: HWND) -> anyhow::Result<(RECT, RECT, POINT, f64)> {
    anyhow::ensure!(valid(hwnd), "window_owner_invalid");
    let (mut frame, mut client, mut origin) = (RECT::default(), RECT::default(), POINT::default());
    // SAFETY: 输出布局符合 API；不保存借用指针。
    unsafe {
        GetWindowRect(hwnd, &mut frame).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        GetClientRect(hwnd, &mut client).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        anyhow::ensure!(ClientToScreen(hwnd, &mut origin).as_bool(), "window_client_origin_unavailable");
        let dpi = GetDpiForWindow(hwnd);
        anyhow::ensure!(dpi > 0, "window_dpi_unavailable");
        Ok((frame, client, origin, f64::from(dpi) / 96.))
    }
}

/// 边距以本窗当前 DPI 换算；跨屏后必须重新读取，不能沿用起点屏边框。
pub(crate) fn frame_insets(window: &Window) -> anyhow::Result<WindowFrameInsets> { insets(native(window)?) }

/// 主窗与辅助窗共用实际客户区映射，避免边框混入持久内容尺寸。
pub(super) fn insets(hwnd: HWND) -> anyhow::Result<WindowFrameInsets> {
    let (frame, client, origin, scale) = geometry(hwnd)?;
    Ok(WindowFrameInsets { left: f64::from(origin.x - frame.left) / scale,
        top: f64::from(origin.y - frame.top) / scale,
        right: f64::from(frame.right - origin.x - client.right) / scale,
        bottom: f64::from(frame.bottom - origin.y - client.bottom) / scale })
}

/// 隐藏、最小化和全屏不覆盖普通状态；最大化只更新标志，保留旧普通锚点。
pub(crate) fn capture_window(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<NativeWindowState> {
    let hwnd = native(window)?;
    // SAFETY: HWND 已验证且当前 Window 借用保持有效。
    let (maximized, transient) = unsafe { (IsZoomed(hwnd).as_bool(), IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || window.is_fullscreen()) };
    let normal = if maximized || transient { None } else { Some(capture_normal(window, desktop)?) };
    Ok(NativeWindowState { normal, maximized, transient })
}

/// 隐藏创建与显示前读回允许捕获普通几何；完整状态是否合法仍由 core 决定。
pub(crate) fn capture_normal(window: &Window, desktop: &DesktopSnapshot) -> anyhow::Result<WindowStateV2> {
    let hwnd = native(window)?;
    let target = display(hwnd, desktop)?;
    let (frame, client, _, scale) = geometry(hwnd)?;
    capture_window_state(&target.area, WindowDipPoint { x: (f64::from(frame.left) - target.global_origin.x) / scale,
        y: (f64::from(frame.top) - target.global_origin.y) / scale },
        WindowDipSize { width: f64::from(client.right) / scale, height: f64::from(client.bottom) / scale }, insets(hwnd)?, false)
        .ok_or_else(|| anyhow::anyhow!("window_geometry_invalid"))
}

/// 旧主窗存的是全局客户区 DIP，只接受完整位于唯一工作区的解释。
pub(crate) fn legacy_main(desktop: &DesktopSnapshot, bounds: [f32; 4], insets: WindowFrameInsets, maximized: bool) -> Option<WindowStateV2> {
    let candidates = desktop.displays.iter().filter_map(|display| {
        let origin = WindowDipPoint { x: f64::from(bounds[0]) - display.global_origin.x / display.area.scale - insets.left,
            y: f64::from(bounds[1]) - display.global_origin.y / display.area.scale - insets.top };
        let size = WindowDipSize { width: bounds[2].into(), height: bounds[3].into() };
        let work = display.area.work_area;
        (origin.x >= work.origin.x && origin.y >= work.origin.y
            && origin.x + size.width + insets.left + insets.right <= work.origin.x + work.size.width
            && origin.y + size.height + insets.top + insets.bottom <= work.origin.y + work.size.height)
            .then(|| capture_window_state(&display.area, origin, size, insets, maximized)).flatten()
    }).collect::<Vec<_>>();
    unique_legacy_window_state(&candidates).cloned()
}
