//! 桌宠浮窗位置策略：合法坐标保留，缺失或不再与工作区相交则回退默认。

use serde::{Deserialize, Serialize};

/// 桌宠浮窗左上角物理像素位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayPosition {
    /// 相对虚拟桌面的水平物理像素。
    pub x: i32,
    /// 相对虚拟桌面的垂直物理像素。
    pub y: i32,
}

/// 一块显示器工作区，用于判断浮窗是否仍可见。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PetOverlayWorkArea {
    /// 工作区左上角水平物理像素。
    pub x: i32,
    /// 工作区左上角垂直物理像素。
    pub y: i32,
    /// 工作区宽度。
    pub width: u32,
    /// 工作区高度。
    pub height: u32,
}

/// 规范化已保存浮窗位置。与任一工作区相交则保留；缺失、损坏或越界返回 `None`，由调用方回退默认几何。
pub fn resolve_pet_overlay_position(
    stored: Option<PetOverlayPosition>,
    overlay_size: (u32, u32),
    work_areas: &[PetOverlayWorkArea],
) -> Option<PetOverlayPosition> {
    let position = stored?;
    let (width, height) = overlay_size;
    if width == 0 || height == 0 || work_areas.is_empty() {
        return None;
    }
    work_areas
        .iter()
        .any(|area| {
            rectangles_intersect(
                (position.x, position.y, width, height),
                (area.x, area.y, area.width, area.height),
            )
        })
        .then_some(position)
}

/// 使用半开矩形边界判断浮窗与工作区是否具有正面积交集。
fn rectangles_intersect(window: (i32, i32, u32, u32), work_area: (i32, i32, u32, u32)) -> bool {
    let (x, y, width, height) = window;
    let (area_x, area_y, area_width, area_height) = work_area;
    let x = i64::from(x);
    let y = i64::from(y);
    let area_x = i64::from(area_x);
    let area_y = i64::from(area_y);
    x < area_x + i64::from(area_width)
        && x + i64::from(width) > area_x
        && y < area_y + i64::from(area_height)
        && y + i64::from(height) > area_y
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造覆盖常见全高清桌面的测试工作区。
    fn desktop() -> PetOverlayWorkArea {
        PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    /// 验证与任一工作区相交的已保存位置会被原样保留。
    #[test]
    fn valid_position_intersecting_work_area_is_kept() {
        let stored = PetOverlayPosition { x: 240, y: 90 };
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &[desktop()]),
            Some(stored)
        );
    }

    /// 验证缺少已保存位置时交由调用方回退默认几何。
    #[test]
    fn missing_position_falls_back_to_default() {
        assert_eq!(
            resolve_pet_overlay_position(None, (360, 360), &[desktop()]),
            None
        );
    }

    /// 验证完全位于工作区外的位置会回退默认几何。
    #[test]
    fn offscreen_position_falls_back_to_default() {
        let stored = PetOverlayPosition {
            x: 50_000,
            y: 50_000,
        };
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &[desktop()]),
            None
        );
    }

    /// 验证空工作区或零尺寸浮窗均不能保留旧位置。
    #[test]
    fn empty_work_areas_or_zero_size_fall_back() {
        let stored = PetOverlayPosition { x: 20, y: 20 };
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &[]),
            None
        );
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (0, 360), &[desktop()]),
            None
        );
    }

    /// 验证部分落在工作区内的浮窗仍视为可见并保留位置。
    #[test]
    fn position_partially_on_work_area_is_kept() {
        let stored = PetOverlayPosition { x: 1800, y: 900 };
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &[desktop()]),
            Some(stored)
        );
    }

    /// 验证高分屏场景使用物理外框尺寸判断半离屏窗口是否可见。
    #[test]
    fn retina_half_offscreen_uses_physical_outer_size() {
        let stored = PetOverlayPosition { x: -400, y: 100 };
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (720, 720), &[desktop()]),
            Some(stored)
        );
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &[desktop()]),
            None
        );
    }
}
