//! 原样旧算法作为独立像素基准，覆盖连续参数及紧凑缓存边界。

use super::*;

/// 比较完整直通像素和映射位值，透明 RGB 或抗锯齿半字节也不能漂移。
fn assert_same(actual: &Pixels, x: &[Slice], y: &[Slice], expected: ShadowRaster) {
    assert_eq!((actual.width, actual.height), (expected.pixels.width, expected.pixels.height));
    assert_eq!(actual.bytes, expected.pixels.bytes);
    let bits = |axis: &[Slice]| axis.iter().map(|s| [
        s.source_start.to_bits(), s.source_end.to_bits(),
        s.target_start.to_bits(), s.target_end.to_bits(),
    ]).collect::<Vec<_>>();
    assert_eq!(bits(x), bits(&expected.x));
    assert_eq!(bits(y), bits(&expected.y));
}

/// 旧覆盖率和切片也独立冻结；仅未改动的 Gaussian CDF 共用原函数。
#[test]
fn compact_shadow_matches_old_pixels_at_scale_radius_and_slice_boundaries() {
    for scale in [1., 1.125, 1.25, 1.5, 2.] {
        let cap = |value: f64| (value * scale).ceil() as u32;
        let (x_cap, y_cap) = (cap(126.) * 2 + 1, cap(146.) + cap(106.) + 1);
        for (width, height, radius) in [
            (0, 0, -0.), (1, 1, 20.), (25, 37, 20.), (50, 50, 12.),
            (76, 76, 0.25), (240, 100, 12.), (x_cap - 1, y_cap - 1, 0.),
            (x_cap, y_cap, 20.), (x_cap + 1, y_cap + 1, 19.999),
            (20000, 10000, 20.),
        ] {
            let geometry = ShadowGeometry::new(width, height, scale, radius);
            let (x, y) = geometry.slices();
            for dark in [false, true] {
                for progress in [0., 0.267, 0.5, 0.997, 1.] {
                    let sample = ShadowSample::new(dark, progress, scale);
                    assert_same(&geometry.render(sample), x, y,
                        shadow_oracle(width, height, scale, radius, sample));
                }
            }
        }
    }
}

/// 内插样本的颜色独立改变，挖空透明区仍写 RGB，零投影也必须保留旧字节。
#[test]
fn repeated_recolor_keeps_transparent_rgb_and_does_not_grow_geometry() {
    let geometry = ShadowGeometry::new(43, 29, 1.5, 20.);
    let allocated = geometry.allocated_bytes();
    let (x, y) = geometry.slices();
    for step in 0..64 {
        let sample = ShadowSample {
            bgra: [step, 127, 255 - step],
            ..ShadowSample::new(step % 2 == 0, f32::from(step) / 63., 1.5)
        };
        assert_same(&geometry.render(sample), x, y, shadow_oracle(43, 29, 1.5, 20., sample));
        assert_eq!(geometry.allocated_bytes(), allocated);
    }
}

/// 真正主题时间线及80ms反向保留混合颜色、Gaussian参数与结束端点的完整像素。
#[test]
fn theme_transition_and_reversal_match_old_pixels() {
    use crate::app::surface::palette::MaterialMotion;
    use std::time::{Duration, Instant};

    let now = Instant::now();
    for scale in [1., 1.125, 1.5, 2.] {
        let geometry = ShadowGeometry::new(480, 360, scale, 20.);
        let (x, y) = geometry.slices();
        for initial_dark in [false, true] {
            for reversing in [false, true] {
                let mut motion = MaterialMotion::new(now);
                motion.sample_static(initial_dark, true, now);
                for at in [0, 1, 16, 79, 80, 81, 99, 100, 116, 299, 300, 399, 400] {
                    let dark = if reversing && (80..100).contains(&at) {
                        initial_dark
                    } else { !initial_dark };
                    let frame = motion.sample_static(dark, true,
                        now + Duration::from_millis(at));
                    if at == 400 || (!reversing && at == 300) { assert!(!frame.running); }
                    let sample = frame.shadow.scaled(scale);
                    assert_same(&geometry.render(sample), x, y,
                        shadow_oracle(480, 360, scale, 20., sample));
                }
            }
        }
    }
}

/// 零透明度和空投影均保留内部非零RGB，四边保护圈保持全零。
#[test]
fn zero_alpha_and_empty_projection_keep_rgb_and_clear_border() {
    for (width, height) in [(1, 1), (43, 29), (480, 360)] {
        let geometry = ShadowGeometry::new(width, height, 1.5, 20.);
        let (x, y) = geometry.slices();
        for alpha in [0., -0.] {
            let sample = ShadowSample { alpha, bgra: [19, 87, 243],
                ..ShadowSample::new(false, 0.5, 1.5) };
            let pixels = geometry.render(sample);
            assert_same(&pixels, x, y, shadow_oracle(width, height, 1.5, 20., sample));
            for (row, bytes) in pixels.bytes.chunks_exact(pixels.width as usize * 4).enumerate() {
                for (column, pixel) in bytes.chunks_exact(4).enumerate() {
                    let border = row == 0 || row == pixels.height as usize - 1
                        || column == 0 || column == pixels.width as usize - 1;
                    assert_eq!(pixel, if border { &[0, 0, 0, 0] } else { &[19, 87, 243, 0] });
                }
            }
        }
    }
}

/// 用实际 Vec 容量核验常驻计划，避免把大 f64 遮罩换成隐藏的同量级预留。
#[test]
fn resident_geometry_stays_compact_for_large_cards() {
    for scale in [1., 1.125, 1.25, 1.5, 2.] {
        let geometry = ShadowGeometry::new(20000, 10000, scale, 20.);
        let image = geometry.render(ShadowSample::new(false, 0.5, scale));
        let allocated = geometry.allocated_bytes();
        assert!(allocated < 80 * 1024, "{scale}x geometry capacity: {allocated} bytes");
        assert!(allocated * 16 < image.bytes.len(), "计划不应接近像素表大小");
    }
}

/// 创建至多约405×405 logical px的阴影；空投影不留下内部黑色。
fn shadow_oracle(
    width: u32,
    height: u32,
    scale: f64,
    radius: f64,
    sample: ShadowSample,
) -> ShadowRaster {
    let cap = |value: f64| (value * scale).ceil() as u32;
    let (near_x, near_y) = (cap(76.), cap(52.));
    let (card_w, x) = oracle_slices(width, cap(126.), cap(126.), near_x, cap(76.));
    let (card_h, y) = oracle_slices(height, cap(146.), cap(106.), near_y, cap(100.));
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
        (0. ..=20.).contains(&radius),
        "品牌材质半径必须处于当前产品的0..20px范围"
    );
    let radius = (radius * scale).min(f64::from(card_w.min(card_h)) / 2.);
    for row in 1..h - 1 {
        for column in 1..w - 1 {
            let px = f64::from(column) - f64::from(near_x) - 0.5;
            let py = f64::from(row) - f64::from(near_y) - 0.5;
            let outside =
                1. - oracle_rounded_coverage(px, py, f64::from(card_w), f64::from(card_h), radius);
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


/// 冻结旧九切片映射，独立核对源轴与目标轴的完整位值。
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


/// 冻结旧圆角距离函数，防止覆盖率量化被最终 alpha 舍入掩盖。
pub(super) fn oracle_rounded_coverage(x: f64, y: f64, width: f64, height: f64, radius: f64) -> f64 {
    let qx = (x - width / 2.).abs() - width / 2. + radius;
    let qy = (y - height / 2.).abs() - height / 2. + radius;
    let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius;
    (0.5 - distance).clamp(0., 1.)
}

