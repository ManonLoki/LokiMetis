//! 七种实际历史按钮变体共用颜色解析与预乘插值。

use super::SurfaceButtonVariant;
use gpui_kit::Rgba;

/// 复刻历史AppThemeProvider与其实际Mantine 9.6解析；light混色值未传scheme，亮暗均取色阶6。
pub(in crate::app) fn colors(
    variant: SurfaceButtonVariant,
    dark: bool,
    disabled: bool,
    hovered: bool,
) -> (Rgba, Rgba, Rgba) {
    let rgb = |hex| Rgba::from(gpui_kit::rgb(hex));
    let transparent = rgb(0).opacity(0.);
    if disabled {
        return (
            rgb(if dark { 0x2e2e2e } else { 0xe9ecef }),
            transparent,
            rgb(if dark { 0x696969 } else { 0xadb5bd }),
        );
    }
    let red = matches!(
        variant,
        SurfaceButtonVariant::FilledRed
            | SurfaceButtonVariant::LightRed
            | SurfaceButtonVariant::SubtleRed
    );
    let color = rgb(if red { 0xfa5252 } else { 0xc43b2d });
    let foreground = rgb(match (dark, red) {
        (false, false) => 0x6b1e18,
        (true, false) => 0xfff2ee,
        (false, true) => 0xc92a2a,
        (true, true) => 0xfff5f5,
    });
    match variant {
        SurfaceButtonVariant::FilledBrand | SurfaceButtonVariant::FilledRed => (
            rgb(match (red, dark, hovered) {
                (false, false, false) | (false, true, true) => 0xc43b2d,
                (false, false, true) => 0xa52f25,
                (false, true, false) => 0xd95a44,
                (true, false, false) | (true, true, true) => 0xfa5252,
                (true, false, true) => 0xf03e3e,
                (true, true, false) => 0xff6b6b,
            }),
            transparent,
            rgb(0xffffff),
        ),
        SurfaceButtonVariant::LightBrand | SurfaceButtonVariant::LightRed => (
            color.opacity(if hovered { 0.15 } else { 0.09 }),
            color.opacity(0.26),
            foreground,
        ),
        SurfaceButtonVariant::SubtleBrand | SurfaceButtonVariant::SubtleRed => (
            if !hovered {
                transparent
            } else if !dark {
                rgb(if red { 0xffc9c9 } else { 0xf8c2b1 })
            } else {
                let base = rgb(if red { 0xc92a2a } else { 0x6b1e18 });
                Rgba {
                    r: (base.r * 255. * 0.7).round() / 255.,
                    g: (base.g * 255. * 0.7).round() / 255.,
                    b: (base.b * 255. * 0.7).round() / 255.,
                    a: 1.,
                }
            },
            transparent,
            foreground,
        ),
        SurfaceButtonVariant::Default => (
            rgb(match (dark, hovered) {
                (false, false) => 0xffffff,
                (false, true) => 0xf8f9fa,
                (true, false) => 0x2e2e2e,
                (true, true) => 0x3b3b3b,
            }),
            rgb(if dark { 0x424242 } else { 0xced4da }),
            rgb(if dark { 0xffffff } else { 0x000000 }),
        ),
    }
}

/// 透明颜色按预乘sRGB插值，避免透明端点出现深色晕染。
pub(super) fn premultiply(color: Rgba) -> [f64; 4] {
    [
        f64::from(color.r * color.a),
        f64::from(color.g * color.a),
        f64::from(color.b * color.a),
        f64::from(color.a),
    ]
}
/// GPUI quad接收非预乘颜色，零alpha端点保持精确透明。
pub(super) fn unpremultiply(color: [f64; 4]) -> Rgba {
    if color[3] <= f64::EPSILON {
        return Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        };
    }
    Rgba {
        r: (color[0] / color[3]) as f32,
        g: (color[1] / color[3]) as f32,
        b: (color[2] / color[3]) as f32,
        a: color[3] as f32,
    }
}
