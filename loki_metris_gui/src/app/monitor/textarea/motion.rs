//! 焦点描边沿历史一百毫秒标准 CSS ease 变化，反向不跳变或重新播放完整时长。

use gpui_kit::{Rgba, base::Easing};
use std::time::{Duration, Instant};

/// 每个输入独立拥有颜色时间线，未变的目标不会因编辑或父页面刷新重新计时。
pub(in crate::app::monitor) struct BorderTransition {
    /// 本段过渡的起始颜色。
    from: Rgba,
    /// 目标颜色；首次设置前为 None。
    target: Option<Rgba>,
    /// CSS 过渡规范的 reversing-adjusted start value：中途反向时的参考起点。
    reversing_start: Rgba,
    /// CSS 过渡规范的 reversing shortening factor，用于缩短中途反向的时长。
    reversing_factor: f32,
    /// 本段过渡开始的时刻。
    started: Option<Instant>,
    /// 本段过渡时长。
    duration: Duration,
}

impl BorderTransition {
    /// 挂载时直接绘制真实目标色，不从透明色播放虚构入场动画。
    pub(super) fn new() -> Self {
        Self {
            from: Rgba::default(),
            target: None,
            reversing_start: Rgba::default(),
            reversing_factor: 1.,
            started: None,
            duration: Duration::ZERO,
        }
    }

    /// Mantine Input.css 的 ease 与 app-ease 不同，不能借用卡片的贝塞尔参数。
    fn ease(progress: f32) -> f32 {
        Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.,
        }
        .sample(progress)
    }

    /// 所有取样使用同一个执行器时钟，生产与真实组件测试遵循同样的时间推进。
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.;
        }
        (now.saturating_duration_since(self.started.unwrap())
            .as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0., 1.)
    }

    /// 无论焦点或主题改变，先采样旧颜色，再按 CSS reversing-adjusted start value 转向。
    pub(super) fn sample(&mut self, target: Rgba, now: Instant) -> (Rgba, bool) {
        match self.target {
            None => {
                self.target = Some(target);
                self.from = target;
                self.reversing_start = target;
            }
            Some(previous) if previous != target => {
                let progress = Self::ease(self.progress(now));
                let sampled = interpolate(self.from, previous, progress);
                let reversing = target == self.reversing_start;
                let factor = if reversing {
                    (progress * self.reversing_factor + 1. - self.reversing_factor)
                        .abs()
                        .clamp(0., 1.)
                } else {
                    1.
                };
                self.reversing_start = if reversing { previous } else { sampled };
                self.from = sampled;
                self.target = Some(target);
                self.reversing_factor = factor;
                self.started = Some(now);
                self.duration = Duration::from_millis(100).mul_f32(factor);
            }
            _ => {}
        }
        let progress = self.progress(now);
        let target = self.target.unwrap();
        (
            interpolate(self.from, target, Self::ease(progress)),
            progress < 1. && self.from != target,
        )
    }
}

/// 历史输入描边均完全不透明，RGBA 逐通道插值无需引入透明预乘或额外色板。
fn interpolate(from: Rgba, to: Rgba, progress: f32) -> Rgba {
    if progress >= 1. {
        return to;
    }
    Rgba {
        r: from.r + (to.r - from.r) * progress,
        g: from.g + (to.g - from.g) * progress,
        b: from.b + (to.b - from.b) * progress,
        a: from.a + (to.a - from.a) * progress,
    }
}
