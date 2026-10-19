//! 冻结原端点生成与时间线调用，独立比对完整材质样本位值。

use super::super::{motion::CssMotion, raster::ShadowSample};
use gpui_kit::{Hsla, Rgba, rgb};
use std::time::Instant;

/// 主边框、顶accent、surface和两层阴影分别拥有时间线，颜色存储预乘sRGB。
struct OriginalMotion {
    /// 表面色过渡。
    surface: CssMotion<4>,
    /// 主边框色过渡。
    border: CssMotion<4>,
    /// 顶边色过渡。
    top: CssMotion<4>,
    /// 内高光色过渡。
    highlight: CssMotion<4>,
    /// 阴影参数过渡。
    shadow: CssMotion<7>,
}

/// 单帧完整材质参数；阴影sigma、offset与spread仍是逻辑像素。
struct MaterialSample {
    /// 表面色。
    pub surface: Hsla,
    /// 主边框色。
    pub border: Hsla,
    /// 顶边色。
    pub top: Hsla,
    /// 内高光色。
    pub highlight: Hsla,
    /// 阴影参数。
    pub shadow: ShadowSample,
    /// 过渡仍在进行。
    pub running: bool,
}

impl OriginalMotion {
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
fn unpremultiply(color: [f64; 4]) -> Rgba {
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

/// 目标color-mix在sRGB中计算，主边框hover只包含30%accent。
fn mix(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

use std::time::Duration;

/// 所有颜色、阴影参数与 running 都比较位值，包括 NaN 与带符号零。
fn assert_sample(actual: super::MaterialSample, expected: MaterialSample) {
    let colors = |items: [Hsla; 4]| items.map(|c| [c.h, c.s, c.l, c.a].map(f32::to_bits));
    assert_eq!(
        colors([actual.surface, actual.border, actual.top, actual.highlight]),
        colors([
            expected.surface,
            expected.border,
            expected.top,
            expected.highlight
        ])
    );
    let shadow = |s: ShadowSample| ([s.sigma, s.spread, s.dy, s.alpha].map(f64::to_bits), s.bgra);
    assert_eq!(shadow(actual.shadow), shadow(expected.shadow));
    assert_eq!(actual.running, expected.running);
}

/// 明暗、hover、反向、accent 和即时合同变化均沿旧端点与独立时间线推进。
#[test]
fn reused_targets_match_original_timeline_and_every_sample_bit() {
    let now = Instant::now();
    let gold: Hsla = rgb(0xd7952c).into();
    let blue: Hsla = rgb(0x2c7be5).into();
    let mut actual = super::MaterialMotion::new(now);
    let mut expected = OriginalMotion::new(now);
    for (at, dark, hovered, accent, transition) in [
        (0, false, false, None, true),
        (16, true, false, None, true),
        (32, true, false, None, true),
        (80, true, true, Some(gold), true),
        (96, true, true, Some(gold), true),
        (100, false, false, Some(gold), true),
        (116, false, false, Some(gold), true),
        (121, false, false, Some(blue), true),
        (137, false, false, Some(blue), true),
        (150, true, false, None, false),
        (151, true, false, None, false),
        (160, true, false, None, true),
        (170, false, false, None, true),
        (186, false, false, None, true),
        (240, false, false, None, true),
        (469, false, false, None, true),
        (470, false, false, None, true),
        (1000, false, false, None, true),
    ] {
        let at = now + Duration::from_millis(at);
        assert_sample(
            actual.sample_material(dark, hovered, accent, at, transition),
            expected.sample_material(dark, hovered, accent, at, transition),
        );
    }
}

/// 静止重绘和动画帧不重算端点；过渡开关仍逐帧执行原 set/jump 合同。
#[test]
fn only_color_inputs_rebuild_targets_and_transition_remains_live() {
    let now = Instant::now();
    let mut actual = super::MaterialMotion::new(now);
    let mut expected = OriginalMotion::new(now);
    for at in [0, 1, 16, 80, 300, 1000] {
        let at = now + Duration::from_millis(at);
        assert_sample(
            actual.sample_static(false, true, at),
            expected.sample_static(false, true, at),
        );
    }
    assert_eq!(actual.target_builds, 1);
    for at in [1001, 1017, 1081] {
        let at = now + Duration::from_millis(at);
        assert_sample(
            actual.sample_static(true, true, at),
            expected.sample_static(true, true, at),
        );
    }
    assert_eq!(actual.target_builds, 2);
    for (at, transition) in [(1082, false), (1083, false), (1084, true), (1100, true)] {
        let at = now + Duration::from_millis(at);
        let a = actual.sample_static(true, transition, at);
        assert!(!a.running);
        assert_sample(a, expected.sample_static(true, transition, at));
    }
    assert_eq!(actual.target_builds, 2);
    let at = now + Duration::from_millis(1101);
    let a = actual.sample_static(false, true, at);
    assert!(a.running);
    assert_sample(a, expected.sample_static(false, true, at));
    assert_eq!(actual.target_builds, 3);
    for frame in 1..=100 {
        let at = at + Duration::from_millis(frame * 16);
        assert_sample(
            actual.sample_static(false, true, at),
            expected.sample_static(false, true, at),
        );
    }
    assert_eq!(actual.target_builds, 3);
}

/// 相同异常颜色仍每帧 set_target；完整相等的输入键不能替代数组的普通浮点比较。
#[test]
fn cached_nonfinite_targets_preserve_original_retarget_and_running() {
    let now = Instant::now();
    let base: Hsla = rgb(0xd7952c).into();
    for accent in [
        Hsla {
            a: f32::from_bits(0x7fc01234),
            ..base
        },
        Hsla {
            h: f32::MAX,
            s: 1.,
            l: 0.5,
            a: 1.,
        },
        Hsla {
            h: f32::INFINITY,
            ..base
        },
        Hsla {
            s: f32::NEG_INFINITY,
            ..base
        },
        Hsla {
            l: f32::from_bits(0xffc01234),
            ..base
        },
    ] {
        let mut actual = super::MaterialMotion::new(now);
        let mut expected = OriginalMotion::new(now);
        for (at, transition) in [
            (0, true),
            (16, true),
            (316, true),
            (317, false),
            (318, false),
            (319, true),
            (635, true),
        ] {
            let at = now + Duration::from_millis(at);
            assert_sample(
                actual.sample_material(false, false, Some(accent), at, transition),
                expected.sample_material(false, false, Some(accent), at, transition),
            );
        }
        assert_eq!(actual.target_builds, 1);
    }
    let accent = Hsla {
        h: f32::MAX,
        s: 1.,
        l: 0.5,
        a: 1.,
    };
    let mut actual = super::MaterialMotion::new(now);
    assert!(!actual.sample(false, false, Some(accent), now).running);
    assert!(
        actual
            .sample(false, false, Some(accent), now + Duration::from_millis(16))
            .running
    );
    assert_eq!(actual.target_builds, 1);
}

/// 零符号变化会重算纯端点，但是否重定向仍由原 set_target/jump_to 决定。
#[test]
fn signed_zero_and_nan_payload_changes_match_original_semantics() {
    let now = Instant::now();
    let base: Hsla = rgb(0xd7952c).into();
    for transition in [true, false] {
        let mut actual = super::MaterialMotion::new(now);
        let mut expected = OriginalMotion::new(now);
        for (step, alpha) in [
            0.0,
            -0.0,
            0.0,
            f32::from_bits(0x7fc01234),
            f32::from_bits(0x7fc01234),
            f32::from_bits(0x7fc01235),
        ]
        .into_iter()
        .enumerate()
        {
            let at = now + Duration::from_millis(step as u64 * 16);
            let accent = Some(Hsla { a: alpha, ..base });
            assert_sample(
                actual.sample_material(false, false, accent, at, transition),
                expected.sample_material(false, false, accent, at, transition),
            );
        }
        assert_eq!(actual.target_builds, 5);
    }
}

/// 原有两个公开入口也使用冻结实现对照，避免静态路径或无顶边路径漏接缓存。
#[test]
fn both_entry_points_keep_original_material_output() {
    let now = Instant::now();
    let mut actual = super::MaterialMotion::new(now);
    let mut expected = OriginalMotion::new(now);
    assert_sample(
        actual.sample(false, false, None, now),
        expected.sample(false, false, None, now),
    );
    for (step, dark, transition) in [
        (1, true, true),
        (2, false, false),
        (3, true, false),
        (4, false, true),
    ] {
        let at = now + Duration::from_millis(step * 80);
        assert_sample(
            actual.sample_static(dark, transition, at),
            expected.sample_static(dark, transition, at),
        );
    }
}

/// hover 隐藏期间换 accent 仍要在退出后采用最新颜色，不能复用旧顶边端点。
#[test]
fn accent_changes_while_hovered_are_used_after_hover_exit() {
    let now = Instant::now();
    let gold: Hsla = rgb(0xd7952c).into();
    let blue: Hsla = rgb(0x2c7be5).into();
    let mut actual = super::MaterialMotion::new(now);
    let mut expected = OriginalMotion::new(now);
    for (at, hovered, accent) in [
        (0, false, gold),
        (16, true, gold),
        (32, true, blue),
        (80, true, blue),
        (100, false, blue),
        (400, false, blue),
    ] {
        let at = now + Duration::from_millis(at);
        assert_sample(
            actual.sample(false, hovered, Some(accent), at),
            expected.sample(false, hovered, Some(accent), at),
        );
    }
    assert_eq!(actual.target_builds, 4);
}

/// 每个 Hsla 分量的零符号及异常颜色恢复都按旧公式输出，不以输入键替代目标比较。
#[test]
fn every_signed_zero_component_and_normal_color_recovery_match_original() {
    let now = Instant::now();
    let base: Hsla = rgb(0xd7952c).into();
    for field in 0..4 {
        for transition in [true, false] {
            let mut actual = super::MaterialMotion::new(now);
            let mut expected = OriginalMotion::new(now);
            for (step, value) in [0.0, -0.0, f32::from_bits(0x7fc01234), 0.0]
                .into_iter()
                .enumerate()
            {
                let mut accent = base;
                match field {
                    0 => accent.h = value,
                    1 => accent.s = value,
                    2 => accent.l = value,
                    _ => accent.a = value,
                }
                let at = now + Duration::from_millis(step as u64 * 16);
                assert_sample(
                    actual.sample_material(false, false, Some(accent), at, transition),
                    expected.sample_material(false, false, Some(accent), at, transition),
                );
            }
            for at in [64, 80, 364, 400] {
                let at = now + Duration::from_millis(at);
                assert_sample(
                    actual.sample_material(false, false, Some(base), at, transition),
                    expected.sample_material(false, false, Some(base), at, transition),
                );
            }
            assert_eq!(actual.target_builds, 5);
        }
    }
    assert!(std::mem::size_of::<super::MaterialMotion>() > std::mem::size_of::<OriginalMotion>());
    assert!(
        std::mem::size_of::<super::MaterialMotion>() - std::mem::size_of::<OriginalMotion>() <= 256
    );
}
