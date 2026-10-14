//! 光斑主体复用固定纹理，四角以原卡片坐标烘焙圆角coverage，避免图片相交后的伪圆角。

use super::{
    raster::{self, Pixels as RasterPixels},
    textures::{Retired, Texture, TextureRole},
};
use gpui_kit::{Bounds, ContentMask, Corners, Pixels, Point, RenderImage, Window, point, px, size};
use std::sync::Arc;

/// 径向主体只因主题或设备比例改变，鼠标移动不会生成新主体纹理。
#[derive(Clone, PartialEq)]
struct BaseKey {
    dark: bool,
    scale: u32,
}

/// 圆角纹理只拥有当前位置的键；坐标均相对原卡片，布局变化不能借用过期mask。
#[derive(Clone, PartialEq)]
struct CornerKey {
    base: BaseKey,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    pointer_x: i64,
    pointer_y: i64,
    radius: u64,
}

/// 固定一张径向图与四个小圆角槽，退出hover时退休所有光斑资源。
#[derive(Default)]
pub(super) struct Spotlight {
    base: Texture<BaseKey>,
    corners: [Texture<CornerKey>; 4],
}

impl Spotlight {
    /// 三个无圆角区域使用矩形mask，四角单独烘焙原圆角，因此没有图片交集边缘错误。
    #[expect(clippy::too_many_arguments, reason = "绘制需要的几何、主题与退休资源是本帧独立输入，另加 GPUI 窗口")]
    pub fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        pointer: Point<Pixels>,
        dark: bool,
        radius: Pixels,
        top_width: Pixels,
        retired: &mut Retired,
        window: &mut Window,
    ) {
        let scale = window.scale_factor();
        let scale64 = f64::from(scale);
        let width = (f32::from(bounds.size.width) * scale).round().max(0.) as u32;
        let height = (f32::from(bounds.size.height) * scale).round().max(0.) as u32;
        if width == 0 || height == 0 {
            self.retire(retired, window);
            return;
        }
        let base = BaseKey {
            dark,
            scale: scale.to_bits(),
        };
        let color = if dark {
            [0x8a, 0xa0, 0xf2]
        } else {
            [0x25, 0x2f, 0xa5]
        };
        let image = self
            .base
            .ensure(base.clone(), TextureRole::Radial, retired, window, || {
                raster::radial(scale64, color)
            });
        // background-origin为padding-box；顶accent占3px时，径向中心同样保留3px顶偏移。
        let center = point(
            window.pixel_snap(pointer.x + px(1.)),
            window.pixel_snap(pointer.y + top_width),
        );
        let pointer_x = (f32::from(center.x - bounds.origin.x) * scale).round() as i64;
        let pointer_y = (f32::from(center.y - bounds.origin.y) * scale).round() as i64;
        let radius =
            (f64::from(f32::from(radius)) * scale64).min(f64::from(width.min(height)) / 2.);
        let corner = radius.ceil() as u32;
        let left = corner.min(width.div_ceil(2));
        let right = corner.min(width - left);
        let top = corner.min(height.div_ceil(2));
        let bottom = corner.min(height - top);
        let rect = |x: u32, y: u32, w: u32, h: u32| {
            Bounds::new(
                bounds.origin + point(px(x as f32 / scale), px(y as f32 / scale)),
                size(px(w as f32 / scale), px(h as f32 / scale)),
            )
        };
        let texture_size = image.size(0);
        let image_bounds = Bounds::new(
            center
                - point(
                    px(texture_size.width.0 as f32 / scale / 2.),
                    px(texture_size.height.0 as f32 / scale / 2.),
                ),
            size(
                px(texture_size.width.0 as f32 / scale),
                px(texture_size.height.0 as f32 / scale),
            ),
        );
        for region in [
            rect(left, 0, width - left - right, height),
            rect(0, top, left, height - top - bottom),
            rect(width - right, top, right, height - top - bottom),
        ] {
            paint_region(region, image_bounds, image.clone(), window);
        }
        for (index, (x, y, w, h)) in [
            (0, 0, left, top),
            (width - right, 0, right, top),
            (0, height - bottom, left, bottom),
            (width - right, height - bottom, right, bottom),
        ]
        .into_iter()
        .enumerate()
        {
            let region = rect(x, y, w, h);
            if w == 0 || h == 0 || region.intersect(&image_bounds).is_empty() {
                self.corners[index].retire(TextureRole::corner(index), retired, window);
                continue;
            }
            let key = CornerKey {
                base: base.clone(),
                width,
                height,
                x,
                y,
                w,
                h,
                pointer_x,
                pointer_y,
                radius: radius.to_bits(),
            };
            let image = self.corners[index].ensure(
                key.clone(),
                TextureRole::corner(index),
                retired,
                window,
                || corner_pixels(&key, radius, scale64, color),
            );
            let border = px(1. / scale);
            paint_region(
                region,
                Bounds::new(
                    region.origin - point(border, border),
                    region.size + size(border * 2., border * 2.),
                ),
                image,
                window,
            );
        }
    }

    /// 立即撤掉光斑但在本帧之后回收图集，阴影和边框仍按自己的时间线回退。
    pub fn retire(&mut self, retired: &mut Retired, window: &mut Window) {
        self.base.retire(TextureRole::Radial, retired, window);
        for (index, corner) in self.corners.iter_mut().enumerate() {
            corner.retire(TextureRole::corner(index), retired, window);
        }
    }

    /// 隐藏及owner释放不再等待hover事件，所有当前图集条目同步移除。
    pub fn clear(&mut self, window: &mut Window) {
        self.base.clear(window);
        for corner in &mut self.corners {
            corner.clear(window);
        }
    }

    /// 测试枚举当前光斑图，隐藏及卸载必须逐个移除这些真实atlas条目。
    #[cfg(test)]
    pub fn images(&self) -> Vec<Arc<RenderImage>> {
        self.base
            .image
            .iter()
            .chain(
                self.corners
                    .iter()
                    .filter_map(|corner| corner.image.as_ref()),
            )
            .cloned()
            .collect()
    }
}

/// 将完整纹理放在原坐标，仅以矩形clip裁切，避免paint_image按相交矩形改变UV或圆角。
fn paint_region(
    region: Bounds<Pixels>,
    image_bounds: Bounds<Pixels>,
    image: Arc<RenderImage>,
    window: &mut Window,
) {
    if region.is_empty() {
        return;
    }
    window.with_content_mask(Some(ContentMask { bounds: region }), |window| {
        let _ = window.paint_image(
            image_bounds,
            image_bounds,
            Corners::default(),
            image,
            0,
            false,
        );
    });
}

/// 小角图在原卡片上采样；一圈重复边缘只用于atlas线性采样，不参与实际圆角几何。
fn corner_pixels(key: &CornerKey, radius: f64, scale: f64, color: [u8; 3]) -> RasterPixels {
    let (width, height) = (key.w + 2, key.h + 2);
    let mut bytes = vec![0; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let at_x = f64::from(key.x + x.saturating_sub(1).min(key.w - 1)) + 0.5;
            let at_y = f64::from(key.y + y.saturating_sub(1).min(key.h - 1)) + 0.5;
            let alpha = raster::radial_alpha(
                at_x - key.pointer_x as f64,
                at_y - key.pointer_y as f64,
                scale,
            ) * raster::rounded_coverage(
                at_x,
                at_y,
                f64::from(key.width),
                f64::from(key.height),
                radius,
            );
            let index = (y as usize * width as usize + x as usize) * 4;
            bytes[index..index + 3].copy_from_slice(&color);
            bytes[index + 3] = (alpha * 255.).round() as u8;
        }
    }
    RasterPixels {
        width,
        height,
        bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// slot和普通卡片相同鼠标位置时使用各自原圆角，r12的可见角不能仍被r20遮罩裁掉。
    #[test]
    fn corner_masks_use_configured_radius_and_distinct_cache_key() {
        let key = |radius: f64| CornerKey {
            base: BaseKey {
                dark: false,
                scale: 1_f32.to_bits(),
            },
            width: 100,
            height: 100,
            x: 0,
            y: 0,
            w: 20,
            h: 20,
            pointer_x: 0,
            pointer_y: 0,
            radius: radius.to_bits(),
        };
        let medium_key = key(12.);
        let large_key = key(20.);
        assert!(medium_key != large_key);
        let medium = corner_pixels(&medium_key, 12., 1., [0x25, 0x2f, 0xa5]);
        let large = corner_pixels(&large_key, 20., 1., [0x25, 0x2f, 0xa5]);
        let index = ((6 * medium.width + 6) * 4 + 3) as usize;
        assert!(medium.bytes[index] > 0);
        assert_eq!(large.bytes[index], 0);
        assert_eq!(medium.bytes[3], 0);
        assert_eq!(large.bytes[3], 0);
    }
}
