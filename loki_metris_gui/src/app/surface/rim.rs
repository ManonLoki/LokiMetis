//! 只有不等宽顶accent卡使用紧凑边框纹理，普通卡片继续原生单色边框。

use super::{
    Geometry,
    palette::MaterialSample,
    raster::{self, Slice, rim::RimColors},
    textures::{Retired, Texture, TextureRole, paint_sliced},
};
use gpui_kit::{Bounds, Pixels, Rgba, Window};

/// 当前边框样本与物理布局构成唯一缓存键，不按动画时间累计图像。
#[derive(Clone, PartialEq)]
struct Key {
    width: u32,
    height: u32,
    scale: u32,
    radius: u32,
    top: u32,
    colors: RimColors,
}

/// 小型九切片保留CSS各边颜色和椭圆padding-box，不用矩形顶条遮住圆角。
#[derive(Default)]
pub(super) struct Rim {
    texture: Texture<Key>,
    x: Vec<Slice>,
    y: Vec<Slice>,
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
        let key = Key {
            width,
            height,
            scale: scale.to_bits(),
            radius: radius.to_bits(),
            top: top.to_bits(),
            colors,
        };
        let changed = self.texture.key.as_ref() != Some(&key);
        let mut mapping = None;
        let image = self
            .texture
            .ensure(key, TextureRole::Rim, retired, window, || {
                let data = raster::rim::render(
                    width,
                    height,
                    f64::from(scale),
                    f64::from(radius),
                    f64::from(top),
                    colors,
                );
                mapping = Some((data.x, data.y));
                data.pixels
            });
        if changed {
            let (x, y) = mapping.expect("边框纹理同时提供原坐标切片映射");
            self.x = x;
            self.y = y;
        }
        paint_sliced(bounds, &self.x, &self.y, image, window);
    }

    /// accent移除时也遵循一代退休资源界限。
    pub fn retire(&mut self, retired: &mut Retired, window: &mut Window) {
        self.texture.retire(TextureRole::Rim, retired, window);
        self.x.clear();
        self.y.clear();
    }

    /// 隐藏和owner释放同步清空图集及映射。
    pub fn clear(&mut self, window: &mut Window) {
        self.texture.clear(window);
        self.x.clear();
        self.y.clear();
    }

    /// 生命周期回归取得当前实际边框纹理，覆盖accent卡的清理路径。
    #[cfg(test)]
    pub fn image(&self) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        self.texture.image.clone()
    }
}
