//! 数值回归验证固定切片的实际覆盖范围、短轴重叠与原坐标圆角。

use super::*;

/// 采样纹理像素中心，对应paint_image在物理像素对齐时的原始值。
fn alpha(image: &Pixels, x: u32, y: u32) -> u8 {
    image.bytes[((y * image.width + x) * 4 + 3) as usize]
}

/// 中心压缩只发生在Gaussian已到常量的区域，长卡片不会扩大纹理分配。
#[test]
fn shadow_allocation_is_bounded_on_each_axis() {
    for scale in [1., 1.5, 2.] {
        let large = shadow(
            20000,
            10000,
            scale,
            20.,
            ShadowSample::new(false, 0.5, scale),
        );
        assert!(large.pixels.width <= (405. * scale).ceil() as u32 + 2);
        assert!(large.pixels.height <= (405. * scale).ceil() as u32 + 2);
        assert_eq!((large.x.len(), large.y.len()), (3, 3));
        for axis in [&large.x, &large.y] {
            for adjacent in axis.windows(2) {
                assert_eq!(adjacent[0].target_end, adjacent[1].target_start);
                assert_eq!(adjacent[0].source_end, adjacent[1].source_start);
            }
            assert_eq!(axis[1].source_end - axis[1].source_start, 1.);
        }
    }
}

/// 短卡片保留两端Gaussian重叠；负spread使投影消失时必须全透明。
#[test]
fn short_cards_keep_overlap_and_empty_cast_is_transparent() {
    let sample = ShadowSample::new(true, 0., 1.);
    let card = shadow(240, 100, 1., 20., sample);
    assert_eq!(card.y.len(), 1);
    let x = 120 + 76 + 1;
    let y = 100 + 52 + 8;
    let position_x = f64::from(x) - 76. - 0.5;
    let position_y = f64::from(y) - 52. - 0.5;
    let expected = sample.alpha
        * (cdf((position_x - 38.) / 21.) - cdf((position_x - 202.) / 21.))
        * (cdf((position_y - 54.) / 21.) - cdf((position_y - 78.) / 21.));
    assert_eq!(alpha(&card.pixels, x, y), (expected * 255.).round() as u8);
    let empty = shadow(50, 50, 1., 20., sample);
    assert!(
        empty
            .pixels
            .bytes
            .as_chunks::<4>().0.iter()
            .all(|pixel| pixel[3] == 0)
    );
}

/// 外阴影从原border-box挖空，半透明surface之下不出现内侧暗边。
#[test]
fn shadow_knocks_out_original_card_and_uses_straight_bgra() {
    let card = shadow(400, 300, 1., 20., ShadowSample::new(false, 1., 1.));
    for (x, y) in [(100, 100), (200, 70), (80, 200)] {
        assert_eq!(alpha(&card.pixels, x + 77, y + 53), 0);
    }
    assert!(card.pixels.bytes.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
    for pixel in card
        .pixels
        .bytes
        .as_chunks::<4>().0.iter()
        .filter(|pixel| pixel[3] > 0)
    {
        assert_eq!(&pixel[..3], &[36, 63, 91]);
    }
    assert!((0..card.pixels.width).all(|x| alpha(&card.pixels, x, 0) == 0));
    assert!((0..card.pixels.height).all(|y| alpha(&card.pixels, 0, y) == 0));
}

/// 有界Gaussian保持对称及单调，4σ端点准确到0和1，积分不含平台数学依赖。
#[test]
fn gaussian_cdf_is_monotonic_and_symmetric() {
    assert_eq!(cdf(-4.), 0.);
    assert_eq!(cdf(4.), 1.);
    assert!((cdf(0.) - 0.5).abs() < 1e-12);
    let mut previous = 0.;
    for i in 0..=800 {
        let z = -4. + f64::from(i) / 100.;
        let value = cdf(z);
        assert!(value >= previous);
        assert!((value + cdf(-z) - 1.).abs() < 1e-12);
        previous = value;
    }
    assert!((cdf(1.) - 0.841366888).abs() < 0.000001);
}

/// 光斑固定196px透明半径和8%中心，主题只改变直通颜色而不改变几何。
#[test]
fn radial_stops_and_transparent_guard_are_stable() {
    assert_eq!(radial_alpha(0., 0., 1.), 0.08);
    assert_eq!(radial_alpha(98., 0., 1.), 0.04);
    assert_eq!(radial_alpha(196., 0., 1.), 0.);
    assert_eq!(radial_alpha(392., 0., 2.), 0.);
    let image = radial(2., [0x8a, 0xa0, 0xf2]);
    assert_eq!((image.width, image.height), (786, 786));
    assert_eq!(alpha(&image, 392, 392), 20);
    assert!((0..image.width).all(|x| alpha(&image, x, 0) == 0));
    let center = ((392 * image.width + 392) * 4) as usize;
    assert_eq!(&image.bytes[center..center + 3], &[0x8a, 0xa0, 0xf2]);
}

/// 圆角以完整卡片为坐标系：裁掉左边缘不能凭空给交集加新圆角。
#[test]
fn rounded_mask_preserves_original_corner_coordinates() {
    assert_eq!(rounded_coverage(0.5, 0.5, 500., 200., 20.), 0.);
    assert_eq!(rounded_coverage(20.5, 0.5, 500., 200., 20.), 1.);
    assert_eq!(rounded_coverage(0.5, 40.5, 500., 200., 20.), 1.);
    assert_eq!(rounded_coverage(100.5, 0.5, 500., 200., 20.), 1.);
    for x in 0..20 {
        for y in 0..20 {
            assert_eq!(
                rounded_coverage(x as f64 + 0.5, y as f64 + 0.5, 500., 200., 20.),
                rounded_coverage(499.5 - x as f64, 199.5 - y as f64, 500., 200., 20.)
            );
        }
    }
}

/// r12挖空使用自身圆角；r20的角部投影不能在较小圆角卡片的半透明底下泄漏。
#[test]
fn variable_radius_changes_knockout_without_changing_outer_gaussian() {
    let sample = ShadowSample::new(false, 1., 1.);
    let medium = shadow(240, 180, 1., 12., sample);
    let large = shadow(240, 180, 1., 20., sample);
    assert_eq!(alpha(&medium.pixels, 82, 58), 0);
    assert!(alpha(&large.pixels, 82, 58) > 0);
    assert_eq!(
        alpha(&medium.pixels, 120 + 77, 180 + 53 + 10),
        alpha(&large.pixels, 120 + 77, 180 + 53 + 10)
    );
}
