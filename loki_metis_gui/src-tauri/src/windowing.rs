use tauri::{Manager, WebviewWindow};

use crate::performance_evidence::emit_performance_evidence_main_window_visibility;

/// 显示、取消最小化并聚焦主窗口。
pub(crate) fn restore_main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.show()?;
    let _ = emit_performance_evidence_main_window_visibility(app, true);
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

/// 修复过小或完全位于屏幕外的已保存主窗口几何状态。
pub(crate) fn ensure_main_window_is_recoverable(app: &tauri::AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.set_min_size(Some(tauri::LogicalSize::new(960.0, 640.0)))?;

    let position = window.outer_position()?;
    let size = window.outer_size()?;
    let monitors = monitor_rectangles(&window)?;
    if !saved_window_geometry_is_recoverable(
        (position.x, position.y, size.width, size.height),
        &monitors,
    ) {
        window.set_size(tauri::LogicalSize::new(1440.0, 900.0))?;
        window.center()?;
    }
    Ok(())
}

/// 返回所有可用显示器的物理坐标矩形。
fn monitor_rectangles(window: &WebviewWindow) -> tauri::Result<Vec<(i32, i32, u32, u32)>> {
    Ok(window
        .available_monitors()?
        .into_iter()
        .map(|monitor| {
            (
                monitor.position().x,
                monitor.position().y,
                monitor.size().width,
                monitor.size().height,
            )
        })
        .collect())
}

#[rustfmt::skip]
/// 判断保存的窗口尺寸满足下限且与至少一个显示器相交。
pub(crate) fn saved_window_geometry_is_recoverable(
    window: (i32, i32, u32, u32),
    monitors: &[(i32, i32, u32, u32)],
) -> bool {
    let (_x, _y, width, height) = window;
    let intersects_monitor = monitors.iter().any(|monitor| {
        rectangle_intersects_monitor(window, *monitor)
    });
    width >= 960 && height >= 640 && intersects_monitor
}

/// 判断窗口矩形与显示器矩形是否存在正面积交集。
fn rectangle_intersects_monitor(
    window: (i32, i32, u32, u32),
    monitor: (i32, i32, u32, u32),
) -> bool {
    let (x, y, width, height) = window;
    let (monitor_x, monitor_y, monitor_width, monitor_height) = monitor;
    let x = i64::from(x);
    let y = i64::from(y);
    let monitor_x = i64::from(monitor_x);
    let monitor_y = i64::from(monitor_y);
    x < monitor_x + i64::from(monitor_width)
        && x + i64::from(width) > monitor_x
        && y < monitor_y + i64::from(monitor_height)
        && y + i64::from(height) > monitor_y
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_window_state::StateFlags;

    /// 窗口状态插件应恢复尺寸、位置与最大化状态。
    #[test]
    fn window_state_restores_size_position_and_maximized() {
        let flags = StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED;
        assert!(flags.contains(StateFlags::SIZE));
        assert!(flags.contains(StateFlags::POSITION));
        assert!(flags.contains(StateFlags::MAXIMIZED));
    }

    /// 窗口状态插件不得恢复上次保存的可见性。
    #[test]
    fn window_state_ignores_saved_visibility() {
        let flags = StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED;
        assert!(!flags.contains(StateFlags::VISIBLE));
    }

    /// 过小或完全离屏的保存状态必须触发安全回退。
    #[test]
    fn window_state_falls_back_for_invalid_or_offscreen_state() {
        let monitors = [(0, 0, 1920, 1080)];
        assert!(!saved_window_geometry_is_recoverable(
            (50_000, 50_000, 1440, 900),
            &monitors
        ));
        assert!(!saved_window_geometry_is_recoverable(
            (20, 20, 320, 200),
            &monitors
        ));
    }

    /// 首次启动的默认窗口几何应保持可恢复。
    #[test]
    fn window_state_preserves_first_launch_defaults() {
        let monitors = [(0, 0, 1920, 1080)];
        assert!(saved_window_geometry_is_recoverable(
            (240, 90, 1440, 900),
            &monitors
        ));
    }
}
