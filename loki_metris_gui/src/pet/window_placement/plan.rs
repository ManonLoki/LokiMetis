//! 原生输入先转换屏内 DIP，再调用 core 决定选屏、锚点和旧位置唯一性。

use crate::native::windowing::DesktopSnapshot;
use loki_metris_core::{PetOverlayPosition, PetSizeRange, PetWindowPreferences, window_placement::*};

/// 选屏后才约束画布尺寸，完整外框包含统一的 32 DIP 顶栏。
pub(super) fn placement(desktop: &DesktopSnapshot, prefs: PetWindowPreferences, saved: Option<&WindowStateV2>,
    current: Option<&str>, insets: WindowFrameInsets) -> Option<(WindowPlacement, PetSizeRange)> {
    let areas = desktop.areas();
    let selection = WindowDisplaySelection { pointer: desktop.pointer.as_deref(), current };
    let target = choose_window_display(&areas, selection, saved.map(|state| state.display_key.as_str()))?;
    let range = PetSizeRange::for_work_area(target.work_area.size.width.min(target.work_area.size.height), prefs.layout);
    let (width, height) = super::super::geometry::dimensions(prefs.layout, range.constrain(prefs.pet_size));
    let plan = restore_window_placement(&areas, selection, WindowPlacementRequest {
        role: WindowPlacementRole::Pet(WindowDipSize { width, height }), saved,
        legacy_content_size: None, legacy_maximized: false, frame_insets: insets,
    })?;
    Some((plan, range))
}

/// 旧位置缺少屏幕身份，仅接受完整落在唯一屏工作区的物理像素解释。
pub(super) fn legacy_state(desktop: &DesktopSnapshot, prefs: PetWindowPreferences, stored: Option<PetOverlayPosition>,
    insets: WindowFrameInsets) -> Option<WindowStateV2> {
    let stored = stored?;
    let candidates = desktop.displays.iter().filter_map(|display| {
        let area = &display.area;
        let range = PetSizeRange::for_work_area(area.work_area.size.width.min(area.work_area.size.height), prefs.layout);
        let (width, height) = super::super::geometry::dimensions(prefs.layout, range.constrain(prefs.pet_size));
        let origin = WindowDipPoint {
            x: f64::from(stored.x) / area.scale - display.global_origin.x,
            y: f64::from(stored.y) / area.scale - super::super::geometry::HEADER_HEIGHT - display.global_origin.y,
        };
        let work = area.work_area;
        (origin.x >= work.origin.x && origin.y >= work.origin.y
            && origin.x + width + insets.left + insets.right <= work.origin.x + work.size.width
            && origin.y + height + insets.top + insets.bottom <= work.origin.y + work.size.height)
            .then(|| capture_window_state(area, origin, WindowDipSize { width, height }, insets, false)).flatten()
    }).collect::<Vec<_>>();
    unique_legacy_window_state(&candidates).cloned()
}

/// 对齐后的目标与实际屏内 DIP 在半个物理像素内一致，才结束迁屏事务。
pub(super) fn matches(actual: &WindowStateV2, plan: &WindowPlacement) -> bool {
    let tolerance = 0.5 / actual.scale.max(1.);
    actual.display_key == plan.display_key
        && (actual.origin.x - plan.origin.x).abs() <= tolerance
        && (actual.origin.y - plan.origin.y).abs() <= tolerance
        && (actual.content_size.width - plan.content_size.width).abs() <= tolerance
        && (actual.content_size.height - plan.content_size.height).abs() <= tolerance
}
