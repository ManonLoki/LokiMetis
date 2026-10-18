//! Linux 桌面身份保持 app-id、WM_CLASS、桌面入口与窗口像素一致。

use gpui_kit::{GraphicalEnvironment, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::OnceLock;
use x11rb::{protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode}, wrapper::ConnectionExt as _};

/// 启动前解码并缓存有限尺寸；窗口线程只提交已准备的 ARGB 像素。
static ICON: OnceLock<Vec<u32>> = OnceLock::new();

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
    let handle = x11_window_id(window)?;
    let (connection, _) = x11rb::connect(None)?;
    write_icon(&connection, handle)
}

/// 对具体活 XID 写入有限像素，协议错误在调用栈返回，不把资源存在当作已提供窗口图标。
fn write_icon(connection: &impl x11rb::connection::Connection, handle: u32) -> anyhow::Result<()> {
    let atom = connection.intern_atom(false, b"_NET_WM_ICON")?.reply()?.atom;
    let icon = ICON.get().ok_or_else(|| anyhow::anyhow!("application icon not prepared"))?;
    connection.change_property32(PropMode::REPLACE, handle, atom, AtomEnum::CARDINAL, icon)?.check()?;
    connection.flush()?;
    Ok(())
}

/// 原生显隐只对 X11/XWayland 可用；普通 Wayland 主窗保留 GPUI 激活和正常关闭路径。
pub(crate) fn x11_window_id(window: &Window) -> anyhow::Result<u32> {
    Ok(match HasWindowHandle::window_handle(window)?.as_raw() {
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)?,
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        _ => anyhow::bail!("X11 icon property unavailable"),
    })
}

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
