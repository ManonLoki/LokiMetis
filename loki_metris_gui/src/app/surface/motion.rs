//! 卡片保留历史 CSS 的时间线；系统减少动态效果不改变原有面板过渡合同。

use gpui_kit::base::Easing;
use std::time::{Duration, Instant};

/// 每个CSS属性独立保存可逆时间线；未变的目标不会因其他属性变化重新计时。
pub(super) struct CssMotion<const N: usize> {
    from: [f64; N],
    target: Option<[f64; N]>,
    reversing_start: [f64; N],
    reversing_factor: f32,
    started: Instant,
    duration: Duration,
    base_duration: Duration,
}

impl<const N: usize> CssMotion<N> {
    /// 首个目标立即采用，不在页面挂载时从虚构颜色或阴影播放动画。
    pub fn new(now: Instant) -> Self {
        Self::with_duration(now, Duration::from_millis(300))
    }

    /// 指定真实CSS属性的声明时长；卡片默认300ms，表格行单独采用160ms。
    pub fn with_duration(now: Instant, base_duration: Duration) -> Self {
        Self {
            from: [0.; N],
            target: None,
            reversing_start: [0.; N],
            reversing_factor: 1.,
            started: now,
            duration: Duration::ZERO,
            base_duration,
        }
    }

    /// 没有transition声明的CSS类立即采用目标，清空旧动画的反向起点。
    pub fn jump_to(&mut self, target: [f64; N], now: Instant) {
        self.from = target;
        self.target = Some(target);
        self.reversing_start = target;
        self.reversing_factor = 1.;
        self.started = now;
        self.duration = Duration::ZERO;
    }

    /// 采用 CSS reversing-adjusted start value，短时间反向不跳变或重新播放完整时长。
    pub fn set_target(&mut self, target: [f64; N], now: Instant) {
        let Some(previous) = self.target else {
            self.from = target;
            self.target = Some(target);
            self.reversing_start = target;
            return;
        };
        if previous == target {
            return;
        }
        let progress = self.progress(now);
        let sampled = self.sample(now).0;
        let factor = if target == self.reversing_start {
            (ease(progress) * self.reversing_factor + 1. - self.reversing_factor)
                .abs()
                .clamp(0., 1.)
        } else {
            1.
        };
        self.reversing_start = if target == self.reversing_start {
            previous
        } else {
            sampled
        };
        self.from = sampled;
        self.target = Some(target);
        self.reversing_factor = factor;
        self.started = now;
        self.duration = self.base_duration.mul_f32(factor);
    }

    /// 返回当前连续参数及是否还需要下一帧；结束后精确采用端点以复用静态纹理。
    pub fn sample(&self, now: Instant) -> ([f64; N], bool) {
        let target = self.target.unwrap_or(self.from);
        let progress = self.progress(now);
        if progress >= 1. || self.from == target {
            return (target, false);
        }
        (
            std::array::from_fn(|i| {
                self.from[i] + (target[i] - self.from[i]) * f64::from(ease(progress))
            }),
            true,
        )
    }

    /// 零时长与时钟边界均收敛到合法端点，不产生 NaN。
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            1.
        } else {
            (now.saturating_duration_since(self.started).as_secs_f32()
                / self.duration.as_secs_f32())
            .clamp(0., 1.)
        }
    }
}

/// 面板使用历史 app-ease 控制点，和控件自身的 Ease 曲线分开。
fn ease(progress: f32) -> f32 {
    Easing::CubicBezier {
        x1: 0.16,
        y1: 1.,
        x2: 0.3,
        y2: 1.,
    }
    .sample(progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 连续采样及反向都保持当前位置，最终在有限时间准确落到静态端点。
    #[test]
    fn reversing_hover_keeps_continuity_and_finishes() {
        let now = Instant::now();
        let mut motion = CssMotion::new(now);
        motion.set_target([0.], now);
        assert_eq!(motion.sample(now), ([0.], false));
        motion.set_target([1.], now);
        let turn = now + Duration::from_millis(80);
        let value = motion.sample(turn).0[0];
        assert!(value > 0. && value < 1.);
        motion.set_target([0.], turn);
        assert!((motion.sample(turn).0[0] - value).abs() < f64::EPSILON);
        let second = turn + Duration::from_millis(20);
        let value = motion.sample(second).0[0];
        motion.set_target([1.], second);
        assert!((motion.sample(second).0[0] - value).abs() < f64::EPSILON);
        assert_eq!(
            motion.sample(second + Duration::from_secs(1)),
            ([1.], false)
        );
    }

    /// 表格显式160ms不会改变卡片默认300ms；零时长合同也稳定收敛。
    #[test]
    fn explicit_row_duration_keeps_panel_default_and_zero_duration_valid() {
        let now = Instant::now();
        let mut panel = CssMotion::new(now);
        let mut row = CssMotion::with_duration(now, Duration::from_millis(160));
        let mut immediate = CssMotion::with_duration(now, Duration::ZERO);
        for motion in [&mut panel, &mut row, &mut immediate] {
            motion.set_target([0.], now);
            motion.set_target([1.], now);
        }
        assert!(panel.sample(now + Duration::from_millis(160)).1);
        assert_eq!(row.sample(now + Duration::from_millis(160)), ([1.], false));
        assert_eq!(panel.sample(now + Duration::from_millis(300)), ([1.], false));
        assert_eq!(immediate.sample(now), ([1.], false));
    }
}
