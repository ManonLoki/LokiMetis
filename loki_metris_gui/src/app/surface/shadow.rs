//! 连续CSS参数先生成一张紧凑遮罩，再把长轴的恒定区域拉伸到实际卡片。

use super::{
    raster::{ShadowGeometry, ShadowSample},
    textures::{Retired, paint_sliced},
    shadow_shared::{self, Key, Lease},
};
use gpui_kit::{App, Bounds, Pixels, Window};
use std::rc::Rc;

/// 原始比例与半径位模式决定唯一几何计划，不对连续值量化。
#[derive(Clone, Copy, PartialEq)]
struct GeometryKey {
    width: u32,
    height: u32,
    scale: u32,
    radius: u32,
}

/// 一张当前阴影和最多九个源片；离屏、隐藏或释放时可直接清空。
#[derive(Default)]
pub(super) struct Shadow {
    texture: Option<Rc<Lease>>,
    geometry: Option<(GeometryKey, ShadowGeometry)>,
    #[cfg(test)]
    geometry_builds: usize,
}

impl Shadow {
    /// 实际卡片宽高不决定纹理上限；短轴完整保留，模糊重叠不会被错误拉伸。
    pub fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        sample: ShadowSample,
        retired: &mut Retired,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scale = window.scale_factor();
        let width = (f32::from(bounds.size.width) * scale).round().max(0.) as u32;
        let height = (f32::from(bounds.size.height) * scale).round().max(0.) as u32;
        if width == 0 || height == 0 {
            self.clear(window);
            return;
        }
        let geometry_key = GeometryKey {
            width,
            height,
            scale: scale.to_bits(),
            radius: f32::from(radius).to_bits(),
        };
        if self.geometry.as_ref().map(|(key, _)| *key) != Some(geometry_key) {
            #[cfg(test)]
            { self.geometry_builds += 1; }
            self.geometry = Some((geometry_key, ShadowGeometry::new(width, height,
                f64::from(scale), f64::from(f32::from(radius)))));
        }
        let geometry = &self.geometry.as_ref().unwrap().1;
        let key = Key::new(geometry.canonical_dimensions(), scale, f32::from(radius), sample);
        if self.texture.as_ref().map(|lease| lease.key) != Some(key) {
            let next = shadow_shared::acquire(key, window, cx, || {
                geometry.render(sample.scaled(f64::from(scale)))
            });
            if let Some(previous) = self.texture.replace(next) {
                retired.push_shared_shadow(previous, window);
            }
        }
        let image = self.texture.as_ref().unwrap().image.clone();
        let (x, y) = geometry.slices();
        paint_sliced(bounds, x, y, image, window);
    }

    /// 当前图和映射一起清除，不把CPU句柄释放误认为GPU图集已释放。
    pub fn clear(&mut self, window: &mut Window) {
        if let Some(texture) = self.texture.take() { texture.release(window); }
        self.geometry = None;
    }

    /// 同一ID切换到无阴影筛选条时，旧图仅保留到已提交帧之后再释放。
    pub fn retire(&mut self, retired: &mut Retired, window: &mut Window) {
        if let Some(texture) = self.texture.take() { retired.push_shared_shadow(texture, window); }
        self.geometry = None;
    }

    /// 测试读取实际准备次数和释放状态，不以分配地址推断缓存命中。
    #[cfg(test)]
    pub(super) fn geometry_audit(&self) -> (usize, bool) {
        (self.geometry_builds, self.geometry.is_some())
    }

    /// 测试取得实际上传的当前图，验证owner释放而非只验证CPU缓存长度。
    #[cfg(test)]
    pub fn image(&self) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        self.texture.as_ref().map(|texture| texture.image.clone())
    }
}
