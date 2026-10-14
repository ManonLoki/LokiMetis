//! Win32 修改在全部 GPUI 借用之外执行；迁屏后重读真实边框并返回像素对齐目标。

use super::{DesktopSnapshot, windows as native};
use gpui_kit::Window;
use loki_metris_core::window_placement::*;
use std::{cell::Cell, rc::Rc, time::{Duration, Instant}};
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

/// 主线程共享的几何信号；仅 bounds 回调更新，空闲轮询不持续读拓扑或写文件。
struct Signals {
    revision: Cell<u64>,
    changed: Cell<Option<Instant>>,
}

/// HWND 始终借用 GPUI 所有权；Rc 防止跨线程，原生操作前核对活窗与进程。
#[derive(Clone)]
pub(crate) struct WindowHost {
    hwnd: HWND,
    activation: crate::native::windows::MainWindow,
    signals: Rc<Signals>,
}

impl WindowHost {
    /// 在活窗借用中复制原生句柄，后续显示事务不持有 App/Window 借用。
    pub fn capture(window: &Window) -> anyhow::Result<Self> {
        Self::from_handle(native::native(window)?)
    }

    /// 所有宿主实例均从已验证的同线程窗口建立，测试不模拟 Win32 几何。
    fn from_handle(hwnd: HWND) -> anyhow::Result<Self> {
        Ok(Self { hwnd, activation: crate::native::windows::MainWindow::from_handle(hwnd.0)?,
            signals: Rc::new(Signals { revision: Cell::new(0), changed: Cell::new(None) }) })
    }

    /// bounds 通知只标记待保存版本，不同步进入原生事件回调。
    pub fn geometry_changed(&self) {
        self.signals.revision.set(self.signals.revision.get().wrapping_add(1));
        self.signals.changed.set(Some(Instant::now()));
    }

    /// 最后一帧静默后才读取普通几何；计时只用来合并事件，恢复成功仍需读回匹配。
    pub fn settled_revision(&self) -> Option<u64> {
        (native::valid(self.hwnd) && self.signals.changed.get().is_none_or(|time| time.elapsed() >= Duration::from_millis(250)))
            .then(|| self.revision())
    }

    /// 同一宿主的单调版本供已有原生调度去重。
    pub fn revision(&self) -> u64 { self.signals.revision.get() }

    /// 恢复普通态不把 iconic 或最大化坐标当作保存值，隐藏窗口在准备后保持隐藏。
    pub fn normalize(&self) -> anyhow::Result<()> {
        anyhow::ensure!(native::valid(self.hwnd), "window_owner_invalid");
        // SAFETY: 当前线程拥有 HWND，ShowWindow 可同步派发消息，调用方已释放 GPUI 借用。
        unsafe {
            let visible = IsWindowVisible(self.hwnd).as_bool();
            if IsIconic(self.hwnd).as_bool() { let _ = ShowWindow(self.hwnd, SW_RESTORE); }
            if IsZoomed(self.hwnd).as_bool() { let _ = ShowWindow(self.hwnd, SW_RESTORE); }
            if !visible { let _ = ShowWindow(self.hwnd, SW_HIDE); }
            anyhow::ensure!(!IsIconic(self.hwnd).as_bool() && !IsZoomed(self.hwnd).as_bool(), "window_normalization_failed");
        }
        Ok(())
    }

    /// 第一阶段只迁入目标屏，让 WM_DPICHANGED 更新 DPI；调用方核对租约后重新规划。
    pub fn prime_display(&self, desktop: &DesktopSnapshot, plan: &WindowPlacement) -> anyhow::Result<()> {
        let display = desktop.display(&plan.display_key).ok_or_else(|| anyhow::anyhow!("window_display_removed"))?;
        self.place(display, plan, native::insets(self.hwnd)?)?;
        Ok(())
    }

    /// 最终规划必须采用目标屏真实边距；这里只有一次原生修改，之后由 owner 读回。
    pub fn apply_auxiliary(&self, desktop: &DesktopSnapshot, plan: &WindowPlacement) -> anyhow::Result<WindowPlacement> {
        let display = desktop.display(&plan.display_key).ok_or_else(|| anyhow::anyhow!("window_display_removed"))?;
        anyhow::ensure!(native::display(self.hwnd, desktop)?.id == display.id, "window_target_not_reached");
        let (_, _, _, scale) = native::geometry(self.hwnd)?;
        anyhow::ensure!((scale - display.area.scale).abs() < 0.001, "window_target_dpi_not_reached");
        let insets = native::insets(self.hwnd)?;
        let mut aligned = plan.clone();
        aligned.origin.x = ((display.global_origin.x + aligned.origin.x * scale).round() - display.global_origin.x) / scale;
        aligned.origin.y = ((display.global_origin.y + aligned.origin.y * scale).round() - display.global_origin.y) / scale;
        aligned.content_size.width = (aligned.content_size.width * scale).round() / scale;
        aligned.content_size.height = (aligned.content_size.height * scale).round() / scale;
        aligned.maximized = plan.maximized;
        self.place(display, &aligned, insets)?;
        Ok(aligned)
    }

    /// 目标屏物理原点只加一次；内容尺寸及真实边距都按目标 DPI 转物理像素。
    fn place(&self, display: &super::NativeDisplay, plan: &WindowPlacement, insets: WindowFrameInsets) -> anyhow::Result<()> {
        anyhow::ensure!(native::valid(self.hwnd), "window_owner_invalid");
        let scale = display.area.scale;
        let x = (display.global_origin.x + plan.origin.x * scale).round() as i32;
        let y = (display.global_origin.y + plan.origin.y * scale).round() as i32;
        let width = (plan.content_size.width * scale).round() as i32 + ((insets.left + insets.right) * scale).round() as i32;
        let height = (plan.content_size.height * scale).round() as i32 + ((insets.top + insets.bottom) * scale).round() as i32;
        // SAFETY: 数值来自 core 有界几何，所属线程活窗已复核，调用方不持有 GPUI 借用。
        unsafe { SetWindowPos(self.hwnd, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER).map_err(|error| anyhow::anyhow!(error.to_string()))?; }
        self.geometry_changed();
        Ok(())
    }

    /// 工具窗按位修改锁定和置顶属性，其余 GPUI 样式保持当前值。
    pub fn configure_auxiliary(&self, resizable: bool, always_on_top: bool) -> anyhow::Result<()> {
        anyhow::ensure!(native::valid(self.hwnd), "window_owner_invalid");
        // SAFETY: 本产品仅支持64位Windows，样式修改在已验证的所属线程上同步执行。
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize));
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
            let resize = (WS_THICKFRAME | WS_MAXIMIZEBOX).0 as isize;
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, if resizable { style | resize } else { style & !resize });
            SetWindowPos(self.hwnd, Some(if always_on_top { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED).map_err(|error| anyhow::anyhow!(error.to_string()))?;
            anyhow::ensure!(GetWindowLongPtrW(self.hwnd, GWL_STYLE) & resize == if resizable { resize } else { 0 },
                "window_style_readback_failed");
        }
        Ok(())
    }

    /// 最大化发生在普通几何确认之后，避免将屏幕尺寸保存为普通尺寸。
    // SAFETY: 句柄先经 native::valid 确认仍属于当前线程与进程，只改变显示状态。
    pub fn maximize(&self) { if native::valid(self.hwnd) { unsafe { let _ = ShowWindow(self.hwnd, SW_MAXIMIZE); } } }

    /// 实际 Win32 zoom 标志作为持久最大化状态。
    // SAFETY: 句柄先经 native::valid 确认仍属于当前线程与进程，只读取最大化标志。
    pub fn maximized(&self) -> bool { native::valid(self.hwnd) && unsafe { IsZoomed(self.hwnd).as_bool() } }

    /// 前台拒绝策略沿用已验证的主窗桥接；调用方已经检查显示租约。
    pub fn show(&self) { self.activation.show(); }

    /// 隐藏仅操作当前活窗，取消租约由 owner 在原生调用前完成。
    pub fn hide(&self) { self.activation.hide(); }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
