//! 卡片按信息层级绘制原生光轨；装饰不拥有输入、业务状态或图片资源。

use gpui_kit::{
    Bounds, ContentMask, Hsla, PathBuilder, Pixels, Point, Window, fill, linear_color_stop,
    linear_gradient, point, px, rgb,
};
use std::{f32::consts::{FRAC_PI_2, PI, TAU}, time::Instant};

/// 光轨八秒绕行一周，只有主要卡片持续请求活动窗口的可见帧。
const ORBIT_SECONDS: f32 = 8.;
/// 每条尾迹的渐隐段数固定，卡片尺寸和数据量不会扩大绘制工作量。
const TRAIL_SEGMENTS: usize = 10;
/// 较宽的低透明描边作为光晕，中心线内收保证圆角外没有漏光。
const GLOW_WIDTH: f32 = 4.;
/// 青、紫、玫红三条光轨保持固定间隔，避免正文也跟着变色。
const SPECTRUM: [u32; 3] = [0x22d3ee, 0xa78bfa, 0xfb7185];

/// 主要信息、工作面板和辅助信息分别拥有独立装饰强度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum CardPriority {
    /// 关键指标及当前工作对象：彩色环绕光轨与轻微呼吸。
    Primary,
    /// 图表、配置及资源卡：细角标与悬停光轨。
    Secondary,
    /// 明细、筛选及系统偏好：静态内嵌表面，不产生循环动画。
    Quiet,
}

/// 时间只累计实际播放间隔，窗口隐藏、失焦或减少动态效果时暂停。
#[derive(Default)]
pub(super) struct CardDecoration {
    /// 当前周期进度，始终位于0..1。
    phase: f32,
    /// 上一张实际播放帧的时刻，暂停时清空以免恢复后跳跃。
    last_tick: Option<Instant>,
}

impl CardDecoration {
    /// 后续恢复从当前光轨位置继续，不把后台经过时间补算进动画。
    pub fn pause(&mut self) {
        self.last_tick = None;
    }

    /// 只有主卡和正在悬停的次卡播放；静态降级仍保留同一彩边与角标。
    fn tick(&mut self, priority: CardPriority, hovered: bool, moving: bool, now: Instant) -> bool {
        let running = moving && match priority {
            CardPriority::Primary => true,
            CardPriority::Secondary => hovered,
            CardPriority::Quiet => false,
        };
        if running {
            if let Some(previous) = self.last_tick {
                self.phase = (self.phase + now.saturating_duration_since(previous).as_secs_f32() / ORBIT_SECONDS).rem_euclid(1.);
            }
            self.last_tick = Some(now);
        } else {
            self.pause();
        }
        running
    }

    /// 只画卡片边缘；祖先滚动裁切继续生效，且没有每帧CPU图片上传。
    pub fn paint(&mut self, bounds: Bounds<Pixels>, radius: Pixels, priority: CardPriority,
        dark: bool, hovered: bool, moving: bool, window: &mut Window) {
        let running = self.tick(priority, hovered, moving, window.frame_animation_time());
        let Some(rail) = RoundedRail::new(bounds, radius) else { return };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            match priority {
                CardPriority::Primary => {
                    let pulse = 0.84 + 0.16 * (self.phase * TAU).sin();
                    stroke(window, &rail, 0., rail.length, px(1.), color(0x8b8bda, if dark { 0.38 } else { 0.28 }));
                    let tail = (rail.length * 0.16).min(180.);
                    let strength = pulse * if dark { 0.94 } else { 0.78 };
                    for (index, hue) in SPECTRUM.into_iter().enumerate() {
                        let head = (self.phase + index as f32 / 3.) * rail.length;
                        for segment in 0..TRAIL_SEGMENTS {
                            let t = (segment + 1) as f32 / TRAIL_SEGMENTS as f32;
                            let start = head - tail + tail * segment as f32 / TRAIL_SEGMENTS as f32;
                            let end = head - tail + tail * t;
                            let alpha = t * t * strength;
                            // 光晕与亮线共用同一组采样点，只有线宽与透明度不同。
                            let points = sample(&rail, start, end);
                            stroke_points(window, &points, px(GLOW_WIDTH), color(hue, alpha * 0.12));
                            stroke_points(window, &points, px(1.5), color(hue, alpha));
                        }
                    }
                }
                CardPriority::Secondary => {
                    let length = (rail.length * 0.09).clamp(24., 72.);
                    let tint = color(if dark { 0x67c6db } else { 0x228297 }, if hovered { 0.8 } else { 0.40 });
                    // 两组转角线是面板层级标记，不能被误认成按钮焦点环。
                    stroke(window, &rail, -length * 0.35, length, px(1.25), tint);
                    stroke(window, &rail, rail.length * 0.5 - length * 0.35, rail.length * 0.5 + length, px(1.25), tint);
                    if hovered {
                        let head = self.phase * rail.length;
                        stroke(window, &rail, head, head + length, px(1.5), tint);
                    }
                }
                CardPriority::Quiet => {
                    stroke(window, &rail, 0., (rail.width * 0.25).min(64.), px(1.),
                        color(if dark { 0xb3bac4 } else { 0x6f7987 }, 0.24));
                }
            }
        });
        if running { window.request_animation_frame(); }
    }
}

/// 柔和双色表面只用于区分材质，透明度随层级降低，不覆盖文字或媒体。
pub(super) fn paint_wash(bounds: Bounds<Pixels>, radius: Pixels, priority: CardPriority, dark: bool, window: &mut Window) {
    let alpha = match priority { CardPriority::Primary => 0.075, CardPriority::Secondary => 0.035, CardPriority::Quiet => 0.018 };
    let tint = if dark { 0x45b7da } else { 0x168eac };
    window.paint_quad(fill(bounds, linear_gradient(120.,
        linear_color_stop(color(tint, alpha), 0.),
        linear_color_stop(color(0xa78bfa, alpha * 0.25), 1.),
    )).corner_radii(radius));
}

/// 同一RGB色值按当前明暗主题的透明度进入原生绘制。
fn color(value: u32, alpha: f32) -> Hsla { rgb(value).alpha(alpha).into() }

/// 向内收的圆角矩形中心线，宽光晕也始终落在实际卡片边界之内。
struct RoundedRail {
    /// 内收后的左上原点。
    origin: Point<Pixels>,
    /// 内收后的宽高与圆角，单位为逻辑像素。
    width: f32,
    height: f32,
    radius: f32,
    /// 一整圈中心线长度。
    length: f32,
}

impl RoundedRail {
    /// 零尺寸、小于光晕及非有限几何不产生路径；圆角按真实宽高夹紧。
    fn new(bounds: Bounds<Pixels>, radius: Pixels) -> Option<Self> {
        let inset = GLOW_WIDTH / 2. + 0.5;
        let width = f32::from(bounds.size.width) - inset * 2.;
        let height = f32::from(bounds.size.height) - inset * 2.;
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. { return None }
        let radius = (f32::from(radius) - inset).clamp(0., width.min(height) / 2.);
        Some(Self { origin: bounds.origin + point(px(inset), px(inset)), width, height, radius,
            length: 2. * (width + height - 4. * radius) + TAU * radius })
    }

    /// 沿真实圆角周长顺时针采样，支持跨越首尾及零圆角，不产生对角跳线。
    fn point(&self, distance: f32) -> Point<Pixels> {
        let r = self.radius;
        let x = self.width - 2. * r;
        let y = self.height - 2. * r;
        let arc = FRAC_PI_2 * r;
        let mut s = distance.rem_euclid(self.length);
        let segments = [x, arc, y, arc, x, arc, y, arc];
        for (index, length) in segments.into_iter().enumerate() {
            if s <= length || index == 7 {
                let angle = if r > 0. { s / r } else { 0. };
                let (x, y) = match index {
                    0 => (r + s, 0.),
                    1 => (self.width - r + (angle - FRAC_PI_2).cos() * r, r + (angle - FRAC_PI_2).sin() * r),
                    2 => (self.width, r + s),
                    3 => (self.width - r + angle.cos() * r, self.height - r + angle.sin() * r),
                    4 => (self.width - r - s, self.height),
                    5 => (r + (angle + FRAC_PI_2).cos() * r, self.height - r + (angle + FRAC_PI_2).sin() * r),
                    6 => (0., self.height - r - s),
                    _ => (r + (angle + PI).cos() * r, r + (angle + PI).sin() * r),
                };
                return self.origin + point(px(x), px(y));
            }
            s -= length;
        }
        self.origin
    }
}

/// 以固定最大步长近似圆弧；单条路径有界，移动尾迹跨周长接缝时仍连续。
fn stroke(window: &mut Window, rail: &RoundedRail, start: f32, end: f32, width: Pixels, color: Hsla) {
    stroke_points(window, &sample(rail, start, end), width, color);
}

/// 沿中心线从start到end按最大4px步长取点。
fn sample(rail: &RoundedRail, start: f32, end: f32) -> Vec<Point<Pixels>> {
    let steps = ((end - start).abs() / 4.).ceil().clamp(1., 512.) as usize;
    (0..=steps).map(|step| rail.point(start + (end - start) * step as f32 / steps as f32)).collect()
}

/// 把已采样的折线按指定线宽与颜色画成一条描边路径；点数不足时不产生路径。
fn stroke_points(window: &mut Window, points: &[Point<Pixels>], width: Pixels, color: Hsla) {
    let Some((first, rest)) = points.split_first() else { return };
    let mut path = PathBuilder::stroke(width);
    path.move_to(*first);
    for point in rest { path.line_to(*point); }
    if let Ok(path) = path.build() { window.paint_path(path, color); }
}

#[cfg(test)]
#[path = "decoration_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "decoration_render_tests.rs"]
mod render_tests;
