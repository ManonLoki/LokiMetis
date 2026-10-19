//! 独立加载遮罩绘于背景和原生内容之间，保留完整UV及固定padding-box圆角。

use super::{ButtonMotion, veil_raster};
use crate::app::surface::textures::{Retired, Texture, TextureRole};
use gpui_kit::{
    AnyElement, Bounds, Corners, ImageRoundedClip, Pixels, WeakEntity, Window, canvas, div, point,
    prelude::*, px, size,
};

/// 每个按钮最多当前和退休一张遮罩，不按动画时刻建立新缓存键。
#[derive(Default)]
pub(super) struct Veil {
    /// 当前遮罩纹理。
    texture: Texture<Key>,
    /// 待回收的旧纹理。
    retired: Retired,
    /// 当前纹理的切片几何 (两侧模糊宽度, 规范宽度, 端部列宽)；纹理退休时清空。
    geometry: Option<(u32, u32, u32)>,
}

/// 实际border-box、设备比例、半径及主题决定唯一静态像素。
#[derive(PartialEq)]
struct Key {
    /// border-box 宽度（物理像素）。
    width: u32,
    /// border-box 高度（物理像素）。
    height: u32,
    /// 设备缩放比例的 f32 位模式。
    scale: u32,
    /// 圆角半径的 f32 位模式。
    radius: u32,
    /// 深色主题。
    dark: bool,
}

impl Veil {
    /// 位移属于CSS transform，不进入像素缓存；九切片保留短轴完整Gaussian。
    pub(super) fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        dark: bool,
        slide: f32,
        window: &mut Window,
    ) {
        let scale = window.scale_factor();
        let width = (f32::from(bounds.size.width) * scale).ceil() as u32;
        let height = (f32::from(bounds.size.height) * scale).ceil() as u32;
        if width == 0 || height == 0 {
            return;
        }
        let key = Key {
            width,
            height,
            scale: scale.to_bits(),
            radius: f32::from(radius).to_bits(),
            dark,
        };
        let geometry = &mut self.geometry;
        let image =
            self.texture
                .ensure(key, TextureRole::Shadow, &mut self.retired, window, || {
                    let raster = veil_raster::raster(
                        width,
                        height,
                        f64::from(scale),
                        f64::from(f32::from(radius)),
                        dark,
                    );
                    *geometry = Some((raster.margin, raster.canonical_width, raster.cap));
                    raster.pixels
                });
        let (margin, canonical, cap) = self.geometry.expect("有效像素持有切片几何");
        let dimensions = image.size(0);
        let full_width = dimensions.width.0 as f32;
        let full_height = dimensions.height.0 as f32;
        let clip = ImageRoundedClip {
            bounds: bounds.dilate(-px(1.)),
            corner_radii: Corners::all((radius - px(1.)).max(px(0.))),
        };
        let m = margin as f32;
        let segments = if canonical == width {
            vec![(0., full_width, -m, width as f32 + m)]
        } else {
            let c = cap as f32;
            vec![
                (0., m + c, -m, c),
                (m + c, m + c + 1., c, width as f32 - c),
                (m + c + 1., full_width, width as f32 - c, width as f32 + m),
            ]
        };
        let dy = -(1. - slide) * f32::from(bounds.size.height);
        for (s0, s1, d0, d1) in segments {
            let stretch = (d1 - d0) / (s1 - s0);
            let image_bounds = Bounds::new(
                bounds.origin + point(px((d0 - s0 * stretch) / scale), px(dy - m / scale)),
                size(px(full_width * stretch / scale), px(full_height / scale)),
            );
            let target = Bounds::new(
                bounds.origin + point(px(d0 / scale), px(dy - m / scale)),
                size(px((d1 - d0) / scale), px(full_height / scale)),
            )
            .intersect(&clip.bounds);
            if !target.is_empty() {
                let _ = window.paint_image_with_rounded_clip(
                    target,
                    image_bounds,
                    clip.clone(),
                    image.clone(),
                    0,
                    false,
                );
            }
        }
    }

    /// 旧图延迟一帧回收，当前scene仍可安全读取上一代atlas。
    pub(super) fn needs_cleanup(&self) -> bool {
        !self.retired.is_empty()
    }
    /// 释放已经退役的旧遮罩纹理（动画换新图后，旧图不必再保留）。
    pub(super) fn clear_retired(&mut self, window: &mut Window) {
        self.retired.clear(window);
    }
    /// 隐藏、卸载及动画透明端点都立即释放，不等待下一帧。
    pub(super) fn clear(&mut self, window: &mut Window) {
        self.texture.clear(window);
        self.retired.clear(window);
        self.geometry = None;
    }
    /// 取出当前遮罩的位图副本，只给测试核对像素用。
    #[cfg(test)]
    pub(super) fn image(&self) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        self.texture.image.clone()
    }
}

/// Div提供实际透明度作用域；canvas只绘制遮罩，不改变命中、辅助树或标签/Loader颜色。
pub(super) fn element(owner: WeakEntity<ButtonMotion>, opacity: f32, slide: f32) -> AnyElement {
    div()
        .size_full()
        .opacity(opacity)
        .child(
            canvas(
                |_, _, _| (),
                move |_, _, window, cx| {
                    if opacity <= 0. {
                        return;
                    }
                    let _ = owner.update(cx, |state, cx| {
                        if let Some((bounds, radius, dark)) = state.veil_geometry {
                            state.veil.paint(bounds, radius, dark, slide, window);
                            state.schedule_veil_cleanup(window, cx);
                        }
                    });
                },
            )
            .size_full(),
        )
        .into_any_element()
}
