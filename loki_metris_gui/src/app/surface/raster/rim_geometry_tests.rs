//! 独立冻结旧描边算法与切片，核对颜色重着色的完整像素和采样顺序。

use super::*;
use super::super::{ShadowRaster, Slice};

/// 完整直通像素和四个切片字段均按位相等，保护圈也参与比较。
fn assert_same(geometry: &RimGeometry, colors: RimColors, expected: ShadowRaster) {
    let actual = geometry.render(colors);
    assert_eq!((actual.width, actual.height), (expected.pixels.width, expected.pixels.height));
    assert_eq!(actual.bytes, expected.pixels.bytes);
    let bits = |axis: &[super::super::Slice]| axis.iter().map(|s| [
        s.source_start.to_bits(), s.source_end.to_bits(), s.target_start.to_bits(), s.target_end.to_bits(),
    ]).collect::<Vec<_>>();
    let (x, y) = geometry.slices();
    assert_eq!(bits(x), bits(&expected.x));
    assert_eq!(bits(y), bits(&expected.y));
}

/// 覆盖透明、主/顶/高光不同 alpha 和有效 f32 半字节反例颜色。
fn colors() -> [RimColors; 4] {
    let rgba = |r, g, b, a| Rgba { r, g, b, a };
    [
        RimColors { main: rgba(0., 0., 0., 0.), top: rgba(0., 0., 0., 0.), highlight: rgba(0., 0., 0., 0.) },
        RimColors { main: rgba(0.12, 0.34, 0.56, 1.), top: rgba(0.9, 0.2, 0.1, 0.75), highlight: rgba(1., 1., 1., 0.7) },
        RimColors { main: rgba(0.2, 0.18, 0.16, 0.63), top: rgba(0.84, 0.58, 0.17, 1.), highlight: rgba(1., 1., 1., 0.06) },
        RimColors { main: rgba(0., 0., f32::from_bits(0x3dcff032), f32::from_bits(0x3eeb7959)),
            top: rgba(0., 0., f32::from_bits(0x3f097d35), f32::from_bits(0x3eeb7959)), highlight: rgba(0., 0., 0., 0.) },
    ]
}

/// 短边半径收敛、椭圆顶角、粗顶边和切片阈值前后均使用冻结旧算法。
#[test]
fn geometry_matches_old_bgra_and_slice_bits_at_boundaries() {
    for scale in [1_f64, 1.125, 1.25, 1.5, 2.] {
        let cap = (20. * scale + 1.).ceil() as u32;
        for (width, height, radius, top) in [
            (0, 0, 20., 3.), (0, 50, 20., 3.), (50, 0, 20., 3.),
            (1, 1, 20., 3.), (1, 50, 20., 3.), (50, 1, 20., 3.),
            (25, 37, 20., 3.), (50, 50, 12., 1.), (76, 76, 0.25, 3.),
            (240, 100, 12., 20.), (1000, 500, -0., -0.),
            (cap * 2, cap * 2, 20., 3.), (cap * 2 + 1, cap * 2 + 1, 20., 3.),
            (cap * 2 + 2, cap * 2 + 2, 20., 3.), (20000, 10000, 20., 3.),
        ] {
            let geometry = RimGeometry::new(width, height, scale, radius, top);
            let capacity = geometry.allocated_bytes();
            for colors in colors() {
                assert_same(&geometry, colors, render_oracle(width, height, scale, radius, top, colors));
                assert_eq!(geometry.allocated_bytes(), capacity);
            }
        }
    }
}

/// 独立按旧分支展开贡献，省略 None 但不允许重排 top/main/highlight。
fn oracle_sequence(col: u32, row: u32, width: f64, height: f64, radius: f64, top: f64, side: f64) -> Vec<SampleKind> {
    let mut sequence = Vec::new();
    for sy in 0..8 {
        for sx in 0..8 {
            let px = f64::from(col) + (f64::from(sx) + 0.5) / 8.;
            let py = f64::from(row) + (f64::from(sy) + 0.5) / 8.;
            if !inside_outer(px, py, width, height, radius) { continue; }
            let padding = inside_padding(px, py, width, height, radius, top, side);
            let kind = if !padding {
                let at_top = top > 0. && py / top <= px / side
                    && py / top <= (width - px) / side && py / top <= (height - py) / side;
                if at_top { SampleKind::Top } else { SampleKind::Main }
            } else if !inside_padding(px, py - side, width, height, radius, top, side) {
                SampleKind::Highlight
            } else { continue; };
            sequence.push(kind);
        }
    }
    sequence
}

/// 每个 texel 的连段展开与旧采样顺序一致，包括整格 64 次和非对称上/下角。
#[test]
fn runs_preserve_every_contribution_in_original_order() {
    let mut has_64 = false;
    for (width, height, scale, radius, top) in [
        (1, 1, 1.125, 20., 3.), (25, 37, 1.25, 20., 3.),
        (1000, 500, 2., 20., 3.), (1000, 500, 1.5, 12., 1.),
        (1000, 500, 1., -0., -0.),
    ] {
        let geometry = RimGeometry::new(width, height, scale, radius, top);
        let radius = (radius * scale).min(f64::from(width.min(height)) / 2.);
        let (cw, ch) = (geometry.width - 2, geometry.height - 2);
        assert_eq!(geometry.pixel_offsets.len(), (cw * ch) as usize + 1);
        for (pixel, offsets) in geometry.pixel_offsets.windows(2).enumerate() {
            let runs = &geometry.runs[offsets[0]..offsets[1]];
            let actual: Vec<_> = runs.iter().flat_map(|run| {
                assert!(run.repeats > 0 && run.repeats <= 64);
                has_64 |= run.repeats == 64;
                std::iter::repeat_n(run.kind, run.repeats as usize)
            }).collect();
            let expected = oracle_sequence(pixel as u32 % cw, pixel as u32 / cw,
                f64::from(cw), f64::from(ch), radius, top * scale, scale);
            assert!(actual.len() <= 64);
            assert!(actual == expected, "sampling sequence at {pixel} drifted");
        }
    }
    assert!(has_64, "整格同类覆盖必须保留全部 64 次");
}

/// 有效 f32 颜色在半字节边界杀死每个连段改用计数乘法的错误优化。
#[test]
fn half_byte_counterexample_requires_repeated_addition_inside_each_run() {
    let geometry = RimGeometry::new(1000, 500, 2., 20., 3.);
    let colors = colors()[3];
    let image = geometry.render(colors);
    let offset = ((19 * image.width + 77) * 4) as usize;
    assert_eq!(&image.bytes[offset..offset + 4], &[127, 0, 0, 86]);
    assert_same(&geometry, colors, render_oracle(1000, 500, 2., 20., 3., colors));
    let pixel = (19 - 1) * (image.width - 2) + 77 - 1;
    let offsets = &geometry.pixel_offsets[pixel as usize..pixel as usize + 2];
    let runs = &geometry.runs[offsets[0]..offsets[1]];
    let actual: Vec<_> = runs.iter().map(|run| (run.kind as u8, run.repeats)).collect();
    assert_eq!(actual, [(1, 31), (0, 1), (1, 6), (0, 1), (1, 6), (0, 2)]);
    let channels = [channels(colors.main), channels(colors.top), channels(colors.highlight)];
    let mut mutant = [0.; 4];
    for run in runs {
        for channel in 0..4 { mutant[channel] += channels[run.kind as usize][channel] * f64::from(run.repeats); }
    }
    assert_eq!((mutant[0] / mutant[3] * 255.).round() as u8, 128);
}

/// alpha 舍入为零前仍写直通 RGB；三类互斥贡献也不应提前量化。
#[test]
fn sub_byte_highlight_keeps_rgb_when_final_alpha_rounds_to_zero() {
    let geometry = RimGeometry::new(1000, 500, 1., 12., 3.);
    let clear = Rgba { r: 0., g: 0., b: 0., a: 0. };
    let colors = RimColors { main: clear, top: clear, highlight: Rgba { r: 0., g: 1., b: 0., a: 1. / 512. } };
    let image = geometry.render(colors);
    let index = ((7 * image.width + 3) * 4) as usize;
    assert_eq!(&image.bytes[index..index + 4], &[0, 255, 0, 0]);
    assert_same(&geometry, colors, render_oracle(1000, 500, 1., 12., 3., colors));
    let colors = RimColors {
        main: Rgba { r: 0., g: 0., b: 1., a: 1. },
        top: Rgba { r: 1., g: 0., b: 0., a: 1. },
        highlight: Rgba { r: 0., g: 1., b: 0., a: 1. },
    };
    let image = geometry.render(colors);
    assert_eq!(&image.bytes[index..index + 4], &[36, 4, 215, 255]);
}

/// 长轴不会累计计划；实际容量包含预留，透明颜色首绘也不删除几何类别。
#[test]
fn geometry_capacity_is_bounded_and_recolor_does_not_grow_it() {
    for scale in [1., 1.125, 1.25, 1.5, 2.] {
        let geometry = RimGeometry::new(20000, 10000, scale, 20., 3.);
        let capacity = geometry.allocated_bytes();
        assert!(capacity < 80 * 1024, "{scale}x: {capacity} bytes");
        let same = RimGeometry::new(1000, 500, scale, 20., 3.);
        assert_eq!(same.allocated_bytes(), capacity);
        for colors in colors().into_iter().cycle().take(64) {
            assert_same(&geometry, colors, render_oracle(20000, 10000, scale, 20., 3., colors));
            assert_eq!(geometry.allocated_bytes(), capacity);
        }
    }
}

/// 每个角只需保存原半径范围；长边的恒定条带压缩到一物理像素。
fn render_oracle(
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
    let (cw, x) = oracle_slices(width, cap, cap, 0, 0);
    let (ch, y) = oracle_slices(height, cap, cap, 0, 0);
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

/// 以固定安全 cap 压缩长轴；短轴完整保留，不把重叠的 Gaussian 拉成常量。
fn oracle_slices(actual: u32, before: u32, after: u32, near: u32, far: u32) -> (u32, Vec<Slice>) {
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
