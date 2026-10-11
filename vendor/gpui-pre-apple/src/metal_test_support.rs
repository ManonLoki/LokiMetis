//! 父圆角裁剪 Metal 实物测试共用的离屏渲染入口，各测试仅提供逻辑画布尺寸。

use crate::metal_renderer::MetalHeadlessRenderer;
use gpui::{DevicePixels, PlatformHeadlessRenderer, Scene, size};

/// 按逻辑画布尺寸乘以缩放因子渲染到 RGBA 图像；调用方负责事先 finish 场景。
pub(crate) fn render_scene(
    renderer: &mut MetalHeadlessRenderer,
    scene: &Scene,
    logical: (f32, f32),
    factor: f32,
) -> image::RgbaImage {
    renderer
        .render_scene_to_image(
            scene,
            size(
                DevicePixels((logical.0 * factor) as i32),
                DevicePixels((logical.1 * factor) as i32),
            ),
        )
        .unwrap()
}
