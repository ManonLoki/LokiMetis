//! AppKit 的 zoom 动画先发送普通 resize，再更新 isZoomed；静默后才允许读回几何。

use std::time::{Duration, Instant};

const GEOMETRY_QUIET_PERIOD: Duration = Duration::from_millis(250);

/// zoom delegate 提供的原生 AppKit 外框，只在宿主边界使用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NativeFrame {
    /// 外框左下角在 AppKit 全局坐标中的横坐标（点）。
    pub x: f64,
    /// 外框左下角在 AppKit 全局坐标中的纵坐标（点，原点在主屏底部）。
    pub y: f64,
    /// 外框宽度（点）。
    pub width: f64,
    /// 外框高度（点）。
    pub height: f64,
}

/// 明确目标保护 zoom/unzoom 中间帧；未知终态不会因计时到期而自动接受。
#[derive(Clone, Default, PartialEq)]
pub(super) struct ZoomGuard {
    /// 进行中的 zoom/unzoom 期望外框；到达前的中间帧都被拒绝。
    target: Option<NativeFrame>,
    /// 目标外框不可用（含非有限值）；此时须等显式恢复，不因计时到期而接受。
    invalid: bool,
}

impl ZoomGuard {
    /// 重复 zoom 意图替换旧目标，不能把前一动画中间矩形作为普通几何。
    pub fn begin(&mut self, target: Option<NativeFrame>) {
        self.target = target.filter(|frame| [frame.x, frame.y, frame.width, frame.height].into_iter().all(f64::is_finite)
            && frame.width > 0. && frame.height > 0.);
        self.invalid = self.target.is_none();
    }
    /// 非法目标也保持保护，必须由明确恢复动作解除。
    pub fn pending(&self) -> bool { self.invalid || self.target.is_some() }
    /// 只有明确终点、用户完成 live resize 或新的受控恢复动作才撤销保护。
    pub fn clear(&mut self) { self.target = None; self.invalid = false; }
    /// 容差以当前原生 backing 像素给出，不用不同显示器的逻辑比例重解释外框。
    pub fn observe(&mut self, frame: NativeFrame, tolerance: f64) -> bool {
        if self.invalid { return false; }
        let Some(target) = self.target else { return true; };
        let close = |a: f64, b: f64| (a - b).abs() <= tolerance;
        if tolerance.is_finite() && tolerance >= 0. && close(frame.x, target.x) && close(frame.y, target.y)
            && close(frame.width, target.width) && close(frame.height, target.height) {
            self.clear(); return true;
        }
        false
    }
}

/// 只合并宿主事件；空闲时不创建 task、不读取屏幕，也不触发偏好写入。
#[derive(Default)]
pub(super) struct GeometrySettle {
    /// 最近一次几何变化的时刻；经过静默期后才视为稳定。
    last_change: Option<Instant>,
}

impl GeometrySettle {
    /// 原生通知与 GPUI bounds 回调均记时，避免两者回调顺序造成动画帧漏拦。
    pub fn changed(&mut self, now: Instant) { self.last_change = Some(now); }

    /// 单调时间不受系统时钟调整影响；每个动画帧都会重新延后允许读回的时刻。
    pub fn ready(&self, now: Instant) -> bool {
        self.last_change.is_none_or(|last| now.saturating_duration_since(last) >= GEOMETRY_QUIET_PERIOD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定普通高度，仅改变宽度以复现系统 zoom 中间帧。
    fn frame(width: f64) -> NativeFrame { NativeFrame { x: 0., y: 33., width, height: 917. } }

    /// 中间外框不解除保护，达到半个 backing 像素容差内的目标才完成。
    #[test]
    fn zoom_guard_rejects_intermediate_frame_until_actual_target_arrives() {
        let mut guard = ZoomGuard::default();
        guard.begin(Some(frame(1512.)));
        assert!(!guard.observe(frame(1383.), 0.25));
        assert!(guard.pending());
        assert!(guard.observe(frame(1511.8), 0.25));
        assert!(!guard.pending());
    }

    /// 反向 unzoom 意图取代尚未结束的 zoom，旧终点不能误判成功。
    #[test]
    fn new_unzoom_target_replaces_in_flight_zoom_target() {
        let mut guard = ZoomGuard::default();
        guard.begin(Some(frame(1512.)));
        guard.begin(Some(frame(961.)));
        assert!(!guard.observe(frame(1512.), 0.25));
        assert!(guard.observe(frame(961.), 0.25));
    }

    /// 非有限或缺失目标不能按时间自动接受，显式恢复才撤销保护。
    #[test]
    fn invalid_zoom_target_requires_an_explicit_recovery() {
        let mut guard = ZoomGuard::default();
        guard.begin(Some(frame(f64::NAN)));
        assert!(!guard.observe(frame(961.), 0.25));
        guard.clear();
        assert!(guard.observe(frame(961.), 0.25));
        guard.begin(None);
        assert!(!guard.observe(frame(961.), 0.25));
    }

    /// isZoomed 晚于 resize 更新时，静默窗口仍不能捕获动画中间尺寸。
    #[test]
    fn zoom_intermediate_frames_never_become_saved_normal_geometry() {
        let start = Instant::now();
        let mut settle = GeometrySettle::default();
        let mut saved_width = 961;
        for (millis, animated_width, zoomed) in [(0, 1100, false), (40, 1383, false), (80, 1512, true)] {
            let now = start + Duration::from_millis(millis);
            settle.changed(now);
            if settle.ready(now) && !zoomed { saved_width = animated_width; }
        }
        assert!(!settle.ready(start + Duration::from_millis(329)));
        assert!(settle.ready(start + Duration::from_millis(330)));
        assert_eq!(saved_width, 961);
    }

    /// 连续普通 resize 只在最后事件静默后允许读回，空闲不重新延期。
    #[test]
    fn normal_resize_is_readable_once_after_the_final_event() {
        let start = Instant::now();
        let mut settle = GeometrySettle::default();
        assert!(settle.ready(start));
        settle.changed(start);
        settle.changed(start + Duration::from_millis(200));
        assert!(!settle.ready(start + Duration::from_millis(449)));
        assert!(settle.ready(start + Duration::from_millis(450)));
        assert!(settle.ready(start + Duration::from_secs(60)));
    }

    /// 比最后事件更早的观测不会使待保存几何提前可读。
    #[test]
    fn backwards_observation_cannot_make_pending_geometry_ready() {
        let start = Instant::now();
        let mut settle = GeometrySettle::default();
        settle.changed(start + Duration::from_secs(1));
        assert!(!settle.ready(start));
    }
}
