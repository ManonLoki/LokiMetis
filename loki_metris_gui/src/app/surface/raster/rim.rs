//! 不等宽CSS边框保留椭圆内角和3:1角部分界，小纹理切片也包含顶部内高光。

use super::{Pixels, slices};
#[cfg(test)]
use super::ShadowRaster;
use gpui_kit::Rgba;

/// 直通sRGB颜色；像素覆盖期间先预乘合成，再恢复直通BGRA交给GPUI。
#[derive(Clone, Copy, PartialEq)]
pub(in crate::app::surface) struct RimColors {
    pub main: Rgba,
    pub top: Rgba,
    pub highlight: Rgba,
}

/// 互斥采样类别独立于颜色和透明度，不能按主题删去透明类别。
#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
enum SampleKind {
    Main,
    Top,
    Highlight,
}

/// 连续同类贡献仍逐次累加；单 texel 最多 64 次，可直接存入 u8。
struct SampleRun {
    kind: SampleKind,
    repeats: u8,
}

/// 每个 owner 仅保存当前圆角、粗顶边和内高光的原序采样计划。
pub(in crate::app::surface) struct RimGeometry {
    width: u32,
    height: u32,
    x: Vec<super::Slice>,
    y: Vec<super::Slice>,
    pixel_offsets: Vec<usize>,
    runs: Vec<SampleRun>,
}

impl RimGeometry {
    /// 几何变化时计算 8×8 分类，保留 clamp、cap、切片和 sy→sx 原顺序。
    pub fn new(width: u32, height: u32, scale: f64, radius: f64, top: f64) -> Self {
        let radius = (radius * scale).min(f64::from(width.min(height)) / 2.);
        let top = top * scale;
        let cap = (radius.max(top).max(scale) + 1.).ceil() as u32;
        let (cw, x) = slices(width, cap, cap, 0, 0);
        let (ch, y) = slices(height, cap, cap, 0, 0);
        let mut pixel_offsets = Vec::with_capacity(cw as usize * ch as usize + 1);
        let mut runs: Vec<SampleRun> = Vec::new();
        pixel_offsets.push(0);
        for row in 0..ch {
            for col in 0..cw {
                let start = runs.len();
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let px = f64::from(col) + (f64::from(sx) + 0.5) / f64::from(SAMPLES);
                        let py = f64::from(row) + (f64::from(sy) + 0.5) / f64::from(SAMPLES);
                        let Some(kind) = sample_kind(px, py, f64::from(cw), f64::from(ch), radius, top, scale) else {
                            continue;
                        };
                        if runs.len() > start && runs.last().unwrap().kind == kind {
                            runs.last_mut().unwrap().repeats += 1;
                        } else {
                            runs.push(SampleRun { kind, repeats: 1 });
                        }
                    }
                }
                pixel_offsets.push(runs.len());
            }
        }
        Self { width: cw + 2, height: ch + 2, x, y, pixel_offsets, runs }
    }

    /// 仅更新颜色；保留逐样本 f64 加法、原归一化及透明保护圈。
    pub fn render(&self, colors: RimColors) -> Pixels {
        let colors = [channels(colors.main), channels(colors.top), channels(colors.highlight)];
        let mut bytes = vec![0; self.width as usize * self.height as usize * 4];
        for (pixel, offsets) in self.pixel_offsets.windows(2).enumerate() {
            let color = shade(&self.runs[offsets[0]..offsets[1]], &colors);
            let row = pixel / (self.width - 2) as usize + 1;
            let col = pixel % (self.width - 2) as usize + 1;
            let index = (row * self.width as usize + col) * 4;
            bytes[index..index + 4].copy_from_slice(&color);
        }
        Pixels { width: self.width, height: self.height, bytes }
    }

    /// 目标轴仍使用实际卡片大小，平移或重着色不复制切片。
    pub fn slices(&self) -> (&[super::Slice], &[super::Slice]) { (&self.x, &self.y) }

    /// 回归与局部基准读取 Vec 实际容量，包含偏移、连段和映射。
    #[cfg(test)]
    pub(super) fn allocated_bytes(&self) -> usize {
        self.pixel_offsets.capacity() * std::mem::size_of::<usize>()
            + self.runs.capacity() * std::mem::size_of::<SampleRun>()
            + (self.x.capacity() + self.y.capacity()) * std::mem::size_of::<super::Slice>()
    }
}

/// 固定覆盖采样保留既有近似，不宣称与 WebKit 栅格器逐像素相同。
const SAMPLES: u32 = 8;

/// 圆角内框、高光和顶侧边互斥分类；不镜像不对称顶角，也不依赖颜色。
fn sample_kind(px: f64, py: f64, width: f64, height: f64, radius: f64, top: f64, side: f64) -> Option<SampleKind> {
    if !inside_outer(px, py, width, height, radius) { return None; }
    let padding = inside_padding(px, py, width, height, radius, top, side);
    if !padding {
        let at_top = top > 0.
            && py / top <= px / side
            && py / top <= (width - px) / side
            && py / top <= (height - py) / side;
        Some(if at_top { SampleKind::Top } else { SampleKind::Main })
    } else if !inside_padding(px, py - side, width, height, radius, top, side) {
        Some(SampleKind::Highlight)
    } else {
        None
    }
}

/// 必须逐次相加；计数乘法或按类别聚合会改变半字节处的舍入结果。
fn shade(runs: &[SampleRun], colors: &[[f64; 4]; 3]) -> [u8; 4] {
    let mut color = [0.; 4];
    for run in runs {
        let sample = colors[run.kind as usize];
        for _ in 0..run.repeats {
            for channel in 0..4 { color[channel] += sample[channel]; }
        }
    }
    let mut bytes = [0; 4];
    if color[3] > 0. {
        for channel in 0..3 {
            bytes[channel] = (color[channel] / color[3] * 255.).round().clamp(0., 255.) as u8;
        }
        bytes[3] = (color[3] / f64::from(SAMPLES * SAMPLES) * 255.).round().clamp(0., 255.) as u8;
    }
    bytes
}

/// 测试保留旧单次入口，生产 owner 直接复用计划。
#[cfg(test)]
pub(in crate::app::surface) fn render(width: u32, height: u32, scale: f64, radius: f64, top: f64, colors: RimColors) -> ShadowRaster {
    let geometry = RimGeometry::new(width, height, scale, radius, top);
    ShadowRaster { pixels: geometry.render(colors), x: geometry.x, y: geometry.y }
}

/// BGRA通道预乘，透明内高光与边框在互斥覆盖区合成一次。
fn channels(color: Rgba) -> [f64; 4] {
    let alpha = f64::from(color.a);
    [
        f64::from(color.b) * alpha,
        f64::from(color.g) * alpha,
        f64::from(color.r) * alpha,
        alpha,
    ]
}

/// 外边界使用完整卡片的圆角，不对源片或可见交集施加新圆角。
fn inside_outer(x: f64, y: f64, width: f64, height: f64, r: f64) -> bool {
    if x < 0. || y < 0. || x > width || y > height {
        return false;
    }
    let dx = (x - width / 2.).abs() - (width / 2. - r);
    let dy = (y - height / 2.).abs() - (height / 2. - r);
    dx.max(0.).hypot(dy.max(0.)) <= r
}

/// padding-box顶角为(r-side,r-top)椭圆，不能用max(widths)得到统一内圆角。
fn inside_padding(x: f64, y: f64, width: f64, height: f64, r: f64, top: f64, side: f64) -> bool {
    if x < side || x > width - side || y < top || y > height - side {
        return false;
    }
    let dx = if x < r {
        r - x
    } else if x > width - r {
        x - (width - r)
    } else {
        0.
    };
    let (dy, ry) = if y < r {
        (r - y, (r - top).max(0.))
    } else if y > height - r {
        (y - (height - r), (r - side).max(0.))
    } else {
        (0., 0.)
    };
    let rx = (r - side).max(0.);
    if dx == 0. || dy == 0. || rx == 0. || ry == 0. {
        true
    } else {
        (dx / rx).powi(2) + (dy / ry).powi(2) <= 1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 顶3px与侧1px的分界延伸进圆角，透明内区不被误涂为accent。
    #[test]
    fn unequal_top_border_preserves_corner_join_and_padding_ellipse() {
        assert!(inside_outer(2.5, 6.5, 100., 100., 12.));
        assert!(!inside_padding(2.5, 6.5, 100., 100., 12., 3., 1.));
        assert!(!inside_padding(1.5, 4.5, 100., 100., 12., 3., 1.));
        assert!(inside_padding(12.5, 3.5, 100., 100., 12., 3., 1.));
        let colors = RimColors {
            main: Rgba {
                r: 0.,
                g: 0.,
                b: 1.,
                a: 1.,
            },
            top: Rgba {
                r: 1.,
                g: 0.,
                b: 0.,
                a: 1.,
            },
            highlight: Rgba {
                r: 1.,
                g: 1.,
                b: 1.,
                a: 0.7,
            },
        };
        let image = render(1000, 500, 1., 12., 3., colors).pixels;
        assert!(image.width <= 31 && image.height <= 31);
        let pixel = |x: u32, y: u32| {
            &image.bytes
                [((y * image.width + x) * 4) as usize..((y * image.width + x) * 4 + 4) as usize]
        };
        assert_eq!(pixel(14, 2), [0, 0, 255, 255]);
        assert_eq!(pixel(1, 14), [255, 0, 0, 255]);
        assert_eq!(&pixel(14, 4)[..3], &[255, 255, 255]);
        assert!(
            (i16::from(pixel(14, 4)[3]) - 179).abs() <= 1,
            "f32 alpha在8bit半值处允许一次舍入"
        );
        assert_eq!(pixel(14, 14)[3], 0);
        assert_eq!(pixel(1, 1)[3], 0);
        assert!(
            image
                .bytes
                .chunks_exact(4)
                .any(|pixel| pixel[0] > 0 && pixel[1] == 0 && pixel[2] > 0 && pixel[3] > 0)
        );
    }
}

#[cfg(test)]
#[path = "rim_geometry_tests.rs"]
mod geometry_tests;
