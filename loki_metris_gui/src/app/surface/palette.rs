//! 每个CSS属性只因自身目标改变而重定向，主题与hover使用同一历史过渡合同。

use super::{motion::CssMotion, raster::ShadowSample};
use gpui_kit::{Hsla, Rgba, rgb};
use std::time::Instant;

/// 主边框、顶accent、surface和两层阴影分别拥有时间线，颜色存储预乘sRGB。
pub(super) struct MaterialMotion {
    surface: CssMotion<4>,
    border: CssMotion<4>,
    top: CssMotion<4>,
    highlight: CssMotion<4>,
    shadow: CssMotion<7>,
}

/// 单帧完整材质参数；阴影sigma、offset与spread仍是逻辑像素。
pub(super) struct MaterialSample {
    pub surface: Hsla,
    pub border: Hsla,
    pub top: Hsla,
    pub highlight: Hsla,
    pub shadow: ShadowSample,
    pub running: bool,
}

impl MaterialMotion {
    /// 首绘目标即真实主题，不额外播放挂载动画。
    pub fn new(now: Instant) -> Self {
        Self {
            surface: CssMotion::new(now),
            border: CssMotion::new(now),
            top: CssMotion::new(now),
            highlight: CssMotion::new(now),
            shadow: CssMotion::new(now),
        }
    }

    /// 先设定各属性最终CSS值再采样；top accent只在非hover状态保留。
    pub fn sample(
        &mut self,
        dark: bool,
        hovered: bool,
        top_accent: Option<Hsla>,
        now: Instant,
    ) -> MaterialSample {
        self.sample_material(dark, hovered, top_accent, now, true)
    }

    /// 静态类没有hover目标；只有table-panel声明了跨主题300ms过渡。
    pub fn sample_static(&mut self, dark: bool, transition: bool, now: Instant) -> MaterialSample {
        self.sample_material(dark, false, None, now, transition)
    }

    /// 同一组CSS端点由显式过渡合同推进，不复制静态颜色或阴影公式。
    fn sample_material(
        &mut self,
        dark: bool,
        hovered: bool,
        top_accent: Option<Hsla>,
        now: Instant,
        transition: bool,
    ) -> MaterialSample {
        let base = rgba(if dark { 0x332f2a } else { 0xe6ddd1 }, 1.);
        let accent = rgba(if dark { 0xf2a08a } else { 0xa52f25 }, 1.);
        let border = if hovered {
            mix(base, accent, 0.30)
        } else {
            base
        };
        set(&mut self.border, border, now, transition);
        set(
            &mut self.top,
            if hovered {
                border
            } else {
                top_accent.map(premultiply).unwrap_or(base)
            },
            now,
            transition,
        );
        set(
            &mut self.surface,
            rgba(
                if dark { 0x1e1c1a } else { 0xfffdf9 },
                if dark { 0.92 } else { 0.90 },
            ),
            now,
            transition,
        );
        set(
            &mut self.highlight,
            rgba(0xffffff, if dark { 0.06 } else { 0.7 }),
            now,
            transition,
        );
        let shadow = ShadowSample::new(dark, f32::from(hovered), 1.);
        let color = rgba(if dark { 0 } else { 0x5b3f24 }, shadow.alpha);
        set(
            &mut self.shadow,
            [
                shadow.sigma,
                shadow.spread,
                shadow.dy,
                color[0],
                color[1],
                color[2],
                color[3],
            ],
            now,
            transition,
        );
        let (surface, s0) = self.surface.sample(now);
        let (border, s1) = self.border.sample(now);
        let (top, s2) = self.top.sample(now);
        let (highlight, s3) = self.highlight.sample(now);
        let (shadow, s4) = self.shadow.sample(now);
        let color = unpremultiply([shadow[3], shadow[4], shadow[5], shadow[6]]);
        MaterialSample {
            surface: unpremultiply(surface).into(),
            border: unpremultiply(border).into(),
            top: unpremultiply(top).into(),
            highlight: unpremultiply(highlight).into(),
            shadow: ShadowSample {
                sigma: shadow[0],
                spread: shadow[1],
                dy: shadow[2],
                alpha: shadow[6],
                bgra: [
                    (color.b * 255.).round() as u8,
                    (color.g * 255.).round() as u8,
                    (color.r * 255.).round() as u8,
                ],
            },
            running: s0 || s1 || s2 || s3 || s4,
        }
    }
}

/// CSS声明决定立即替换或连续动画；即时模式不会保留隐藏的未完成时间线。
fn set<const N: usize>(
    motion: &mut CssMotion<N>,
    target: [f64; N],
    now: Instant,
    transition: bool,
) {
    if transition {
        motion.set_target(target, now);
    } else {
        motion.jump_to(target, now);
    }
}

/// CSS颜色过渡先预乘alpha再插值，透明度不同的跨主题颜色不会偏暗。
fn premultiply(color: Hsla) -> [f64; 4] {
    let color: Rgba = color.into();
    let alpha = f64::from(color.a);
    [
        f64::from(color.r) * alpha,
        f64::from(color.g) * alpha,
        f64::from(color.b) * alpha,
        alpha,
    ]
}

/// 精确主题端点由本地CSS数值生成，不依赖控件primary的不同品牌色。
fn rgba(value: u32, alpha: f64) -> [f64; 4] {
    let mut color: Hsla = rgb(value).into();
    color.a = alpha as f32;
    premultiply(color)
}

/// 完全透明颜色约定为透明黑；非透明值恢复直通RGBA供GPUI绘制。
pub(super) fn unpremultiply(color: [f64; 4]) -> Rgba {
    let alpha = color[3];
    let channel = |i: usize| {
        if alpha > 0. {
            (color[i] / alpha) as f32
        } else {
            0.
        }
    };
    Rgba {
        r: channel(0),
        g: channel(1),
        b: channel(2),
        a: alpha as f32,
    }
}

/// 历史tbody行只混入6%品牌accent；透明起点与颜色保持预乘sRGB插值。
pub(super) fn row_hover(dark: bool) -> [f64; 4] {
    rgba(if dark { 0xf2a08a } else { 0xa52f25 }, 0.06)
}

/// 目标color-mix在sRGB中计算，主边框hover只包含30%accent。
fn mix(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// hover过程中切换主题仍从当前阴影参数继续，背景独立保留其未变化目标的时钟。
    #[test]
    fn theme_and_hover_retarget_continuously_without_restarting_surface() {
        let now = Instant::now();
        let mut motion = MaterialMotion::new(now);
        let initial = motion.sample(false, false, None, now);
        assert!(!initial.running);
        let begin = motion.sample(true, false, None, now);
        assert_eq!(begin.surface, initial.surface);
        let turn = now + Duration::from_millis(80);
        let before = motion.sample(true, false, None, turn);
        let after = motion.sample(true, true, None, turn);
        assert_eq!(before.surface, after.surface);
        assert_eq!(before.shadow.sigma, after.shadow.sigma);
        assert_eq!(before.shadow.alpha, after.shadow.alpha);
        assert!(after.running);
        let end = motion.sample(true, true, None, now + Duration::from_millis(300));
        let expected = unpremultiply(rgba(0x1e1c1a, 0.92));
        assert_eq!(Rgba::from(end.surface), expected);
        let end = motion.sample(true, true, None, turn + Duration::from_millis(300));
        assert_eq!(
            (end.shadow.sigma, end.shadow.spread, end.shadow.dy),
            (24., -30., 20.)
        );
        assert_eq!(end.shadow.bgra, [0, 0, 0]);
        assert!(!end.running);
    }

    /// 顶accent在hover目标到达后完全采用主边框，退出时恢复accent且首帧不跳变。
    #[test]
    fn top_accent_uses_same_css_border_transition_and_reverses() {
        let now = Instant::now();
        let mut motion = MaterialMotion::new(now);
        let accent: Hsla = rgb(0xd7952c).into();
        let base = motion.sample(false, false, Some(accent), now);
        assert_eq!(base.top, accent);
        let start = motion.sample(false, true, Some(accent), now);
        assert_eq!(start.top, accent);
        let turn = now + Duration::from_millis(80);
        let before = motion.sample(false, true, Some(accent), turn);
        let after = motion.sample(false, false, Some(accent), turn);
        assert_eq!(before.top, after.top);
        let end = motion.sample(
            false,
            false,
            Some(accent),
            turn + Duration::from_millis(300),
        );
        assert_eq!(end.top, accent);
        motion.sample(false, true, Some(accent), turn + Duration::from_millis(300));
        let end = motion.sample(false, true, Some(accent), turn + Duration::from_millis(600));
        assert_eq!(end.top, end.border);
    }
}
