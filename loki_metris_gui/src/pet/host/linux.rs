//! X11/XWayland 辅助窗使用实际窗口属性和 Shape 输入区域；纯 Wayland 不伪报成功。

use gpui_kit::{Bounds, Pixels, point, px, size};
#[cfg(not(test))]
use gpui_kit::Window;
use loki_metris_core::PetWindowPreferences;
use x11rb::{
    connection::Connection as _,
    properties::WmSizeHints,
    protocol::{randr::ConnectionExt as _, shape::{ConnectionExt as _, SK, SO}, xproto::{
        AtomEnum, ClientMessageEvent, ClipOrdering, ConfigureWindowAux, ConnectionExt as _,
        EventMask, MapState, PropMode,
    }},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

/// 每次操作重新核对活窗，连接只拥有本次协议请求，不跨窗口缓存 XID。
struct NativeWindow {
    connection: RustConnection,
    window: u32,
    root: u32,
}

/// 全局物理矩形只在 X11 边界使用，转换到 GPUI 时才除以当前窗口 scale。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalRect { x: i32, y: i32, width: u32, height: u32 }

impl PhysicalRect {
    /// 只保留真实相交部分；工作区和显示器不相交时不产生虚假可用区域。
    fn intersect(self, other: Self) -> Option<Self> {
        let left = i64::from(self.x).max(i64::from(other.x));
        let top = i64::from(self.y).max(i64::from(other.y));
        let right = (i64::from(self.x) + i64::from(self.width)).min(i64::from(other.x) + i64::from(other.width));
        let bottom = (i64::from(self.y) + i64::from(self.height)).min(i64::from(other.y) + i64::from(other.height));
        (right > left && bottom > top).then(|| Self { x: left as i32, y: top as i32,
            width: (right - left) as u32, height: (bottom - top) as u32 })
    }
    /// 以物理中心或保存原点选择显示器，保留负坐标和半开边界。
    fn contains(self, x: i64, y: i64) -> bool {
        x >= i64::from(self.x) && y >= i64::from(self.y)
            && x < i64::from(self.x) + i64::from(self.width)
            && y < i64::from(self.y) + i64::from(self.height)
    }
    /// 与 GPUI 共享当前窗口缩放，持久化仍保持全局物理坐标。
    fn logical(self, scale: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(self.x as f32 / scale), px(self.y as f32 / scale)),
            size(px(self.width as f32 / scale), px(self.height as f32 / scale)))
    }
}

impl NativeWindow {
    /// X11 和 XWayland 共用原生协议；Wayland toplevel 不具备全局定位和置顶合同。
    #[cfg(not(test))]
    fn capture(window: &Window) -> anyhow::Result<Self> {
        Self::connect(crate::native::linux::x11_window_id(window)?)
    }
    /// GetGeometry 验证 XID 仍属于当前服务器，并从窗口取得真实 root。
    fn connect(window: u32) -> anyhow::Result<Self> {
        let (connection, _) = x11rb::connect(None)?;
        let root = connection.get_geometry(window)?.reply()?.root;
        Ok(Self { connection, window, root })
    }
    /// 原子名由服务器分配，不使用跨服务器静态缓存。
    fn atom(&self, name: &[u8]) -> anyhow::Result<u32> {
        Ok(self.connection.intern_atom(false, name)?.reply()?.atom)
    }
    /// 有界读取 32 位属性；类型不匹配或字段缺失由调用方确定回退。
    fn property(&self, window: u32, name: &[u8], kind: AtomEnum, limit: u32) -> anyhow::Result<Vec<u32>> {
        Ok(self.connection.get_property(false, window, self.atom(name)?, kind, 0, limit)?
            .reply()?.value32().map(Iterator::collect).unwrap_or_default())
    }
    /// 客户区原点经 root 转换，避免窗口管理器重设父级后保存 frame 内局部坐标。
    fn bounds(&self) -> anyhow::Result<PhysicalRect> {
        let geometry = self.connection.get_geometry(self.window)?.reply()?;
        let origin = self.connection.translate_coordinates(self.window, self.root, 0, 0)?.reply()?;
        Ok(PhysicalRect { x: origin.dst_x.into(), y: origin.dst_y.into(),
            width: geometry.width.into(), height: geometry.height.into() })
    }
    /// EWMH 工作区可能覆盖整个多屏桌面；逐屏相交后才供尺寸约束和恢复使用。
    fn work_areas(&self) -> anyhow::Result<Vec<PhysicalRect>> {
        let geometry = self.connection.get_geometry(self.root)?.reply()?;
        let root = PhysicalRect { x: 0, y: 0, width: geometry.width.into(), height: geometry.height.into() };
        let desktop = self.property(self.root, b"_NET_CURRENT_DESKTOP", AtomEnum::CARDINAL, 1)?
            .first().copied().unwrap_or(0).min(4095);
        let areas = self.property(self.root, b"_NET_WORKAREA", AtomEnum::CARDINAL, (desktop + 1) * 4)?;
        let work = areas.get(desktop as usize * 4..desktop as usize * 4 + 4)
            .filter(|value| value[2] > 0 && value[3] > 0)
            .map(|value| PhysicalRect { x: value[0] as i32, y: value[1] as i32, width: value[2], height: value[3] });
        let mut monitors = self.connection.randr_get_monitors(self.root, true).ok()
            .and_then(|reply| reply.reply().ok()).map(|reply| reply.monitors.into_iter().map(|monitor| PhysicalRect {
                x: monitor.x.into(), y: monitor.y.into(), width: monitor.width.into(), height: monitor.height.into(),
            }).filter(|area| area.width > 0 && area.height > 0).collect::<Vec<_>>()).unwrap_or_default();
        // RandR 1.2-1.4 仍能给出活动 CRTC；较老服务器才回退真实 root 尺寸。
        if monitors.is_empty()
            && let Some(resources) = self.connection.randr_get_screen_resources_current(self.root).ok().and_then(|reply| reply.reply().ok()) {
                for crtc in resources.crtcs {
                    if let Ok(info) = self.connection.randr_get_crtc_info(crtc, resources.config_timestamp)?.reply()
                        && info.width > 0 && info.height > 0 {
                            monitors.push(PhysicalRect { x: info.x.into(), y: info.y.into(), width: info.width.into(), height: info.height.into() });
                        }
                }
            }
        if monitors.is_empty() { monitors.push(root); }
        let clipped: Vec<_> = monitors.iter().filter_map(|monitor| work.map_or(Some(*monitor), |work| monitor.intersect(work))).collect();
        Ok(if clipped.is_empty() { monitors } else { clipped })
    }
    /// 映射只作用于当前活窗，不通过应用激活顺便抢焦点；Unmap 保留窗口和播放状态。
    fn set_visible(&self, visible: bool) -> anyhow::Result<()> {
        let desired = if visible { MapState::VIEWABLE } else { MapState::UNMAPPED };
        let confirmed = self.connection.get_window_attributes(self.window)?.reply()?.map_state == desired;
        if !confirmed {
            if visible { self.connection.map_window(self.window)?.check()?; }
            else { self.connection.unmap_window(self.window)?.check()?; }
            if visible {
                self.connection.change_property32(PropMode::REPLACE, self.window,
                    self.atom(b"_LOKI_METRIS_AUXILIARY_REMAP_PENDING")?, AtomEnum::CARDINAL, &[1])?.check()?;
            }
        }
        self.connection.change_property32(PropMode::REPLACE, self.window,
            self.atom(b"_LOKI_METRIS_AUXILIARY_VISIBLE")?, AtomEnum::CARDINAL, &[u32::from(visible)])?.check()?;
        self.connection.flush()?;
        Ok(())
    }
    /// 已映射窗口始终交由 WM 解释 EWMH；回执只证明请求被服务器接受，确认交给异步 settle。
    fn request_state(&self, atom: u32, enabled: bool) -> anyhow::Result<()> {
        let event = ClientMessageEvent::new(32, self.window, self.atom(b"_NET_WM_STATE")?, [u32::from(enabled), atom, 0, 1, 0]);
        self.connection.send_event(false, self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?.check()?;
        Ok(())
    }
    /// 初次 map 前直接登记 EWMH 状态，已映射窗口用 ClientMessage 通知 WM 保持置顶。
    fn state(&self, atom: u32, enabled: bool) -> anyhow::Result<()> {
        let property = self.atom(b"_NET_WM_STATE")?;
        let mut states = self.property(self.window, b"_NET_WM_STATE", AtomEnum::ATOM, 256)?;
        if states.contains(&atom) == enabled { return Ok(()); }
        states.retain(|state| *state != atom);
        if enabled { states.push(atom); }
        let mapped = self.connection.get_window_attributes(self.window)?.reply()?.map_state != MapState::UNMAPPED;
        if !mapped {
            self.connection.change_property32(PropMode::REPLACE, self.window, property, AtomEnum::ATOM, &states)?.check()?;
        } else {
            self.request_state(atom, enabled)?;
        }
        self.connection.flush()?;
        Ok(())
    }
    /// 装饰、工具窗身份及缩放边界不改变鼠标穿透；领域偏好仍由 core 校验。
    fn apply(&self, prefs: PetWindowPreferences, settings: bool, scale: f32) -> anyhow::Result<()> {
        let motif = self.atom(b"_MOTIF_WM_HINTS")?;
        let functions = if settings { (1 << 2) | (1 << 5) } else if prefs.locked { 1 << 5 }
            else { (1 << 1) | (1 << 2) | (1 << 5) };
        self.connection.change_property32(PropMode::REPLACE, self.window, motif, motif, &[3, functions, 0, 0, 0])?.check()?;
        self.connection.change_property32(PropMode::REPLACE, self.window, self.atom(b"_NET_WM_WINDOW_TYPE")?,
            AtomEnum::ATOM, &[self.atom(b"_NET_WM_WINDOW_TYPE_UTILITY")?])?.check()?;
        self.state(self.atom(b"_NET_WM_STATE_SKIP_TASKBAR")?, true)?;
        self.state(self.atom(b"_NET_WM_STATE_SKIP_PAGER")?, true)?;
        self.state(self.atom(b"_NET_WM_STATE_ABOVE")?, prefs.always_on_top || settings)?;
        let mut hints = WmSizeHints::new();
        if settings {
            let bounds = self.bounds()?;
            hints.min_size = Some((bounds.width as i32, bounds.height as i32));
            hints.max_size = hints.min_size;
        } else {
            let bounds = self.bounds()?;
            let area = select_area(&self.work_areas()?, bounds).logical(scale);
            let range = super::super::geometry::range(area, prefs.layout);
            let dimensions = |size| {
                let (width, height) = super::super::geometry::dimensions(prefs.layout, size);
                ((width as f32 * scale).round() as i32, (height as f32 * scale).round() as i32)
            };
            hints.min_size = Some(dimensions(if prefs.locked { range.constrain(prefs.pet_size) } else { range.min }));
            hints.max_size = Some(dimensions(if prefs.locked { range.constrain(prefs.pet_size) } else { range.max }));
        }
        hints.set_normal_hints(&self.connection, self.window)?.check()?;
        self.connection.change_property32(PropMode::REPLACE, self.window,
            self.atom(b"_LOKI_METRIS_AUXILIARY_TOPMOST")?, AtomEnum::CARDINAL,
            &[u32::from(prefs.always_on_top || settings)])?.check()?;
        self.connection.flush()?;
        Ok(())
    }
    /// 位置和客户区尺寸在单个 ConfigureWindow 请求提交，不改激活状态和既有 Z 序。
    fn set_geometry(&self, bounds: PhysicalRect) -> anyhow::Result<()> {
        anyhow::ensure!(bounds.width > 0 && bounds.height > 0 && bounds.width <= u16::MAX.into()
            && bounds.height <= u16::MAX.into(), "invalid auxiliary geometry");
        self.connection.change_property32(PropMode::REPLACE, self.window,
            self.atom(b"_LOKI_METRIS_AUXILIARY_GEOMETRY")?, AtomEnum::CARDINAL,
            &[bounds.x as u32, bounds.y as u32, bounds.width, bounds.height])?.check()?;
        self.connection.configure_window(self.window, &ConfigureWindowAux::new().x(bounds.x).y(bounds.y)
            .width(bounds.width).height(bounds.height))?.check()?;
        self.connection.flush()?;
        Ok(())
    }
    /// 每次只读一轮实际状态；等待由 GPUI timer 驱动，不在 UI 线程 sleep 或等待 WM。
    fn settled(&self) -> anyhow::Result<bool> {
        let mapped = self.connection.get_window_attributes(self.window)?.reply()?.map_state;
        if let Some(visible) = self.property(self.window, b"_LOKI_METRIS_AUXILIARY_VISIBLE", AtomEnum::CARDINAL, 1)?.first().copied() {
            if mapped != if visible != 0 { MapState::VIEWABLE } else { MapState::UNMAPPED } { return Ok(false); }
            self.connection.delete_property(self.window, self.atom(b"_LOKI_METRIS_AUXILIARY_VISIBLE")?)?.check()?;
        }
        if let Some(topmost) = self.property(self.window, b"_LOKI_METRIS_AUXILIARY_TOPMOST", AtomEnum::CARDINAL, 1)?.first().copied() {
            let targets = [(self.atom(b"_NET_WM_STATE_SKIP_TASKBAR")?, true),
                (self.atom(b"_NET_WM_STATE_SKIP_PAGER")?, true), (self.atom(b"_NET_WM_STATE_ABOVE")?, topmost != 0)];
            if mapped == MapState::VIEWABLE
                && self.property(self.window, b"_LOKI_METRIS_AUXILIARY_REMAP_PENDING", AtomEnum::CARDINAL, 1)?.first() == Some(&1) {
                    // WM 可在 map 时改写初始状态；新命令排在 MapRequest 之后，下回合再确认。
                    for (atom, enabled) in targets { self.request_state(atom, enabled)?; }
                    self.connection.change_property32(PropMode::REPLACE, self.window,
                        self.atom(b"_LOKI_METRIS_AUXILIARY_REMAP_PENDING")?, AtomEnum::CARDINAL, &[0])?.check()?;
                    let target = self.property(self.window, b"_LOKI_METRIS_AUXILIARY_GEOMETRY", AtomEnum::CARDINAL, 4)?;
                    if target.len() == 4 {
                        // WM 首次管理可能执行自动放置，map 后再提交本次明确的内容区目标。
                        self.set_geometry(PhysicalRect { x: target[0] as i32, y: target[1] as i32,
                            width: target[2], height: target[3] })?;
                    }
                    self.connection.flush()?;
                    return Ok(false);
                }
            let states = self.property(self.window, b"_NET_WM_STATE", AtomEnum::ATOM, 256)?;
            if targets.iter().any(|(atom, enabled)| states.contains(atom) != *enabled) {
                for (atom, enabled) in targets { self.state(atom, enabled)?; }
                return Ok(false);
            }
        }
        let target = self.property(self.window, b"_LOKI_METRIS_AUXILIARY_GEOMETRY", AtomEnum::CARDINAL, 4)?;
        if target.len() == 4 {
            if self.bounds()? != (PhysicalRect { x: target[0] as i32, y: target[1] as i32,
                width: target[2], height: target[3] }) { return Ok(false); }
            // 目标仅是本次命令的确认屏障；成功后用户拖动不再受旧矩形约束。
            self.connection.delete_property(self.window, self.atom(b"_LOKI_METRIS_AUXILIARY_GEOMETRY")?)?.check()?;
        }
        Ok(true)
    }
    /// Shape Input 与视觉 bounding/clip 分离；关闭用 None 恢复随 resize 自动扩展的默认输入区域。
    fn set_mouse_passthrough(&self, enabled: bool, state: &mut super::MousePassthroughState) -> anyhow::Result<()> {
        let version = self.connection.shape_query_version()?.reply()?;
        anyhow::ensure!(version.major_version > 1 || (version.major_version == 1 && version.minor_version >= 1),
            "X Shape input region requires version 1.1");
        let before = self.connection.shape_get_rectangles(self.window, SK::INPUT)?.reply()?;
        let applied = (|| -> anyhow::Result<bool> {
            if enabled {
                self.connection.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, self.window, 0, 0, &[])?.check()?;
            } else {
                self.connection.shape_mask(SO::SET, SK::INPUT, self.window, 0, 0, x11rb::NONE)?.check()?;
            }
            let actual = self.connection.shape_get_rectangles(self.window, SK::INPUT)?.reply()?.rectangles.is_empty();
            anyhow::ensure!(actual == enabled, "X Shape input readback mismatch");
            Ok(actual)
        })();
        match applied {
            Ok(actual) => { state.confirmed = Some(actual); Ok(()) }
            Err(error) => {
                let rollback = self.connection.shape_rectangles(SO::SET, SK::INPUT, before.ordering,
                    self.window, 0, 0, &before.rectangles).map_err(anyhow::Error::from)
                    .and_then(|cookie| cookie.check().map_err(anyhow::Error::from));
                state.confirmed = if rollback.is_ok() {
                    self.connection.shape_get_rectangles(self.window, SK::INPUT).ok().and_then(|reply| reply.reply().ok())
                        .map(|reply| reply.rectangles.is_empty())
                } else { None };
                anyhow::bail!("X Shape input apply failed: {error}; rollback={rollback:?}")
            }
        }
    }
}

/// 优先窗口中心所在工作区，跨屏窗口再按最大交集选择；无交集使用第一块真实屏幕。
fn select_area(areas: &[PhysicalRect], bounds: PhysicalRect) -> PhysicalRect {
    let center = (i64::from(bounds.x) + i64::from(bounds.width) / 2, i64::from(bounds.y) + i64::from(bounds.height) / 2);
    areas.iter().copied().find(|area| area.contains(center.0, center.1)).or_else(|| {
        areas.iter().copied().filter(|area| area.intersect(bounds).is_some())
            .max_by_key(|area| area.intersect(bounds).map_or(0, |rect| u64::from(rect.width) * u64::from(rect.height)))
    }).or_else(|| areas.first().copied()).expect("X11 work areas contain at least the root window")
}

/// 原生输入开关成功并读回后才允许 controller 保存偏好。
#[cfg(not(test))]
pub fn set_mouse_passthrough(window: &Window, enabled: bool, state: &mut super::MousePassthroughState) -> anyhow::Result<()> {
    NativeWindow::capture(window)?.set_mouse_passthrough(enabled, state)
}
/// 托盘显示和隐藏命令实际 map/unmap 当前活窗，不把 activate 当作显示。
#[cfg(not(test))]
pub fn set_visible(window: &Window, visible: bool) -> anyhow::Result<()> {
    NativeWindow::capture(window)?.set_visible(visible)
}
/// 单次原生读回供 controller 的异步确认屏障使用，不把请求回执当作应用完成。
#[cfg(not(test))]
pub fn settled(window: &Window) -> anyhow::Result<bool> { NativeWindow::capture(window)?.settled() }
/// 仅在已验证 X11 活窗上应用置顶、工具窗、装饰和缩放边界。
#[cfg(not(test))]
pub fn apply(window: &Window, prefs: PetWindowPreferences, settings: bool) -> anyhow::Result<()> {
    let native = NativeWindow::capture(window)?;
    let result = native.apply(prefs, settings, window.scale_factor());
    // 复用本次已验证活窗的连接补写 Dock 图标，不再为同一 XID 另开连接。
    if result.is_ok() { let _ = crate::native::linux::write_icon(&native.connection, native.window); }
    result
}
/// 设置窗居中只修改 root 全局位置，不激活其它应用窗口。
#[cfg(not(test))]
pub fn move_to(window: &Window, x: f64, y: f64) -> anyhow::Result<()> {
    let native = NativeWindow::capture(window)?;
    let mut bounds = native.bounds()?;
    bounds.x = (x * f64::from(window.scale_factor())).round() as i32;
    bounds.y = (y * f64::from(window.scale_factor())).round() as i32;
    native.set_geometry(bounds)
}
/// RandR 和 EWMH 共同确定实际可用区；服务器失败时只使用 GPUI 当前真实 bounds。
#[cfg(not(test))]
pub fn work_area(window: &Window) -> Bounds<Pixels> {
    NativeWindow::capture(window).and_then(|native| Ok(select_area(&native.work_areas()?, native.bounds()?).logical(window.scale_factor())))
        .unwrap_or_else(|_| window.bounds())
}
/// 读回实际全局客户区，避免 ConfigureNotify 尚在事件队列时保存旧 GPUI bounds。
#[cfg(not(test))]
pub fn bounds(window: &Window) -> Bounds<Pixels> {
    NativeWindow::capture(window).and_then(|native| native.bounds()).map(|bounds| bounds.logical(window.scale_factor()))
        .unwrap_or_else(|_| window.bounds())
}
/// 以当前真实 scale 一次量化物理坐标；原生 setter 失败继续触发 controller 回滚。
#[cfg(not(test))]
pub fn set_geometry(window: &mut Window, bounds: Bounds<Pixels>) -> anyhow::Result<()> {
    let scale = window.scale_factor();
    NativeWindow::capture(window)?.set_geometry(PhysicalRect {
        x: (f32::from(bounds.origin.x) * scale).round() as i32,
        y: (f32::from(bounds.origin.y) * scale).round() as i32,
        width: (f32::from(bounds.size.width) * scale).round() as u32,
        height: (f32::from(bounds.size.height) * scale).round() as u32,
    })
}
/// 保存位置只在当前真实屏幕中恢复；拔屏或越界由 core 返回安全默认位置。
#[cfg(not(test))]
pub fn restoration_area(window: &Window, stored: Option<loki_metris_core::PetOverlayPosition>, prefs: PetWindowPreferences)
    -> (Bounds<Pixels>, f64, bool) {
    let scale = window.scale_factor();
    let fallback = (work_area(window), f64::from(scale), false);
    let Some(stored) = stored else { return fallback; };
    let Ok(areas) = NativeWindow::capture(window).and_then(|native| native.work_areas()) else { return fallback; };
    let mut valid = None;
    for area in areas {
        let logical = area.logical(scale);
        let range = super::super::geometry::range(logical, prefs.layout);
        let (width, height) = loki_metris_core::pet_window_dimensions(prefs.layout, range.constrain(prefs.pet_size));
        let physical = loki_metris_core::PetOverlayWorkArea { x: area.x, y: area.y, width: area.width, height: area.height };
        if loki_metris_core::resolve_pet_overlay_position(Some(stored),
            ((width * f64::from(scale)).round() as u32, (height * f64::from(scale)).round() as u32), &[physical]).is_some() {
            valid = Some((logical, f64::from(scale), true));
            if area.contains(stored.x.into(), stored.y.into()) { break; }
        }
    }
    valid.unwrap_or(fallback)
}
/// X11 后端已实现 EWMH 交互移动，交给原始按下事件启动系统拖动。
#[cfg(not(test))]
pub fn start_drag(window: &Window, _: &gpui_kit::App) { window.start_window_move(); }

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
