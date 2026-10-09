//! 连续CSS参数先生成一张紧凑遮罩，再把长轴的恒定区域拉伸到实际卡片。

use super::{
    raster::{self, ShadowSample, Slice},
    textures::{Retired, Texture, TextureRole, paint_sliced},
};
use gpui_kit::{Bounds, Pixels, Window};

/// 缓存键只描述当前物理几何及当前动画采样，不形成时间点字典。
#[derive(Clone, PartialEq)]
struct Key {
    width: u32,
    height: u32,
    scale: u32,
    radius: u32,
    sample: ShadowSample,
}

/// 一张当前阴影和最多九个源片；离屏、隐藏或释放时可直接清空。
#[derive(Default)]
pub(super) struct Shadow {
    texture: Texture<Key>,
    x: Vec<Slice>,
    y: Vec<Slice>,
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
    ) {
        let scale = window.scale_factor();
        let width = (f32::from(bounds.size.width) * scale).round().max(0.) as u32;
        let height = (f32::from(bounds.size.height) * scale).round().max(0.) as u32;
        if width == 0 || height == 0 {
            self.clear(window);
            return;
        }
        let key = Key {
            width,
            height,
            scale: scale.to_bits(),
            radius: f32::from(radius).to_bits(),
            sample,
        };
        let changed = self.texture.key.as_ref() != Some(&key);
        let mut mapping = None;
        let image = self
            .texture
            .ensure(key, TextureRole::Shadow, retired, window, || {
                let data = raster::shadow(
                    width,
                    height,
                    f64::from(scale),
                    f64::from(f32::from(radius)),
                    sample.scaled(f64::from(scale)),
                );
                mapping = Some((data.x, data.y));
                data.pixels
            });
        if changed {
            let (x, y) = mapping.expect("新阴影同时提供源片映射");
            self.x = x;
            self.y = y;
        }
        paint_sliced(bounds, &self.x, &self.y, image, window);
    }

    /// 当前图和映射一起清除，不把CPU句柄释放误认为GPU图集已释放。
    pub fn clear(&mut self, window: &mut Window) {
        self.texture.clear(window);
        self.x.clear();
        self.y.clear();
    }

    /// 同一ID切换到无阴影筛选条时，旧图仅保留到已提交帧之后再释放。
    pub fn retire(&mut self, retired: &mut Retired, window: &mut Window) {
        self.texture.retire(TextureRole::Shadow, retired, window);
        self.x.clear();
        self.y.clear();
    }

    /// 测试取得实际上传的当前图，验证owner释放而非只验证CPU缓存长度。
    #[cfg(test)]
    pub fn image(&self) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
        self.texture.image.clone()
    }
}
