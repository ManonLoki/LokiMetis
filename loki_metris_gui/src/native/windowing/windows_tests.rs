//! 使用本线程真实 Win32 窗口核验恢复序列、客户区尺寸及实际目标屏。

use super::*;
use windows::{core::w, Win32::{Foundation::RECT, Graphics::Gdi::*, UI::HiDpi::*}};

/// 即使用例断言失败也销毁本线程窗口，恢复原先的 DPI awareness。
struct TestWindow {
    /// 测试窗口句柄。
    hwnd: HWND,
    /// 创建窗口时使用的 DPI 感知上下文。
    dpi: DPI_AWARENESS_CONTEXT,
}
impl Drop for TestWindow {
    /// 只销毁测试创建的 HWND，不触碰用户窗口或系统显示设置。
    // SAFETY: hwnd 是本测试创建且仅由 TestWindow 拥有的窗口；恢复的是创建前保存的线程 DPI 上下文。
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.hwnd); let _ = SetThreadDpiAwarenessContext(self.dpi); } }
}

/// 创建隐藏的系统类窗口，测试时与 GPUI 一样采用 per-monitor v2 DPI 语义。
fn test_window() -> TestWindow {
    // SAFETY: 在测试线程创建系统 STATIC 类窗口，字符串为编译期 NUL 结尾常量；返回句柄由 TestWindow 负责销毁。
    unsafe {
        let dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("LokiMetris placement regression"),
            WS_OVERLAPPEDWINDOW, 120, 140, 400, 300, None, None, None, None).unwrap();
        TestWindow { hwnd, dpi }
    }
}

/// 从测试窗当前真实屏读取工作区；不声明不存在的第二块物理显示器。
fn desktop(hwnd: HWND) -> DesktopSnapshot {
    // SAFETY: hwnd 是本测试创建的存活窗口；info 已设置 cbSize 且为本栈可写结构。
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        assert!(GetMonitorInfoW(monitor, &mut info).as_bool());
        let scale = f64::from(GetDpiForWindow(hwnd)) / 96.;
        let RECT { left, top, .. } = info.rcMonitor;
        let work = info.rcWork;
        DesktopSnapshot { pointer: Some("test-monitor".into()), displays: vec![super::super::NativeDisplay {
            id: gpui_kit::DisplayId::new(monitor.0 as u64), global_origin: WindowDipPoint { x: left.into(), y: top.into() },
            area: WindowDisplayArea { key: "test-monitor".into(), primary: true, scale, work_area: WindowDipRect {
                origin: WindowDipPoint { x: f64::from(work.left - left) / scale, y: f64::from(work.top - top) / scale },
                size: WindowDipSize { width: f64::from(work.right - work.left) / scale, height: f64::from(work.bottom - work.top) / scale } } } }] }
    }
}

/// 最大化后最小化的 restore 必须落回普通态，随后定位的真实客户区保持请求的 DIP 尺寸。
#[test]
fn maximized_then_minimized_normalizes_and_places_exact_client_size() {
    let window = test_window();
    let host = WindowHost::from_handle(window.hwnd).unwrap();
    // SAFETY: hwnd 是本测试创建的存活窗口，只改变其显示状态。
    unsafe { let _ = ShowWindow(window.hwnd, SW_MAXIMIZE); let _ = ShowWindow(window.hwnd, SW_MINIMIZE); }
    host.normalize().unwrap();
    assert!(!host.maximized());
    // SAFETY: hwnd 是本测试创建的存活窗口，只读取最小化状态。
    assert!(!unsafe { IsIconic(window.hwnd) }.as_bool());
    let desktop = desktop(window.hwnd);
    let plan = restore_window_placement(&desktop.areas(), WindowDisplaySelection::default(), WindowPlacementRequest {
        role: WindowPlacementRole::PetConfiguration, saved: None, legacy_content_size: None, legacy_maximized: false,
        frame_insets: native::insets(window.hwnd).unwrap() }).unwrap();
    host.prime_display(&desktop, &plan).unwrap();
    let plan = host.apply_auxiliary(&desktop, &plan).unwrap();
    let (frame, client, _, scale) = native::geometry(window.hwnd).unwrap();
    assert!((f64::from(client.right) / scale - plan.content_size.width).abs() < 0.01);
    assert!((f64::from(client.bottom) / scale - plan.content_size.height).abs() < 0.01);
    let display = &desktop.displays[0];
    assert!(((f64::from(frame.left) - display.global_origin.x) / scale - plan.origin.x).abs() < 0.01);
    assert!(((f64::from(frame.top) - display.global_origin.y) / scale - plan.origin.y).abs() < 0.01);
}

/// 负物理原点不能先按其它屏 scale 换算；GPUI 初始 bounds 必须仍归属目标屏。
#[test]
fn initial_bounds_preserve_negative_physical_origin_at_fractional_scale() {
    let window = test_window();
    let mut desktop = desktop(window.hwnd);
    desktop.displays[0].global_origin = WindowDipPoint { x: -1920., y: -240. };
    desktop.displays[0].area.scale = 1.25;
    let plan = WindowPlacement { display_key: "test-monitor".into(), origin: WindowDipPoint { x: 128., y: 96. },
        content_size: WindowDipSize { width: 320., height: 470. }, maximized: false, overflows_work_area: false };
    let initial = super::super::placement_bounds(&desktop, &plan).unwrap();
    assert!((f64::from(f32::from(initial.origin.x)) * 1.25 - (-1920. + 160.)).abs() < 0.01);
    assert!((f64::from(f32::from(initial.origin.y)) * 1.25 - (-240. + 120.)).abs() < 0.01);
}

/// 锁定既关闭边缘缩放，也关闭系统最大化入口；解锁恢复原生调整能力。
#[test]
fn auxiliary_lock_changes_native_resize_and_maximize_capabilities() {
    let window = test_window();
    let host = WindowHost::from_handle(window.hwnd).unwrap();
    for resizable in [true, false, true] {
        host.configure_auxiliary(resizable, false).unwrap();
        // SAFETY: hwnd 是本测试创建的存活窗口，只读取样式。
        let style = unsafe { GetWindowLongPtrW(window.hwnd, GWL_STYLE) };
        assert_eq!(style & WS_THICKFRAME.0 as isize != 0, resizable);
        assert_eq!(style & WS_MAXIMIZEBOX.0 as isize != 0, resizable);
        // SAFETY: hwnd 是本测试创建的存活窗口，只读取扩展样式。
        assert_eq!(unsafe { GetWindowLongPtrW(window.hwnd, GWL_EXSTYLE) } & WS_EX_APPWINDOW.0 as isize, 0);
    }
}
