//! 只有不等宽顶accent卡使用紧凑边框纹理，普通卡片继续原生单色边框。

use super::{
    Geometry,
    palette::MaterialSample,
    raster::rim::{RimColors, RimGeometry},
    textures::{Retired, Texture, TextureRole, paint_sliced},
};
use gpui_kit::{Bounds, Pixels, Rgba, Window};

/// 精确几何独立于颜色；保留比例、半径及顶边原始位模式。
#[derive(Clone, Copy, PartialEq)]
struct GeometryKey {
    /// 宽度。
    width: u32,
    /// 高度。
    height: u32,
    /// 缩放比例的 f32 位模式。
    scale: u32,
    /// 圆角半径的 f32 位模式。
    radius: u32,
    /// 顶边宽度的 f32 位模式。
    top: u32,
}

/// 当前动态颜色只替换纹理，不保留历史主题图像。
#[derive(Clone, PartialEq)]
struct Key {
    /// 几何键。
    geometry: GeometryKey,
    /// 边框颜色。
    colors: RimColors,
}

/// 小型九切片保留CSS各边颜色和椭圆padding-box，不用矩形顶条遮住圆角。
#[derive(Default)]
pub(super) struct Rim {
    /// 当前纹理。
    texture: Texture<Key>,
    /// 当前几何键与采样计划。
    geometry: Option<(GeometryKey, RimGeometry)>,
    /// 测试中重建几何的次数。
    #[cfg(test)]
    geometry_builds: usize,
}

impl Rim {
    /// 颜色仅在CSS采样值改变时栅格化，长卡宽高不扩大角图分配。
    pub fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        geometry: Geometry,
        sample: &MaterialSample,
        retired: &mut Retired,
        window: &mut Window,
    ) {
        let scale = window.scale_factor();
        let width = (f32::from(bounds.size.width) * scale).round() as u32;
        let height = (f32::from(bounds.size.height) * scale).round() as u32;
        if width == 0 || height == 0 {
            self.clear(window);
            return;
        }
        let radius = f32::from(geometry.radius);
        let top = f32::from(geometry.top_width);
        let colors = RimColors {
            main: Rgba::from(sample.border),
            top: Rgba::from(sample.top),
            highlight: Rgba::from(sample.highlight),
        };
        let geometry_key = GeometryKey {
            width,
            height,
            scale: scale.to_bits(),
            radius: radius.to_bits(),
            top: top.to_bits(),
        };
        if self.geometry.as_ref().map(|(key, _)| *key) != Some(geometry_key) {
            #[cfg(test)]
            { self.geometry_builds += 1; }
            self.geometry = Some((geometry_key, RimGeometry::new(width, height,
                f64::from(scale), f64::from(radius), f64::from(top))));
        }
        let geometry = &self.geometry.as_ref().unwrap().1;
        let key = Key { geometry: geometry_key, colors };
        let image = self
            .texture
            .ensure(key, TextureRole::Rim, retired, window, || {
                geometry.render(colors)
            });
        let (x, y) = geometry.slices();
        paint_sliced(bounds, x, y, image, window);
    }

    /// accent移除时也遵循一代退休资源界限。
    pub fn retire(&mut self, retired: &mut Retired, window: &mut Window) {
        self.texture.retire(TextureRole::Rim, retired, window);
        self.geometry = None;
    }

    /// 隐藏和owner释放同步清空图集及映射。
    pub fn clear(&mut self, window: &mut Window) {
        self.texture.clear(window);
        self.geometry = None;
    }

    /// 测试核对实际准备次数及释放状态，不以分配地址推断命中。
    #[cfg(test)]
    pub(super) fn geometry_audit(&self) -> (usize, bool) {
        (self.geometry_builds, self.geometry.is_some())
    }

    /// 生命周期回归取得当前实际边框纹理，覆盖accent卡的清理路径。
    #[cfg(test)]
    pub fn image(&self) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        self.texture.image.clone()
    }
}
