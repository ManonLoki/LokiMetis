//! 原生坐标只在此边界转换；core 接收屏内 DIP，不接收 NSScreen 或全局物理位置。

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_host;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_host;
#[cfg(windows)]
pub(crate) use windows::{capture_desktop, capture_topology, capture_window, capture_normal, frame_insets, window_display_key};
#[cfg(windows)]
pub(crate) use windows_host::WindowHost;
#[cfg(windows)]
pub(crate) use windows::legacy_main;
#[cfg(target_os = "macos")]
mod geometry_settle;
#[cfg(target_os = "macos")]
pub(crate) use macos::{capture_desktop, capture_topology, capture_window, capture_normal, frame_insets, window_display_key};
#[cfg(target_os = "macos")]
pub(crate) use macos_host::WindowHost;
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) use macos_host::configure_auxiliary_window;

use gpui_kit::DisplayId;
use loki_metris_core::window_placement::{WindowDipPoint, WindowDisplayArea, WindowStateV2};

/// 单次原生拓扑的一块屏幕，运行期 ID 和稳定身份各有独立用途。
#[derive(Clone, Debug)]
pub(crate) struct NativeDisplay {
    /// 显式创建窗口所需的运行期 GPUI ID，不持久化。
    pub id: DisplayId,
    /// 已转换成屏内 DIP 的工作区、实际 scale 和持久身份。
    pub area: WindowDisplayArea,
    /// 原生全局原点；macOS 单位为 AppKit 点，Windows 单位为物理像素。
    pub global_origin: WindowDipPoint,
}

/// 捕获入口鼠标屏后随动作传递，后台等待和 defer 不再读取新的鼠标位置。
#[derive(Clone, Debug)]
pub(crate) struct DesktopSnapshot {
    /// 本次有效的显示器拓扑；不长期缓存。
    pub displays: Vec<NativeDisplay>,
    /// 指针所在屏幕稳定身份，失败时交给 core 使用确定回退。
    pub pointer: Option<String>,
    /// AppKit 全局向上坐标转换使用的主屏顶边，保留原生负坐标。
    pub global_top: f64,
}

impl DesktopSnapshot {
    /// 只克隆小型纯数据，交给 core 选择和映射目标屏。
    pub fn areas(&self) -> Vec<WindowDisplayArea> {
        self.displays.iter().map(|display| display.area.clone()).collect()
    }

    /// 拔屏后不接受旧句柄；稳定 key 必须存在于当前入口快照中。
    pub fn display(&self, key: &str) -> Option<&NativeDisplay> {
        self.displays.iter().find(|display| display.area.key == key)
    }
}

/// 首次隐藏创建明确选择运行期屏幕；实际标题栏边距仍在创建后读取并修正。
pub(crate) fn initial_main_bounds(preferences: &crate::preferences::Preferences, desktop: &DesktopSnapshot)
    -> Option<(gpui_kit::WindowBounds, DisplayId)> {
    use loki_metris_core::window_placement::*;
    let placement = restore_window_placement(&desktop.areas(), WindowDisplaySelection {
        pointer: desktop.pointer.as_deref(), current: None,
    }, WindowPlacementRequest {
        role: WindowPlacementRole::Main, saved: preferences.window_state_v2.as_ref(),
        legacy_content_size: preferences.bounds.map(|bounds| WindowDipSize { width: bounds[2].into(), height: bounds[3].into() }),
        legacy_maximized: preferences.maximized, frame_insets: WindowFrameInsets::default(),
    })?;
    let display = desktop.display(&placement.display_key)?;
    let bounds = placement_bounds(desktop, &placement)?;
    Some((gpui_kit::WindowBounds::Windowed(bounds), display.id))
}

/// Windows GPUI 初始 bounds 使用全局物理坐标除以目标屏 scale，AppKit 使用屏内点。
pub(crate) fn placement_bounds(desktop: &DesktopSnapshot, placement: &loki_metris_core::window_placement::WindowPlacement)
    -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let display = desktop.display(&placement.display_key)?;
    #[cfg(windows)]
    let origin = WindowDipPoint { x: display.global_origin.x / display.area.scale + placement.origin.x,
        y: display.global_origin.y / display.area.scale + placement.origin.y };
    #[cfg(not(windows))]
    let origin = { let _ = display; placement.origin };
    Some(gpui_kit::Bounds::new(gpui_kit::point(gpui_kit::px(origin.x as f32), gpui_kit::px(origin.y as f32)),
        gpui_kit::size(gpui_kit::px(placement.content_size.width as f32), gpui_kit::px(placement.content_size.height as f32))))
}

/// 窗口的最终原生读回；最大化期间仅更新状态，普通几何由已有有效记录保留。
#[derive(Clone, Debug)]
pub(crate) struct NativeWindowState {
    /// 普通状态读回成功才产生完整几何记录。
    pub normal: Option<WindowStateV2>,
    /// 原生 zoom 状态，不采用 GPUI 的屏幕大小猜测。
    pub maximized: bool,
    /// 全屏/最小化期间不持久覆盖普通窗口几何。
    pub transient: bool,
}
