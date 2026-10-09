//! 不等宽CSS边框保留椭圆内角和3:1角部分界，小纹理切片也包含顶部内高光。

use super::{Pixels, ShadowRaster, slices};
use gpui_kit::Rgba;

/// 直通sRGB颜色；像素覆盖期间先预乘合成，再恢复直通BGRA交给GPUI。
#[derive(Clone, Copy, PartialEq)]
pub(in crate::app::surface) struct RimColors {
    pub main: Rgba,
    pub top: Rgba,
    pub highlight: Rgba,
}

/// 每个角只需保存原半径范围；长边的恒定条带压缩到一物理像素。
pub(in crate::app::surface) fn render(
    width: u32,
    height: u32,
    scale: f64,
    radius: f64,
    top: f64,
    colors: RimColors,
) -> ShadowRaster {
    let radius = (radius * scale).min(f64::from(width.min(height)) / 2.);
    let top = top * scale;
    let cap = (radius.max(top).max(scale) + 1.).ceil() as u32;
    let (cw, x) = slices(width, cap, cap, 0, 0);
    let (ch, y) = slices(height, cap, cap, 0, 0);
    let (w, h) = (cw + 2, ch + 2);
    let mut bytes = vec![0; w as usize * h as usize * 4];
    let main = channels(colors.main);
    let top_color = channels(colors.top);
    let highlight = channels(colors.highlight);
    // 固定8×8覆盖采样只在≤约45px的角图中执行，非WebKit光栅器的逐像素等价声明。
    const SAMPLES: u32 = 8;
    for row in 1..h - 1 {
        for col in 1..w - 1 {
            let mut color = [0.; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = f64::from(col - 1) + (f64::from(sx) + 0.5) / f64::from(SAMPLES);
                    let py = f64::from(row - 1) + (f64::from(sy) + 0.5) / f64::from(SAMPLES);
                    if !inside_outer(px, py, f64::from(cw), f64::from(ch), radius) {
                        continue;
                    }
                    let padding =
                        inside_padding(px, py, f64::from(cw), f64::from(ch), radius, top, scale);
                    let sample = if !padding {
                        let at_top = top > 0.
                            && py / top <= px / scale
                            && py / top <= (f64::from(cw) - px) / scale
                            && py / top <= (f64::from(ch) - py) / scale;
                        if at_top { top_color } else { main }
                    } else if !inside_padding(
                        px,
                        py - scale,
                        f64::from(cw),
                        f64::from(ch),
                        radius,
                        top,
                        scale,
                    ) {
                        highlight
                    } else {
                        continue;
                    };
                    for channel in 0..4 {
                        color[channel] += sample[channel];
                    }
                }
            }
            let index = (row as usize * w as usize + col as usize) * 4;
            if color[3] > 0. {
                for channel in 0..3 {
                    bytes[index + channel] =
                        (color[channel] / color[3] * 255.).round().clamp(0., 255.) as u8;
                }
                bytes[index + 3] = (color[3] / f64::from(SAMPLES * SAMPLES) * 255.)
                    .round()
                    .clamp(0., 255.) as u8;
            }
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
