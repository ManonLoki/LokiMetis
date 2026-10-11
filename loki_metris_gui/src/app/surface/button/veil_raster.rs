//! Loading伪元素的实心圆角Gaussian；±4σ截断及物理行积分不宣称WebKit逐像素等同。

use crate::app::surface::raster::Pixels;

/// 纹理只保留两侧模糊支持区，长按钮中段用同一恒定列展开。
pub(super) struct Raster {
    pub pixels: Pixels,
    pub margin: u32,
    pub canonical_width: u32,
    pub cap: u32,
}

/// 正态CDF标准近似，最大绝对误差约1.5e-7，远小于8位alpha的量化步长。
fn cdf(value: f64) -> f64 {
    let x = value.abs();
    let t = 1. / (1. + 0.231_641_9 * x);
    let density = (-x * x / 2.).exp() / std::f64::consts::TAU.sqrt();
    let tail = density
        * t
        * (0.319_381_530
            + t * (-0.356_563_782
                + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    if value < 0. { tail } else { 1. - tail }
}

/// blur(12px)的长度是标准差；水平精确积分、垂直四子行积分保留短按钮的上下重叠。
pub(super) fn raster(width: u32, height: u32, scale: f64, radius: f64, dark: bool) -> Raster {
    assert!(width > 0 && height > 0 && scale.is_finite() && scale > 0.);
    let sigma = 12. * scale;
    let radius = (radius * scale).min(f64::from(width.min(height)) / 2.);
    let cap = (radius + 4. * sigma).ceil() as u32;
    let canonical_width = width.min(cap.saturating_mul(2).saturating_add(1));
    let margin = (4. * sigma).ceil() as u32 + 1;
    let w = canonical_width + 2 * margin;
    let h = height + 2 * margin;
    let samples = height as usize * 4;
    // 每个子行的水平覆盖只计算一次；动画期间不重复创建此栅格。
    let mut horizontal = vec![0.; samples * w as usize];
    for row in 0..samples {
        let y = (row as f64 + 0.5) / 4.;
        let edge = y.min(f64::from(height) - y);
        let inset = if edge < radius {
            radius - (radius * radius - (radius - edge).powi(2)).max(0.).sqrt()
        } else {
            0.
        };
        for column in 1..w - 1 {
            let x = f64::from(column) + 0.5 - f64::from(margin);
            horizontal[row * w as usize + column as usize] =
                cdf((x - inset) / sigma) - cdf((x - (f64::from(canonical_width) - inset)) / sigma);
        }
    }
    let mut bytes = vec![0; w as usize * h as usize * 4];
    let color = if dark { 0 } else { 255 };
    for row in 1..h - 1 {
        let y = f64::from(row) + 0.5 - f64::from(margin);
        let weights: Vec<_> = (0..samples)
            .map(|source| {
                let low = source as f64 / 4.;
                cdf((y - low) / sigma) - cdf((y - low - 0.25) / sigma)
            })
            .collect();
        // 连续扫描源行，避免每个输出像素跨行随机访问；不改变积分顺序与精度。
        let mut alpha_row = vec![0.; w as usize];
        for (source, weight) in weights.iter().enumerate() {
            let source_row = &horizontal[source * w as usize..(source + 1) * w as usize];
            for (alpha, coverage) in alpha_row.iter_mut().zip(source_row) {
                *alpha += weight * coverage;
            }
        }
        for column in 1..w - 1 {
            let alpha = alpha_row[column as usize];
            let index = (row as usize * w as usize + column as usize) * 4;
            bytes[index..index + 3].fill(color);
            bytes[index + 3] = (alpha * 0.15 * 255.).round().clamp(0., 255.) as u8;
        }
    }
    Raster {
        pixels: Pixels {
            width: w,
            height: h,
            bytes,
        },
        margin,
        canonical_width,
        cap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 矩形中心与独立正态积分值比较，防止误用box-shadow的sigma/2或扣空内部。
    #[test]
    fn short_solid_rectangle_keeps_vertical_gaussian_overlap() {
        let image = raster(100, 36, 1., 0., false);
        let at = |x: u32, y: u32| {
            image.pixels.bytes
                [((y + image.margin) * image.pixels.width + x + image.margin) as usize * 4 + 3]
        };
        assert_eq!(at(49, 17), 33, "sigma12、36px短轴中心约为0.15×0.8664");
        assert_eq!(at(49, 17), at(50, 18));
        assert!((17..=22).contains(&at(1, 17)), "近边保留Gaussian衰减");
        assert!(image.pixels.bytes.chunks_exact(4).all(|p| p[3] <= 38));
    }

    /// 主题只改变直通RGB，长轴纹理有界且透明采样保护不泄漏白边。
    #[test]
    fn themes_share_alpha_and_long_buttons_keep_bounded_texture() {
        let light = raster(4000, 60, 2., 12., false);
        let dark = raster(4000, 60, 2., 12., true);
        assert!(light.pixels.width < 500);
        assert!(light.canonical_width < 300);
        assert_eq!(light.pixels.width, dark.pixels.width);
        for (a, b) in light
            .pixels
            .bytes
            .chunks_exact(4)
            .zip(dark.pixels.bytes.chunks_exact(4))
        {
            assert_eq!(a[3], b[3]);
            if a[3] > 0 {
                assert_eq!(&a[..3], &[255; 3]);
                assert_eq!(&b[..3], &[0; 3]);
            }
        }
        let w = light.pixels.width as usize;
        assert!(light.pixels.bytes[..w * 4].iter().all(|p| *p == 0));
    }
}
