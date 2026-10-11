//! 顶栏只改变原生外框；持久位置和大小继续描述历史宠物画布。

use gpui_kit::{Bounds, Pixels};
use loki_metris_core::{PetLayout, PetSizeRange, pet_window_dimensions};

/// 包含顶栏与间距的固定逻辑高度，数值或质量变化不触发原生 resize。
pub(super) const HEADER_HEIGHT: f64 = 32.;

/// 布局、尺寸范围继续沿用画布语义，避免增加顶栏后改变原滑块上限。
pub(super) fn range(area: Bounds<Pixels>, layout: PetLayout) -> PetSizeRange {
    PetSizeRange::for_work_area(
        f64::from(f32::from(area.size.width.min(area.size.height))),
        layout,
    )
}

/// 只有完整原生窗口尺寸增加顶栏，图片网格仍使用 core 的原始尺寸。
pub(super) fn dimensions(layout: PetLayout, pet_size: u16) -> (f64, f64) {
    let (width, height) = pet_window_dimensions(layout, pet_size);
    (width, height + HEADER_HEIGHT)
}

/// 原生手动 resize 和视图都从完整高度中扣除同一顶栏。
pub(super) fn body_height(height: f32) -> f32 {
    (height - HEADER_HEIGHT as f32).max(0.)
}

/// 字号、圆角和分页控件继续以画布短边计算。
pub(super) fn body_vmin(window: &gpui_kit::Window) -> f32 {
    f32::from(window.viewport_size().width)
        .min(body_height(f32::from(window.viewport_size().height)))
}
