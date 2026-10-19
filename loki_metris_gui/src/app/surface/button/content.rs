//! 明确历史尺寸的内容层与加载时间线；文字始终参与布局，加载不改变固有宽度。

use gpui_kit::{
    AnyElement, Bounds, FontWeight, PathBuilder, Pixels, Rgba, SharedString, Window,
    base::{Easing, TestSupportExt as _},
    canvas,
    component::Icon,
    div, point,
    prelude::*,
    px, relative,
};
use std::time::{Duration, Instant};

/// 本轮仅支持真实出现的 sm 保存与 xs 写入，避免隐式替换所有原生控件。
#[derive(Clone, Copy)]
pub(in crate::app) enum HistoricalButtonSize {
    /// sm 档，用于保存按钮。
    Small,
    /// xs 档，用于写入按钮。
    XSmall,
}
impl HistoricalButtonSize {
    /// Mantine 9.6 的实际 border-box 高度。
    pub(super) fn height(self) -> f32 {
        match self {
            Self::Small => 36.,
            Self::XSmall => 30.,
        }
    }
    /// 无 section 时两侧内边距；左 section 单独除以1.5。
    pub(super) fn padding(self) -> f32 {
        match self {
            Self::Small => 18.,
            Self::XSmall => 14.,
        }
    }
    /// 内容自己指定字号，不能被组件 label 层默认字号覆盖。
    pub(super) fn font_size(self) -> f32 {
        match self {
            Self::Small => 14.,
            Self::XSmall => 12.,
        }
    }
}

/// 普通 CSS ease 过渡，支持同属性反向时连续采样与缩短剩余时间。
struct Transition {
    /// 本段过渡起点。
    from: f32,
    /// 目标值；首次设置前为 None。
    target: Option<f32>,
    /// CSS 过渡规范的 reversing-adjusted start value。
    reversing_start: f32,
    /// CSS 过渡规范的 reversing shortening factor。
    reversing_factor: f32,
    /// 本段过渡开始的时刻。
    started: Option<Instant>,
    /// 本段过渡时长。
    duration: Duration,
    /// 未缩短前的基础时长。
    base: Duration,
}
impl Transition {
    /// 初始目标直接绘制，不制造挂载过渡。
    fn new(milliseconds: u64) -> Self {
        Self {
            from: 0.,
            target: None,
            reversing_start: 0.,
            reversing_factor: 1.,
            started: None,
            duration: Duration::ZERO,
            base: Duration::from_millis(milliseconds),
        }
    }
    /// 与历史 CSS 独立的 ease(.25,.1,.25,1)，不改变外层 app-ease。
    fn ease(progress: f32) -> f32 {
        Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.,
        }
        .sample(progress)
    }
    /// 过渡前后均来自同一个测试/生产执行器时钟。
    fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.;
        }
        (now.saturating_duration_since(self.started.unwrap())
            .as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0., 1.)
    }
    /// 目标改变时保存真正当前位置，重复 render 不重启时间线。
    fn sample(&mut self, target: f32, now: Instant) -> (f32, bool) {
        match self.target {
            None => {
                self.target = Some(target);
                self.from = target;
                self.reversing_start = target;
            }
            Some(previous) if previous != target => {
                let progress = Self::ease(self.progress(now));
                let sampled = self.from + (previous - self.from) * progress;
                let factor = if target == self.reversing_start {
                    (progress * self.reversing_factor + 1. - self.reversing_factor)
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
                self.started = Some(now);
                self.duration = self.base.mul_f32(factor);
            }
            _ => {}
        }
        let progress = self.progress(now);
        let target = self.target.unwrap();
        (
            self.from + (target - self.from) * Self::ease(progress),
            progress < 1. && self.from != target,
        )
    }
}

/// 三个 CSS 属性独立计时；旋转只在真实 loading 生命周期内存在。
pub(super) struct LoadingMotion {
    /// 文字不透明度过渡。
    opacity: Transition,
    /// 文字位移过渡。
    slide: Transition,
    /// 加载指示器显现过渡。
    loader: Transition,
    /// 当前处于加载态。
    loading: bool,
    /// 旋转开始的时刻；非加载态为 None。
    rotation_start: Option<Instant>,
}
/// 单帧不可变绘制参数，不向业务状态或偏好写入动画信息。
#[derive(Clone, Copy)]
pub(super) struct LoadingPaint {
    /// 文字不透明度。
    opacity: f32,
    /// 文字位移。
    slide: f32,
    /// 加载指示器不透明度。
    loader: f32,
    /// 加载指示器旋转角度。
    angle: f32,
    /// 任一过渡或旋转仍在进行，需要继续请求下一帧。
    pub(super) running: bool,
}
impl LoadingPaint {
    /// before与inner共享同样时长和ease，但遮罩从透明上方进入。
    pub(super) fn veil_opacity(self) -> f32 {
        1. - self.opacity
    }
    /// 遮罩当前的滑动进度：0 表示还在起点，1 表示已经滑到终点。
    pub(super) fn veil_slide(self) -> f32 {
        self.slide
    }
}
impl LoadingMotion {
    /// 历史 inner opacity100ms、inner/loader transform150ms。
    pub(super) fn new() -> Self {
        Self {
            opacity: Transition::new(100),
            slide: Transition::new(150),
            loader: Transition::new(150),
            loading: false,
            rotation_start: None,
        }
    }
    /// Loader 旋转保持1.2s linear，不随每次重绘重新从零开始。
    pub(super) fn sample(&mut self, loading: bool, now: Instant) -> LoadingPaint {
        if loading && !self.loading {
            self.rotation_start = Some(now);
        }
        self.loading = loading;
        let target = f32::from(loading);
        let (opacity, a) = self.opacity.sample(target, now);
        let (slide, b) = self.slide.sample(target, now);
        let (loader, c) = self.loader.sample(target, now);
        let angle = self
            .rotation_start
            .map(|start| {
                (now.saturating_duration_since(start).as_secs_f32() / 1.2).fract()
                    * std::f32::consts::TAU
            })
            .unwrap_or(0.);
        LoadingPaint {
            opacity: 1. - opacity,
            slide,
            loader,
            angle,
            running: a || b || c,
        }
    }
}

/// 原生 Button 的唯一内容子树；图标隐藏辅助语义，真实文字保留单独行盒。
pub(super) fn element(
    label: SharedString,
    icon: Option<Icon>,
    icon_size: Pixels,
    size: HistoricalButtonSize,
    loading: bool,
    foreground: Rgba,
    paint: LoadingPaint,
) -> AnyElement {
    let has_icon = icon.is_some();
    let mut inner = div()
        .id("historical-button-inner")
        .test_support()
        .relative()
        .top(px(paint.slide * (size.height() - 2.)))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .opacity(paint.opacity)
        .text_size(px(size.font_size()))
        .font_weight(FontWeight::SEMIBOLD)
        .line_height(px(size.font_size()));
    if let Some(icon) = icon {
        inner = inner.child(
            div()
                .id("historical-button-icon")
                .test_support()
                .size(icon_size)
                .flex_shrink_0()
                .child(icon.size(icon_size)),
        );
    }
    inner = inner.child(
        div()
            .id("historical-button-label")
            .test_support()
            .whitespace_nowrap()
            .line_height(px(size.font_size()))
            .text_size(px(size.font_size()))
            .opacity(if loading { 0.2 } else { 1. })
            .child(label),
    );
    let diameter = size.height() / 1.8;
    // loader定位相对整个border-box；原生内容区的非对称padding需抵消半差。
    let compensation = if has_icon {
        (size.padding() - size.padding() / 1.5) / 2.
    } else {
        0.
    };
    let loader = div()
        .id("historical-button-loader")
        .test_support()
        .absolute()
        .left(relative(0.5))
        .top(relative(0.5))
        .ml(px(-diameter / 2. + compensation))
        .mt(px(-2. * diameter + paint.loader * (1.5 * diameter + 1.)))
        .size(px(diameter))
        .opacity(paint.loader)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if paint.loader > 0. {
                        oval(bounds, paint.angle, foreground, window);
                    }
                },
            )
            .size_full(),
        );
    div()
        .relative()
        .h_full()
        .flex_shrink_0()
        .child(inner)
        .child(loader)
        .into_any_element()
}

/// CSS oval 为三侧同色、一侧透明的圆环；矢量路径避免纹理、缓存和额外像素租约。
fn oval(bounds: Bounds<gpui_kit::Pixels>, angle: f32, color: Rgba, window: &mut Window) {
    let radius = bounds.size.width / 2.;
    let inner = radius - bounds.size.width / 8.;
    let center = bounds.center();
    let start = angle + std::f32::consts::PI * 1.25;
    let mid = start + std::f32::consts::PI * 0.75;
    let end = start + std::f32::consts::PI * 1.5;
    let position =
        |r: gpui_kit::Pixels, theta: f32| center + point(r * theta.cos(), r * theta.sin());
    let mut path = PathBuilder::fill();
    path.move_to(position(radius, start));
    path.arc_to(
        point(radius, radius),
        px(0.),
        false,
        true,
        position(radius, mid),
    );
    path.arc_to(
        point(radius, radius),
        px(0.),
        false,
        true,
        position(radius, end),
    );
    path.line_to(position(inner, end));
    path.arc_to(
        point(inner, inner),
        px(0.),
        false,
        false,
        position(inner, mid),
    );
    path.arc_to(
        point(inner, inner),
        px(0.),
        false,
        false,
        position(inner, start),
    );
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
