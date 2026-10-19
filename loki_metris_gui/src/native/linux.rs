//! Linux 桌面身份保持 app-id、WM_CLASS、桌面入口与窗口像素一致。

use gpui_kit::{GraphicalEnvironment, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::OnceLock;
use x11rb::{connection::Connection as _, protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, MapState, PropMode}, rust_connection::RustConnection, wrapper::ConnectionExt as _};

/// 启动前解码并缓存有限尺寸；窗口线程只提交已准备的 ARGB 像素。
static ICON: OnceLock<Vec<u32>> = OnceLock::new();

/// 只缓存已发送的任务栏意图；托盘变化、显窗或请求失败后需要重新提交，不代表 WM 已确认。
#[derive(Clone, Copy, Default)]
pub(crate) struct TaskbarRequest(Option<bool>);

impl TaskbarRequest {
    /// 恢复入口可靠时跳过任务栏；请求失败保留重试，成功后同一意图不逐帧访问 X11。
    pub(crate) fn sync(&mut self, restore_available: bool, request: impl FnOnce(bool) -> anyhow::Result<()>) -> anyhow::Result<()> {
        if self.0 == Some(restore_available) { return Ok(()); }
        // 请求可能部分成功；失败后连旧意图也不再可信，托盘重新可用仍须重发。
        self.0 = None;
        request(restore_available)?;
        self.0 = Some(restore_available);
        Ok(())
    }

    /// WM 在窗口撤回时可删除状态；下次显示后的轮询必须重发同一个意图。
    pub(crate) fn invalidate(&mut self) { self.0 = None; }
}

/// 从已映射主窗的真实句柄申请跳过任务栏；只发 EWMH 消息，不改写 WM 拥有的属性。
pub(crate) fn set_skip_taskbar(window: &Window, skip: bool) -> anyhow::Result<()> {
    X11Window::capture(window)?.request_skip_taskbar(skip, false)
}

/// 显窗复用同一连接：map 前准备初始状态，MapRequest 后无条件补发，不依赖 WM 是否删除或重建 _NET_WM_STATE。
pub(crate) fn show_skipping_taskbar(window: &Window, skip: bool, map: impl FnOnce(&Window) -> anyhow::Result<()>) -> bool {
    let native = X11Window::capture(window);
    let request = |prepare_initial_state| {
        let failed = match &native {
            Ok(native) => native.request_skip_taskbar(skip, prepare_initial_state).is_err(),
            Err(_) => true,
        };
        if failed { tracing::warn!(event="linux_taskbar_request_failed"); }
    };
    request(true);
    let mapped = map(window).is_ok();
    request(false);
    mapped
}

/// 主窗与桌宠共用的 X11 活窗；每次操作重新核对活窗，连接只拥有本次协议请求，不跨窗口缓存 XID。
pub(crate) struct X11Window {
    /// 本次操作独占的 X11 连接。
    pub(crate) connection: RustConnection,
    /// 目标窗口 XID，已经 GetGeometry 验证存活。
    pub(crate) window: u32,
    /// 目标窗口所在屏幕的 root 窗口。
    pub(crate) root: u32,
}

impl X11Window {
    /// X11 和 XWayland 共用原生协议；Wayland toplevel 不具备全局定位和 EWMH 合同。
    pub(crate) fn capture(window: &Window) -> anyhow::Result<Self> {
        Self::connect(x11_window_id(window)?)
    }
    /// GetGeometry 验证 XID 仍属于当前服务器，并从窗口取得真实 root。
    pub(crate) fn connect(window: u32) -> anyhow::Result<Self> {
        let (connection, _) = x11rb::connect(None)?;
        let root = connection.get_geometry(window)?.reply()?.root;
        Ok(Self { connection, window, root })
    }
    /// 原子名由服务器分配，不使用跨服务器静态缓存。
    pub(crate) fn atom(&self, name: &[u8]) -> anyhow::Result<u32> {
        Ok(self.connection.intern_atom(false, name)?.reply()?.atom)
    }
    /// 有界读取 32 位属性；类型不匹配或字段缺失由调用方确定回退。
    pub(crate) fn property(&self, window: u32, name: &[u8], kind: AtomEnum, limit: u32) -> anyhow::Result<Vec<u32>> {
        Ok(self.connection.get_property(false, window, self.atom(name)?, kind, 0, limit)?
            .reply()?.value32().map(Iterator::collect).unwrap_or_default())
    }
    /// 已映射窗口始终交由 WM 解释 EWMH；回执只证明请求被服务器接受，确认交给调用方读回。
    pub(crate) fn request_state(&self, atom: u32, enabled: bool) -> anyhow::Result<()> {
        let event = ClientMessageEvent::new(32, self.window, self.atom(b"_NET_WM_STATE")?, [u32::from(enabled), atom, 0, 1, 0]);
        self.connection.send_event(false, self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?.check()?;
        Ok(())
    }
    /// 只有撤回窗口允许客户端直接写入 _NET_WM_STATE，否则交由 WM 解释请求。
    pub(crate) fn state(&self, atom: u32, enabled: bool) -> anyhow::Result<()> {
        let states = self.property(self.window, b"_NET_WM_STATE", AtomEnum::ATOM, 256)?;
        if states.contains(&atom) == enabled { return Ok(()); }
        if self.withdrawn()? { self.write_initial_state(atom, enabled)?; } else { self.request_state(atom, enabled)?; }
        self.connection.flush()?;
        Ok(())
    }
    /// 撤回窗口合并初始状态后仍补发请求，覆盖另一连接的 MapWindow 已在途的情形。
    fn request_skip_taskbar(&self, skip: bool, prepare_initial_state: bool) -> anyhow::Result<()> {
        let atom = self.atom(b"_NET_WM_STATE_SKIP_TASKBAR")?;
        if prepare_initial_state && self.withdrawn()? { self.write_initial_state(atom, skip)?; }
        self.request_state(atom, skip)?;
        self.connection.flush()?;
        Ok(())
    }
    /// 未映射且 WM 未登记 WM_STATE 才是撤回窗口；最小化窗口仍归 WM 管理。
    /// 协议依据：https://specifications.freedesktop.org/wm/latest/ar01s05.html 的 5.7 节。
    fn withdrawn(&self) -> anyhow::Result<bool> {
        if self.connection.get_window_attributes(self.window)?.reply()?.map_state != MapState::UNMAPPED { return Ok(false); }
        let wm_state = self.atom(b"WM_STATE")?;
        let managed = self.connection.get_property(false, self.window, wm_state, wm_state, 0, 2)?.reply()?
            .value32().and_then(|mut values| values.next()).is_some_and(|value| value != 0);
        Ok(!managed)
    }
    /// 只替换目标 atom，保留其它初始状态；损坏属性失败关闭而不是覆盖。
    fn write_initial_state(&self, atom: u32, enabled: bool) -> anyhow::Result<()> {
        let state = self.atom(b"_NET_WM_STATE")?;
        let property = self.connection.get_property(false, self.window, state, AtomEnum::ATOM, 0, 256)?.reply()?;
        anyhow::ensure!(property.bytes_after == 0 && (property.type_ == u32::from(AtomEnum::NONE)
            || (property.type_ == u32::from(AtomEnum::ATOM) && property.format == 32)), "invalid window state property");
        let mut states: Vec<_> = property.value32().map(Iterator::collect).unwrap_or_default();
        states.retain(|value| *value != atom);
        if enabled { states.push(atom); }
        self.connection.change_property32(PropMode::REPLACE, self.window, state, AtomEnum::ATOM, &states)?.check()?;
        Ok(())
    }
}

/// 本会话有 X11/XWayland 时显式选择；不改写进程环境或伪造纯 Wayland 能力。
pub(crate) fn desktop_environment(mut environment: GraphicalEnvironment) -> GraphicalEnvironment {
    if environment.x11_display.as_ref().is_some_and(|name| !name.is_empty()) {
        environment.wayland_display = None;
    }
    environment
}

/// 已批准 Logo 的 64/128 像素版本供任务栏与 Dock 直接读取，无需用户安装开发入口。
pub(crate) fn prepare_icon() {
    ICON.get_or_init(|| {
        let source = image::load_from_memory(include_bytes!("../../assets/logo.png"))
            .expect("embedded application logo must decode");
        let mut result = Vec::with_capacity(4 + 64 * 64 + 128 * 128);
        for dimension in [64, 128] {
            result.extend([dimension, dimension]);
            result.extend(source.resize_exact(dimension, dimension, image::imageops::FilterType::Lanczos3)
                .to_rgba8().pixels().map(|pixel| {
                    let [red, green, blue, alpha] = pixel.0;
                    u32::from_be_bytes([alpha, red, green, blue])
                }));
        }
        result
    });
}

/// EWMH 图标属于当前活窗；ARGB 顺序、尺寸头及 app-id 对应最终包的桌面身份。
pub(crate) fn install_icon(window: &Window) -> anyhow::Result<()> {
    let native = X11Window::capture(window)?;
    write_icon(&native.connection, native.window)
}

/// 对具体活 XID 写入有限像素，协议错误在调用栈返回，不把资源存在当作已提供窗口图标。
pub(crate) fn write_icon(connection: &impl x11rb::connection::Connection, handle: u32) -> anyhow::Result<()> {
    let atom = connection.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
    let icon = ICON.get().ok_or_else(|| anyhow::anyhow!("application icon not prepared"))?;
    connection.change_property32(PropMode::REPLACE, handle, atom, AtomEnum::CARDINAL, icon)?.check()?;
    connection.flush()?;
    Ok(())
}

/// 原生显隐、图标与桌宠宿主只对 X11/XWayland 可用；普通 Wayland 窗口保留 GPUI 激活和正常关闭路径。
pub(crate) fn x11_window_id(window: &Window) -> anyhow::Result<u32> {
    Ok(match HasWindowHandle::window_handle(window)?.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)?,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => anyhow::bail!("window is not an X11 or XWayland surface"),
    })
}

#[cfg(test)]
#[path = "linux_taskbar_tests.rs"]
mod taskbar_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::connection::Connection as _;

    /// Wayland 会话的 XWayland 保留完整桌宠宿主能力，纯 Wayland 不伪造 DISPLAY。
    #[test]
    fn desktop_selection_preserves_session_and_uses_available_xwayland() {
        let original = GraphicalEnvironment {
            wayland_display: Some("wayland-0".into()), x11_display: Some(":7".into()),
            xdg_runtime_dir: Some("/session".into()), activation_token: Some("token".into()),
        };
        let selected = desktop_environment(original.clone());
        assert!(selected.wayland_display.is_none());
        assert_eq!(selected.x11_display, original.x11_display);
        assert_eq!(selected.xdg_runtime_dir, original.xdg_runtime_dir);
        assert_eq!(selected.activation_token, original.activation_token);
        let pure = desktop_environment(GraphicalEnvironment { x11_display: None, ..original });
        assert_eq!(pure.wayland_display.as_deref(), Some(std::ffi::OsStr::new("wayland-0")));
        assert!(pure.x11_display.is_none());
    }

    /// 实际 Logo 可解码，像素与 EWMH ARGB 合同相同，有限尺寸中必须存在非透明内容。
    #[test]
    fn dock_icon_contains_real_logo_at_bounded_sizes() {
        prepare_icon();
        let icon = ICON.get().unwrap();
        assert_eq!(&icon[..2], &[64, 64]);
        assert_eq!(&icon[2 + 64 * 64..4 + 64 * 64], &[128, 128]);
        assert_eq!(icon.len(), 4 + 64 * 64 + 128 * 128);
        assert!(icon[2..2 + 64 * 64].iter().any(|pixel| pixel >> 24 != 0 && pixel & 0xffffff != 0));
    }

    /// 自有真实 X11 窗口读回 EWMH CARDINAL 图标，证明已选 Logo 的像素实际提交到服务器。
    #[test]
    #[ignore = "requires a live X11 server; run explicitly for native Linux verification"]
    fn native_x11_dock_icon_is_published_as_argb_cardinals() {
        use x11rb::protocol::xproto::{CreateWindowAux, WindowClass};
        prepare_icon();
        let (connection, screen) = x11rb::connect(None).expect("native X11 icon test requires DISPLAY");
        let window = connection.generate_id().unwrap();
        connection.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, connection.setup().roots[screen].root,
            0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &CreateWindowAux::new())
            .unwrap().check().unwrap();
        let result = (|| -> anyhow::Result<()> {
            write_icon(&connection, window)?;
            let atom = connection.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
            let property = connection.get_property(false, window, atom, AtomEnum::CARDINAL, 0, 32_768)?.reply()?;
            assert_eq!(property.format, 32); assert_eq!(property.bytes_after, 0);
            let actual: Vec<_> = property.value32().unwrap().collect();
            assert_eq!(&actual, ICON.get().unwrap());
            Ok(())
        })();
        let _ = connection.destroy_window(window).unwrap().check();
        result.unwrap();
    }
}
