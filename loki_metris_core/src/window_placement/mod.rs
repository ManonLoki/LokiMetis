//! 窗口恢复的纯规则：屏内 DIP 与屏幕身份分离，不在不同 DPI 间缩放全局原点。

use serde::{Deserialize, Serialize};

/// 显示器内部的逻辑点；原点是该屏完整区域左上角，而非全局桌面原点。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowDipPoint {
    /// 向右的逻辑距离。
    pub x: f64,
    /// 向下的逻辑距离。
    pub y: f64,
}

/// 与显示器缩放无关的内容尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowDipSize {
    /// 内容宽度，不含原生窗口边框。
    pub width: f64,
    /// 内容高度，不含原生标题栏。
    pub height: f64,
}

/// 原生层转换后的屏内逻辑矩形，工作区可以带菜单栏造成的非零原点。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowDipRect {
    /// 相对完整屏幕原点的位置。
    pub origin: WindowDipPoint,
    /// 矩形逻辑尺寸。
    pub size: WindowDipSize,
}

/// 单次入口快照中的可用屏幕；稳定 key 由原生层生成，不保存临时句柄。
#[derive(Clone, Debug, PartialEq)]
pub struct WindowDisplayArea {
    /// 可跨进程恢复的显示器身份。
    pub key: String,
    /// 已转换为该屏 DIP 的原生可用区。
    pub work_area: WindowDipRect,
    /// 仅用于保存与诊断的实际设备缩放，不能乘以全局位置。
    pub scale: f64,
    /// 原生主屏标志；不是鼠标所在屏的替代值。
    pub primary: bool,
}

/// 原生外框与内容尺寸之差，由目标显示器的原生机制提供。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowFrameInsets {
    /// 内容左侧边框。
    pub left: f64,
    /// 内容上方标题栏或边框。
    pub top: f64,
    /// 内容右侧边框。
    pub right: f64,
    /// 内容下方边框。
    pub bottom: f64,
}

/// 版本化普通窗口几何；不持久化隐藏、最小化或全屏状态。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowStateV2 {
    /// 当前只接受版本 2，未知版本不影响其它偏好。
    pub version: u8,
    /// 保存所属显示器的稳定身份。
    pub display_key: String,
    /// 普通窗口内容 DIP 尺寸。
    pub content_size: WindowDipSize,
    /// 普通窗口外框左上角的屏内 DIP 位置。
    pub origin: WindowDipPoint,
    /// 相对工作区可移动距离的锚点，两个分量均在 0..=1。
    pub anchor: WindowDipPoint,
    /// 保存当时的屏内可用区，用于拒绝畸形状态。
    pub work_area: WindowDipRect,
    /// 保存当时的设备缩放，不参与跨屏全局原点换算。
    pub scale: f64,
    /// 主窗是否应在恢复普通几何后最大化。
    pub maximized: bool,
}

impl WindowStateV2 {
    /// 有界校验仅针对此窗口记录，调用方可独立回退旧尺寸及其它偏好。
    pub fn is_valid(&self) -> bool {
        self.version == 2
            && valid_key(&self.display_key)
            && valid_size(self.content_size)
            && valid_point(self.origin)
            && valid_anchor(self.anchor)
            && valid_rect(self.work_area)
            && valid_scale(self.scale)
    }
}

/// 三类独立原生窗具有不同的默认位置与尺寸合同。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowPlacementRole {
    /// 无状态 1440×900，最小普通内容尺寸 960×640。
    Main,
    /// 无状态位于目标屏右下 16 DIP；尺寸来自桌宠布局规则。
    Pet(WindowDipSize),
    /// 独立配置窗固定 320×470，重开时居中且不恢复长期窗口状态。
    PetConfiguration,
}

/// 入口捕获的屏幕身份；延迟回调复用这份值，不能重新追踪鼠标。
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowDisplaySelection<'a> {
    /// 原生层将入口鼠标点归属到的稳定屏幕身份。
    pub pointer: Option<&'a str>,
    /// 已存在窗口当前所属的稳定屏幕身份。
    pub current: Option<&'a str>,
}

/// 一次主动显示的输入，旧状态只保留可解释尺寸和最大化。
#[derive(Clone, Copy, Debug)]
pub struct WindowPlacementRequest<'a> {
    /// 决定默认尺寸、位置以及最小内容尺寸。
    pub role: WindowPlacementRole,
    /// 有效版本化状态优先于旧尺寸；配置窗忽略此项。
    pub saved: Option<&'a WindowStateV2>,
    /// 旧位置不能唯一解释时仍保留其有效内容尺寸。
    pub legacy_content_size: Option<WindowDipSize>,
    /// 没有有效新记录时读取的旧主窗最大化状态。
    pub legacy_maximized: bool,
    /// 由目标屏原生样式产生的外框边距。
    pub frame_insets: WindowFrameInsets,
}

/// 纯恢复决策；原生层先设置普通外框，再按 maximized 显示并读回实际结果。
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPlacement {
    /// 必须用于显式选屏的稳定身份。
    pub display_key: String,
    /// 外框左上角的目标屏内 DIP，不能作为全局坐标直接传给宿主。
    pub origin: WindowDipPoint,
    /// 保留正常内容尺寸，另由原生层转换为目标屏外框。
    pub content_size: WindowDipSize,
    /// 只允许主窗在普通几何恢复完成后最大化。
    pub maximized: bool,
    /// 屏小于窗口最低尺寸时保留尺寸并确保左上标题区域可操作。
    pub overflows_work_area: bool,
}

/// 指针屏、保存屏、当前屏、主屏依次选择；坏拓扑不参与恢复。
pub fn choose_window_display<'a>(
    displays: &'a [WindowDisplayArea],
    selection: WindowDisplaySelection<'_>,
    saved_key: Option<&str>,
) -> Option<&'a WindowDisplayArea> {
    let valid = |display: &&WindowDisplayArea| valid_display(display);
    for key in [selection.pointer, saved_key, selection.current].into_iter().flatten() {
        if let Some(display) = displays.iter().filter(valid).find(|display| display.key == key) {
            return Some(display);
        }
    }
    displays.iter().filter(valid).find(|display| display.primary)
        .or_else(|| displays.iter().find(|display| valid_display(display)))
}

/// 先选目标屏，再保留同屏位置或跨屏映射相对锚点，最后按工作区夹紧一次。
pub fn restore_window_placement(
    displays: &[WindowDisplayArea],
    selection: WindowDisplaySelection<'_>,
    request: WindowPlacementRequest<'_>,
) -> Option<WindowPlacement> {
    if !valid_insets(request.frame_insets) { return None; }
    let saved = request.saved.filter(|state| state.is_valid())
        .filter(|_| request.role != WindowPlacementRole::PetConfiguration)
        .filter(|state| request.role != WindowPlacementRole::Main || valid_main_size(state.content_size));
    let display = choose_window_display(displays, selection, saved.map(|state| state.display_key.as_str()))?;
    let content_size = match request.role {
        WindowPlacementRole::Main => saved.map(|state| state.content_size)
            .filter(|size| valid_main_size(*size))
            .or(request.legacy_content_size.filter(|size| valid_main_size(*size)))
            .unwrap_or(WindowDipSize { width: 1440., height: 900. }),
        WindowPlacementRole::Pet(size) if valid_size(size) => size,
        WindowPlacementRole::Pet(_) => return None,
        WindowPlacementRole::PetConfiguration => WindowDipSize { width: 320., height: 470. },
    };
    let outer = outer_size(content_size, request.frame_insets);
    let area = display.work_area;
    let anchor_origin = |anchor: WindowDipPoint| WindowDipPoint {
        x: area.origin.x + (area.size.width - outer.width).max(0.) * anchor.x,
        y: area.origin.y + (area.size.height - outer.height).max(0.) * anchor.y,
    };
    let origin = match saved {
        Some(state) if state.display_key == display.key => state.origin,
        Some(state) => anchor_origin(state.anchor),
        None if matches!(request.role, WindowPlacementRole::Pet(_)) => WindowDipPoint {
            x: area.origin.x + area.size.width - outer.width - 16.,
            y: area.origin.y + area.size.height - outer.height - 16.,
        },
        None => anchor_origin(WindowDipPoint { x: 0.5, y: 0.5 }),
    };
    Some(WindowPlacement {
        display_key: display.key.clone(),
        origin: WindowDipPoint {
            x: origin.x.clamp(area.origin.x, area.origin.x + (area.size.width - outer.width).max(0.)),
            y: origin.y.clamp(area.origin.y, area.origin.y + (area.size.height - outer.height).max(0.)),
        },
        content_size,
        maximized: request.role == WindowPlacementRole::Main
            && saved.map_or(request.legacy_maximized, |state| state.maximized),
        overflows_work_area: outer.width > area.size.width || outer.height > area.size.height,
    })
}

/// 从最终原生读回的普通几何生成可恢复状态；迁屏中间反馈不能调用此函数提交。
pub fn capture_window_state(
    display: &WindowDisplayArea,
    origin: WindowDipPoint,
    content_size: WindowDipSize,
    frame_insets: WindowFrameInsets,
    maximized: bool,
) -> Option<WindowStateV2> {
    if !valid_display(display) || !valid_point(origin) || !valid_size(content_size)
        || !valid_insets(frame_insets) { return None; }
    let outer = outer_size(content_size, frame_insets);
    let area = display.work_area;
    let relative = |position: f64, start: f64, available: f64| {
        if available > 0. { ((position - start) / available).clamp(0., 1.) } else { 0. }
    };
    Some(WindowStateV2 {
        version: 2,
        display_key: display.key.clone(),
        content_size,
        origin,
        anchor: WindowDipPoint {
            x: relative(origin.x, area.origin.x, area.size.width - outer.width),
            y: relative(origin.y, area.origin.y, area.size.height - outer.height),
        },
        work_area: area,
        scale: display.scale,
        maximized,
    })
}

/// 最大化迁屏保留可信普通尺寸与锚点，只将其映射到已确认的当前屏，拒绝猜测缺失身份。
pub fn reconcile_maximized_window_display(displays: &[WindowDisplayArea], current: &str,
    saved: &WindowStateV2, frame_insets: WindowFrameInsets) -> Option<WindowStateV2> {
    if !saved.is_valid() || !valid_main_size(saved.content_size) { return None; }
    let mut candidates = displays.iter().filter(|display| display.key == current && valid_display(display));
    let display = candidates.next()?;
    if candidates.next().is_some() { return None; }
    let placement = restore_window_placement(displays, WindowDisplaySelection {
        pointer: Some(current), current: Some(current),
    }, WindowPlacementRequest {
        role: WindowPlacementRole::Main, saved: Some(saved), legacy_content_size: None,
        legacy_maximized: true, frame_insets,
    })?;
    capture_window_state(display, placement.origin, placement.content_size, frame_insets, true)
}

/// 没有原屏身份的旧位置只有一个有效解释才迁移，禁止按多个 scale 猜中即接受。
pub fn unique_legacy_window_state(candidates: &[WindowStateV2]) -> Option<&WindowStateV2> {
    let mut candidates = candidates.iter().filter(|state| state.is_valid());
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

/// 历史主窗位置是缺少屏幕身份的屏内 DIP；只有完整落在唯一工作区时才迁移。
pub fn migrate_legacy_main_window_state(displays: &[WindowDisplayArea], origin: WindowDipPoint,
    content_size: WindowDipSize, frame_insets: WindowFrameInsets, maximized: bool) -> Option<WindowStateV2> {
    if !valid_point(origin) || !valid_main_size(content_size) || !valid_insets(frame_insets) { return None; }
    let outer = outer_size(content_size, frame_insets);
    let candidates = displays.iter().filter(|display| valid_display(display)).filter(|display| {
        let area = display.work_area;
        origin.x >= area.origin.x && origin.y >= area.origin.y
            && origin.x + outer.width <= area.origin.x + area.size.width
            && origin.y + outer.height <= area.origin.y + area.size.height
    }).filter_map(|display| capture_window_state(display, origin, content_size, frame_insets, maximized)).collect::<Vec<_>>();
    unique_legacy_window_state(&candidates).cloned()
}

/// 显示器身份及工作区必须适于有界持久化和几何计算。
fn valid_display(display: &WindowDisplayArea) -> bool {
    valid_key(&display.key) && valid_rect(display.work_area) && valid_scale(display.scale)
}

/// 稳定身份允许平台原生设备路径，但拒绝空值、控制字符和无界字符串。
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 1024 && !key.chars().any(char::is_control)
}

/// 几何值域独立于桌面全局原点，允许多屏工作区的负坐标。
fn valid_point(point: WindowDipPoint) -> bool {
    [point.x, point.y].into_iter().all(|value| value.is_finite() && value.abs() <= 1_048_576.)
}

/// 有限正尺寸防止坏文件传播到原生分配或布局。
fn valid_size(size: WindowDipSize) -> bool {
    [size.width, size.height].into_iter().all(|value| value.is_finite() && value > 0. && value <= 16_384.)
}

/// 主窗普通尺寸遵守已批准的最小内容尺寸，不因小屏静默缩小。
fn valid_main_size(size: WindowDipSize) -> bool {
    valid_size(size) && size.width >= 960. && size.height >= 640.
}

/// 工作区的起点与尺寸分别校验，原点不被 scale 重解释。
fn valid_rect(rect: WindowDipRect) -> bool {
    valid_point(rect.origin) && valid_size(rect.size)
}

/// 相对锚点表示窗口可移动距离中的比例，不保存无限或离屏比例。
fn valid_anchor(point: WindowDipPoint) -> bool {
    [point.x, point.y].into_iter().all(|value| value.is_finite() && (0. ..=1.).contains(&value))
}

/// 保存缩放只接受真实桌面设备可解释的范围。
fn valid_scale(scale: f64) -> bool {
    scale.is_finite() && (0.25..=8.).contains(&scale)
}

/// 边距由 host 读回或目标屏样式计算，坏值不能伪装成零边框。
fn valid_insets(insets: WindowFrameInsets) -> bool {
    [insets.left, insets.top, insets.right, insets.bottom].into_iter()
        .all(|value| value.is_finite() && (0. ..=256.).contains(&value))
}

/// 只有局部内容尺寸加边框参与换算；全局屏幕原点从不进入这里。
fn outer_size(content: WindowDipSize, insets: WindowFrameInsets) -> WindowDipSize {
    WindowDipSize {
        width: content.width + insets.left + insets.right,
        height: content.height + insets.top + insets.bottom,
    }
}

#[cfg(test)]
mod tests;
