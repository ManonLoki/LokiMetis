//! 桌宠布局尺寸、显示器约束与原生缩放收敛。

use loki_metis_core::PetLayout;
use tauri::{LogicalSize, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};

/// 找不到显示器时用于推导保守上限的逻辑短边。
const FALLBACK_LOGICAL_SHORTEST_SIDE: u32 = 1_440;
/// 单格统一最小逻辑像素边长；必须与窗口创建时的 `min_inner_size(64.0, 64.0)` 安全底线保持一致。
/// 低于此值时，无边框可缩放窗口在高 DPI 下的原生缩放热区会覆盖整个窗口客户区，
/// 导致鼠标点击（含右键）被系统当作非客户区缩放操作吞掉，
/// 主线程随之进入原生模态缩放循环、整个应用卡死且无法通过托盘退出。
pub const PET_CELL_MIN: u16 = 64;

/// 由布局与单格大小计算窗口逻辑尺寸。
pub fn logical_pet_window_size(layout: PetLayout, pet_size: u16) -> LogicalSize<f64> {
    let (rows, columns) = layout.dimensions();
    let cell = f64::from(pet_size);
    LogicalSize::new(cell * columns as f64, cell * rows as f64)
}

/// 按显示器逻辑短边和布局最长轴计算单格最大尺寸。
fn maximum_pet_size(shortest_physical: u32, scale_factor: f64, layout: PetLayout) -> u16 {
    let (rows, columns) = layout.dimensions();
    let axis_count = rows.max(columns).max(1);
    (f64::from(shortest_physical) / scale_factor.max(1.0) / 4.0 / axis_count as f64)
        .floor()
        .max(f64::from(PET_CELL_MIN)) as u16
}

/// 解析桌宠当前显示器，窗口尚未映射时回退主显示器。
fn resolve_pet_monitor<R: Runtime>(window: &WebviewWindow<R>) -> Option<Monitor> {
    window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
}

/// 返回给定显示器和布局允许的单格尺寸区间。
fn pet_size_range_for_monitor(monitor: &Monitor, layout: PetLayout) -> (u16, u16) {
    let area = monitor.work_area();
    (
        PET_CELL_MIN,
        maximum_pet_size(
            area.size.width.min(area.size.height),
            monitor.scale_factor(),
            layout,
        ),
    )
}

/// 无显示器句柄时返回与运行态相同公式的保守尺寸区间。
pub fn pet_size_range_fallback(layout: PetLayout) -> (u16, u16) {
    (
        PET_CELL_MIN,
        maximum_pet_size(FALLBACK_LOGICAL_SHORTEST_SIDE, 1.0, layout),
    )
}

/// 返回桌宠当前显示器允许的单格尺寸区间。
pub fn pet_size_range<R: Runtime>(window: &WebviewWindow<R>, layout: PetLayout) -> (u16, u16) {
    resolve_pet_monitor(window).as_ref().map_or_else(
        || pet_size_range_fallback(layout),
        |monitor| pet_size_range_for_monitor(monitor, layout),
    )
}

/// 把当前布局允许的最小和最大窗口尺寸交给操作系统。
pub fn apply_pet_constraints<R: Runtime>(
    window: &WebviewWindow<R>,
    layout: PetLayout,
) -> tauri::Result<()> {
    let (min, max) = pet_size_range(window, layout);
    window.set_min_size(Some(logical_pet_window_size(layout, min)))?;
    window.set_max_size(Some(logical_pet_window_size(layout, max)))
}

/// 以当前显示器区间收敛单格大小并同步原生窗口尺寸。
pub fn apply_pet_size<R: Runtime>(
    window: &WebviewWindow<R>,
    layout: PetLayout,
    requested: u16,
) -> Result<u16, String> {
    let (min, max) = pet_size_range(window, layout);
    let size = requested.clamp(min, max);
    apply_pet_constraints(window, layout).map_err(|error| error.to_string())?;
    let expected = logical_pet_window_size(layout, size);
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let expected_physical = PhysicalSize::new(
        (expected.width * scale).round() as u32,
        (expected.height * scale).round() as u32,
    );
    let differs = window.inner_size().map_or(true, |current| {
        current.width.abs_diff(expected_physical.width) > 1
            || current.height.abs_diff(expected_physical.height) > 1
    });
    if differs {
        window
            .set_size(expected)
            .map_err(|error| error.to_string())?;
    }
    Ok(size)
}

/// 把任意原生 resize 收敛回当前布局宽高比并返回新的单格逻辑尺寸。
pub fn normalize_pet_resize<R: Runtime>(
    window: &WebviewWindow<R>,
    layout: PetLayout,
    size: PhysicalSize<u32>,
    previous_pet_size: u16,
) -> u16 {
    let scale = window.scale_factor().unwrap_or(1.0).max(1.0);
    let (rows, columns) = layout.dimensions();
    let width_cell = f64::from(size.width) / columns.max(1) as f64;
    let height_cell = f64::from(size.height) / rows.max(1) as f64;
    let previous = logical_pet_window_size(layout, previous_pet_size);
    let previous_width = (previous.width * scale).round() as u32;
    let previous_height = (previous.height * scale).round() as u32;
    let requested_cell =
        if size.width.abs_diff(previous_width) >= size.height.abs_diff(previous_height) {
            width_cell
        } else {
            height_cell
        };
    let requested = (requested_cell / scale).round() as u16;
    let (min, max) = pet_size_range(window, layout);
    let pet_size = requested.clamp(min, max);
    let expected = logical_pet_window_size(layout, pet_size);
    let expected_physical = PhysicalSize::new(
        (expected.width * scale).round() as u32,
        (expected.height * scale).round() as u32,
    );
    if size.width.abs_diff(expected_physical.width) > 1
        || size.height.abs_diff(expected_physical.height) > 1
    {
        let _ = window.set_size(expected);
    }
    pet_size
}

/// 把窗口左上角收敛到工作区内；窗口过大时退化为贴齐工作区左上角。
pub fn clamped_pet_position(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    area_position: PhysicalPosition<i32>,
    area_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let right = i64::from(area_position.x) + i64::from(area_size.width);
    let bottom = i64::from(area_position.y) + i64::from(area_size.height);
    let max_x = (right - i64::from(size.width)).max(i64::from(area_position.x));
    let max_y = (bottom - i64::from(size.height)).max(i64::from(area_position.y));
    PhysicalPosition::new(
        i64::from(position.x).clamp(i64::from(area_position.x), max_x) as i32,
        i64::from(position.y).clamp(i64::from(area_position.y), max_y) as i32,
    )
}

/// 把缩放后的桌宠完整收敛回当前显示器工作区。
pub fn clamp_pet_window_to_work_area<R: Runtime>(window: &WebviewWindow<R>) {
    let Some(monitor) = resolve_pet_monitor(window) else {
        return;
    };
    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    let area = monitor.work_area();
    let clamped = clamped_pet_position(position, size, area.position, area.size);
    if clamped != position {
        let _ = window.set_position(clamped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 六种布局应映射到约定的 AI Monitor 窗口尺寸。
    #[test]
    fn six_layouts_have_aimonitor_window_dimensions() {
        assert_eq!(
            logical_pet_window_size(PetLayout::Single, 64),
            LogicalSize::new(64.0, 64.0)
        );
        assert_eq!(
            logical_pet_window_size(PetLayout::Row, 64),
            LogicalSize::new(128.0, 64.0)
        );
        assert_eq!(
            logical_pet_window_size(PetLayout::Column, 64),
            LogicalSize::new(64.0, 128.0)
        );
        assert_eq!(
            logical_pet_window_size(PetLayout::Row3, 64),
            LogicalSize::new(192.0, 64.0)
        );
        assert_eq!(
            logical_pet_window_size(PetLayout::Column3, 64),
            LogicalSize::new(64.0, 192.0)
        );
        assert_eq!(
            logical_pet_window_size(PetLayout::Grid, 64),
            LogicalSize::new(128.0, 128.0)
        );
    }

    /// 无显示器信息时的尺寸范围应按布局最长轴缩放。
    #[test]
    fn fallback_range_uses_layout_longest_axis() {
        assert_eq!(pet_size_range_fallback(PetLayout::Single), (64, 360));
        assert_eq!(pet_size_range_fallback(PetLayout::Grid), (64, 180));
        assert_eq!(pet_size_range_fallback(PetLayout::Row3), (64, 120));
    }

    /// 单格最小尺寸必须至少等于窗口创建时的安全底线，避免无边框缩放热区吞掉整窗点击。
    #[test]
    fn cell_min_matches_window_builder_safety_floor() {
        assert!(PET_CELL_MIN >= 64);
    }

    /// 缩放后的位置应限制在带偏移的显示器工作区内。
    #[test]
    fn resize_position_is_clamped_inside_offset_work_area() {
        let area_position = PhysicalPosition::new(-1_920, 24);
        let area_size = PhysicalSize::new(1_920, 1_056);
        let size = PhysicalSize::new(256, 256);
        assert_eq!(
            clamped_pet_position(
                PhysicalPosition::new(-100, 950),
                size,
                area_position,
                area_size,
            ),
            PhysicalPosition::new(-256, 824)
        );
        assert_eq!(
            clamped_pet_position(
                PhysicalPosition::new(-2_500, -100),
                size,
                area_position,
                area_size,
            ),
            area_position
        );
    }
}
