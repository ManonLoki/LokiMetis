//! 阴影只在固定大小的紧凑纹理中栅格化；4σ截断是有界近似，不宣称与 WebKit 逐像素相同。

use gpui_kit::RenderImage;
pub(super) mod rim;
use std::sync::{Arc, OnceLock};

/// 纹理使用直通 BGRA 像素；所有尺寸均为物理像素，外围保留一圈采样保护。
pub(super) struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

impl Pixels {
    /// 把有界的已校验像素交给 GPUI，不经过异步解码或宿主文件。
    pub fn into_image(self) -> Arc<RenderImage> {
        let pixels = image::RgbaImage::from_raw(self.width, self.height, self.bytes)
            .expect("材质像素长度与物理尺寸一致");
        Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
    }
}

/// 一个轴的九切片映射；源坐标含纹理外扩，目标坐标相对原卡片的近边。
#[derive(Clone, Copy, Debug)]
pub(super) struct Slice {
    pub source_start: f64,
    pub source_end: f64,
    pub target_start: f64,
    pub target_end: f64,
}

/// 紧凑投影与映射同时生成，使短卡片的两端模糊保留真实重叠。
pub(super) struct ShadowRaster {
    pub pixels: Pixels,
    pub x: Vec<Slice>,
    pub y: Vec<Slice>,
}

/// 两个主题共享的 CSS 阴影参数，sigma 已转换为物理像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ShadowSample {
    pub sigma: f64,
    pub spread: f64,
    pub dy: f64,
    pub alpha: f64,
    pub bgra: [u8; 3],
}

impl ShadowSample {
    /// 属性时间线保存逻辑参数，仅在设备比例确定后转换几何为物理像素。
    pub fn scaled(self, scale: f64) -> Self {
        Self {
            sigma: self.sigma * scale,
            spread: self.spread * scale,
            dy: self.dy * scale,
            ..self
        }
    }
    /// 先在 CSS 参数空间连续插值，再转换 sigma；不交叉淡化端点位图。
    pub fn new(dark: bool, progress: f32, scale: f64) -> Self {
        let t = f64::from(progress.clamp(0., 1.));
        let mix = |a: f64, b: f64| a + (b - a) * t;
        if dark {
            Self {
                sigma: mix(21., 24.) * scale,
                spread: mix(-38., -30.) * scale,
                dy: mix(16., 20.) * scale,
                alpha: mix(0.72, 0.85),
                bgra: [0, 0, 0],
            }
        } else {
            Self {
                sigma: mix(20., 24.) * scale,
                spread: mix(-22., -20.) * scale,
                dy: mix(20., 24.) * scale,
                alpha: mix(0.16, 0.24),
                bgra: [36, 63, 91],
            }
        }
    }
}

/// 以固定安全 cap 压缩长轴；短轴完整保留，不把重叠的 Gaussian 拉成常量。
fn slices(actual: u32, before: u32, after: u32, near: u32, far: u32) -> (u32, Vec<Slice>) {
    let canonical = actual.min(before + after + 1);
    let full = canonical + near + far + 2;
    let segment = |s0: f64, s1: f64, d0: f64, d1: f64| Slice {
        source_start: s0,
        source_end: s1,
        target_start: d0,
        target_end: d1,
    };
    if canonical == actual {
        return (
            canonical,
            vec![segment(
                1.,
                f64::from(full - 1),
                -f64::from(near),
                f64::from(actual + far),
            )],
        );
    }
    let first = f64::from(near + before + 1);
    (
        canonical,
        vec![
            segment(1., first, -f64::from(near), f64::from(before)),
            segment(
                first,
                first + 1.,
                f64::from(before),
                f64::from(actual - after),
            ),
            segment(
                first + 1.,
                f64::from(full - 1),
                f64::from(actual - after),
                f64::from(actual + far),
            ),
        ],
    )
}

/// 创建至多约405×405 logical px的阴影；空投影不留下内部黑色。
pub(super) fn shadow(
    width: u32,
    height: u32,
    scale: f64,
    radius: f64,
    sample: ShadowSample,
) -> ShadowRaster {
    let cap = |value: f64| (value * scale).ceil() as u32;
    let (near_x, near_y) = (cap(76.), cap(52.));
    let (card_w, x) = slices(width, cap(126.), cap(126.), near_x, cap(76.));
    let (card_h, y) = slices(height, cap(146.), cap(106.), near_y, cap(100.));
    let (w, h) = (
        card_w + near_x + cap(76.) + 2,
        card_h + near_y + cap(100.) + 2,
    );
    let cast = [
        -sample.spread,
        sample.dy - sample.spread,
        f64::from(card_w) + sample.spread,
        f64::from(card_h) + sample.dy + sample.spread,
    ];
    let profile = |position: f64, lo: f64, hi: f64| {
        if hi <= lo {
            0.
        } else {
            cdf((position - lo) / sample.sigma) - cdf((position - hi) / sample.sigma)
        }
    };
    let ax: Vec<_> = (0..w)
        .map(|i| profile(f64::from(i) - f64::from(near_x) - 0.5, cast[0], cast[2]))
        .collect();
    let ay: Vec<_> = (0..h)
        .map(|i| profile(f64::from(i) - f64::from(near_y) - 0.5, cast[1], cast[3]))
        .collect();
    let mut bytes = vec![0; w as usize * h as usize * 4];
    // 当前产品radius≤20且所有CSS spread≤-20，负spread后casting radius严格收敛为0。
    assert!(
        radius >= 0. && radius <= 20.,
        "品牌材质半径必须处于当前产品的0..20px范围"
    );
    let radius = (radius * scale).min(f64::from(card_w.min(card_h)) / 2.);
    for row in 1..h - 1 {
        for column in 1..w - 1 {
            let px = f64::from(column) - f64::from(near_x) - 0.5;
            let py = f64::from(row) - f64::from(near_y) - 0.5;
            let outside =
                1. - rounded_coverage(px, py, f64::from(card_w), f64::from(card_h), radius);
            let alpha = sample.alpha * ax[column as usize] * ay[row as usize] * outside;
            let index = (row as usize * w as usize + column as usize) * 4;
            bytes[index..index + 3].copy_from_slice(&sample.bgra);
            bytes[index + 3] = (alpha * 255.).round().clamp(0., 255.) as u8;
        }
    }
    ShadowRaster {
        pixels: Pixels {
            width: w,
            height: h,
            bytes,
        },
        x,
        y,
    }
}

/// 原卡片的圆角覆盖率以物理像素计算，和 GPUI quad 一样保留一像素抗锯齿带。
pub(super) fn rounded_coverage(x: f64, y: f64, width: f64, height: f64, radius: f64) -> f64 {
    let qx = (x - width / 2.).abs() - width / 2. + radius;
    let qy = (y - height / 2.).abs() - height / 2. + radius;
    let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius;
    (0.5 - distance).clamp(0., 1.)
}

/// 标准 Gaussian 在±4σ外截断并归一化；LUT积分与插值均使用标准库，不引入数学依赖。
fn cdf(z: f64) -> f64 {
    const STEPS: usize = 2048;
    static TABLE: OnceLock<Vec<f64>> = OnceLock::new();
    if z <= -4. {
        return 0.;
    }
    if z >= 4. {
        return 1.;
    }
    let table = TABLE.get_or_init(|| {
        let step = 8. / STEPS as f64;
        let gaussian = |x: f64| (-x * x / 2.).exp();
        let mut values = vec![0.; STEPS + 1];
        for i in 0..STEPS {
            let x = -4. + i as f64 * step;
            values[i + 1] = values[i]
                + step / 6. * (gaussian(x) + 4. * gaussian(x + step / 2.) + gaussian(x + step));
        }
        let total = values[STEPS];
        for value in &mut values {
            *value /= total;
        }
        values
    });
    let position = (z + 4.) / 8. * STEPS as f64;
    let i = position.floor() as usize;
    table[i] + (table[i + 1] - table[i]) * (position - i as f64)
}

/// 固定径向纹理含透明采样边距，颜色不做预乘，交给 GPUI 正常 alpha 合成。
pub(super) fn radial(scale: f64, color: [u8; 3]) -> Pixels {
    let half = (196. * scale).ceil() as u32 + 1;
    let width = half * 2;
    let mut bytes = vec![0; width as usize * width as usize * 4];
    for y in 0..width {
        for x in 0..width {
            let alpha = radial_alpha(
                f64::from(x) + 0.5 - f64::from(half),
                f64::from(y) + 0.5 - f64::from(half),
                scale,
            );
            let index = (y as usize * width as usize + x as usize) * 4;
            bytes[index..index + 3].copy_from_slice(&color);
            bytes[index + 3] = (alpha * 255.).round() as u8;
        }
    }
    Pixels {
        width,
        height: width,
        bytes,
    }
}

/// 280px circle在70%处到达透明，非零半径因此是196px。
pub(super) fn radial_alpha(dx: f64, dy: f64, scale: f64) -> f64 {
    0.08 * (1. - dx.hypot(dy) / (196. * scale)).clamp(0., 1.)
}

#[cfg(test)]
mod tests;
