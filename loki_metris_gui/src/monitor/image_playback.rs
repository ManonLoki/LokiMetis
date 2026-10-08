//! 图片时间线与自有原生画布，支持有限循环并在失焦窗口继续播放。

use super::image_decode::{DecodedMonitorImage, MonitorImageDecoder, MonitorImageSource};
use gpui_kit::{
    Bounds, ContentMask, Context, Corners, ObjectFit, Render, Subscription, Task, Window, canvas,
    div, point, prelude::*, size,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// 帧边界用于按真实经过时间定位，调度迟到不累积播放漂移。
struct Timeline {
    ends: Vec<Duration>,
    cycle: Duration,
    plays: Option<u32>,
}
impl Timeline {
    /// 与参考 App 的 WebKit 一致，低于11ms的帧以100ms显示。
    /// 来源：https://github.com/WebKit/WebKit/blob/main/Source/WebCore/platform/graphics/cg/ImageDecoderCG.cpp
    fn new(image: &DecodedMonitorImage) -> Self {
        let mut total = Duration::ZERO;
        let ends = (0..image.image.frame_count())
            .map(|index| {
                let delay = Duration::from(image.image.delay(index));
                total = total.saturating_add(if delay < Duration::from_millis(11) {
                    Duration::from_millis(100)
                } else {
                    delay
                });
                total
            })
            .collect();
        Self {
            ends,
            cycle: total,
            plays: image.plays,
        }
    }
    /// 有限播放结束后停在末帧，不继续安排任务。
    fn at(&self, elapsed: Duration) -> (usize, Option<Duration>) {
        if self.ends.len() <= 1
            || self
                .plays
                .is_some_and(|plays| elapsed >= self.cycle.saturating_mul(plays))
        {
            return (self.ends.len().saturating_sub(1), None);
        }
        let local = elapsed.as_nanos() % self.cycle.as_nanos();
        let index = self.ends.partition_point(|end| end.as_nanos() <= local);
        let next = Duration::from_nanos(
            (self.ends[index].as_nanos() - local).min(u128::from(u64::MAX)) as u64,
        );
        (index, Some(next))
    }
}

/// 每个挂载位置拥有一个时钟；像素缓存仍按图库ID共享而不复制。
pub(crate) struct MonitorImageView {
    source: MonitorImageSource,
    fit: ObjectFit,
    decoded: Option<Arc<DecodedMonitorImage>>,
    timeline: Option<Timeline>,
    frame: usize,
    elapsed: Duration,
    started: Option<Instant>,
    mounted: bool,
    element_visible: bool,
    finished: bool,
    generation: u64,
    task: Option<Task<()>>,
    #[cfg(test)]
    painted: std::rc::Rc<std::cell::Cell<usize>>,
    _visibility: Subscription,
}
impl MonitorImageView {
    /// 观察可见性而非激活状态；用户切到另一个 App 不会冻结图库 GIF。
    pub fn new(
        source: MonitorImageSource,
        fit: ObjectFit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let visibility = cx.observe_window_visibility(window, |this, _, window, cx| {
            if !window.is_visible() {
                this.pause(cx);
            } else {
                this.start(window, cx);
            }
            cx.notify();
        });
        Self {
            source,
            fit,
            decoded: None,
            timeline: None,
            frame: 0,
            elapsed: Duration::ZERO,
            started: None,
            mounted: false,
            element_visible: false,
            finished: false,
            generation: 0,
            task: None,
            #[cfg(test)]
            painted: Default::default(),
            _visibility: visibility,
        }
    }
    /// 桌宠只在真实解码完成后隐藏空位轮廓。
    pub fn is_decoded(&self) -> bool { self.decoded.is_some() }
    /// 页面离开或选项卸载立即释放像素并取消计时，保留当前时间点。
    pub fn set_mounted(&mut self, mounted: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.mounted == mounted {
            if !mounted {
                self.release_pixels(cx);
            }
            return;
        }
        self.mounted = mounted;
        if mounted {
            self.start(window, cx);
        } else {
            self.element_visible = false;
            self.release_pixels(cx);
        }
    }
    /// 像素释放与时间线分离，离屏后恢复仍使用剩余延时和有限循环终态。
    pub fn release_pixels(&mut self, cx: &Context<Self>) -> Option<Arc<DecodedMonitorImage>> {
        if self.started.is_some() {
            self.pause(cx);
        }
        self.decoded.take()
    }
    /// 只有真实可见画布才请求资源，缓存未命中时由 Asset 完成通知重绘。
    fn ensure_decoded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.decoded.is_none() {
            if let Some(Ok(decoded)) = window.use_asset::<MonitorImageDecoder>(&self.source, cx) {
                if self.timeline.is_none() {
                    self.timeline = Some(Timeline::new(&decoded));
                }
                self.decoded = Some(decoded);
                cx.notify();
            }
        }
    }
    /// 可测试 executor 时钟同时覆盖运行与暂停时间。
    fn elapsed_at(&self, now: Instant) -> Duration {
        self.elapsed.saturating_add(
            self.started
                .map(|start| now.saturating_duration_since(start))
                .unwrap_or_default(),
        )
    }
    /// 暂停不丢失当前帧剩余延时，旧任务代数不能继续推进。
    fn pause(&mut self, cx: &Context<Self>) {
        self.elapsed = self.elapsed_at(cx.background_executor().now());
        self.started = None;
        self.generation = self.generation.wrapping_add(1);
        self.task.take();
    }
    /// 唯一计时 owner 只为实际可见动画工作，静态图和已结束动画没有轮询。
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.started.is_some()
            || self.decoded.is_none()
            || self.finished
            || !self.mounted
            || !self.element_visible
            || !window.is_visible()
        {
            return;
        }
        let Some(timeline) = &self.timeline else {
            return;
        };
        let (frame, next) = timeline.at(self.elapsed);
        self.frame = frame;
        let Some(mut delay) = next else {
            self.finished = true;
            return;
        };
        self.started = Some(cx.background_executor().now());
        let generation = self.generation;
        self.task = Some(cx.spawn_in(window, async move |entity, cx| {
            loop {
                cx.background_executor().timer(delay).await;
                let next = entity
                    .update_in(cx, |this, window, cx| {
                        if this.generation != generation {
                            return None;
                        }
                        if !this.mounted || !this.element_visible || !window.is_visible() {
                            this.started = None;
                            return None;
                        }
                        let elapsed = this.elapsed_at(cx.background_executor().now());
                        let (frame, next) = this.timeline.as_ref()?.at(elapsed);
                        this.frame = frame;
                        if next.is_none() {
                            this.elapsed = elapsed;
                            this.started = None;
                            this.finished = true;
                        }
                        cx.notify();
                        next
                    })
                    .ok()
                    .flatten();
                match next {
                    Some(next) => delay = next,
                    None => return,
                }
            }
        }));
    }
}
impl Render for MonitorImageView {
    /// Canvas公开选帧避免 Img 内部无限循环及失焦停止逻辑。
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = self.decoded.as_ref().map(Arc::downgrade);
        let frame = self.frame;
        let fit = match self.fit {
            ObjectFit::Fill => ObjectFit::Fill,
            ObjectFit::Contain => ObjectFit::Contain,
            ObjectFit::Cover => ObjectFit::Cover,
            ObjectFit::ScaleDown => ObjectFit::ScaleDown,
            ObjectFit::None => ObjectFit::None,
        };
        let weak = cx.entity().downgrade();
        #[cfg(test)]
        let painted = self.painted.clone();
        div().size_full().child(
            canvas(
                move |bounds, window, cx| {
                    let visible = bounds.intersect(&window.content_mask().bounds);
                    let visible = window.is_visible()
                        && visible.size.width > gpui_kit::px(0.)
                        && visible.size.height > gpui_kit::px(0.);
                    weak.update(cx, |this, cx| {
                        if this.element_visible != visible {
                            this.element_visible = visible;
                            if visible {
                                this.ensure_decoded(window, cx);
                                this.start(window, cx);
                            } else {
                                this.release_pixels(cx);
                            }
                        } else if visible {
                            this.ensure_decoded(window, cx);
                            this.start(window, cx);
                        }
                        visible.then_some(this.frame)
                    })
                    .unwrap_or_else(|_| visible.then_some(frame))
                },
                move |bounds, frame, window, _| {
                    if let (Some(decoded), Some(frame)) = (image.and_then(|image|image.upgrade()), frame) {
                        let natural_size = decoded.natural_size;
                        let image_bounds = fit.get_bounds(bounds, natural_size);
                        let dx = image_bounds.size.width / natural_size.width.0 as f32;
                        let dy = image_bounds.size.height / natural_size.height.0 as f32;
                        let padded = Bounds::new(
                            point(image_bounds.origin.x - dx, image_bounds.origin.y - dy),
                            size(
                                image_bounds.size.width + dx * 2.,
                                image_bounds.size.height + dy * 2.,
                            ),
                        );
                        let result = window.with_content_mask(
                            Some(ContentMask {
                                bounds: image_bounds.intersect(&bounds),
                            }),
                            |window| {
                                window.paint_image(
                                    padded,
                                    padded,
                                    Corners::default(),
                                    decoded.image.clone(),
                                    frame,
                                    false,
                                )
                            },
                        );
                        #[cfg(test)]
                        if result.is_ok() {
                            painted.set(frame);
                        }
                        #[cfg(not(test))]
                        let _ = result;
                    }
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, TestAppContext, WindowVisibility, test::TestWindowExt};

    /// 独立真实窗口使用实际两帧像素，测试调度器拥有播放任务。
    fn player(
        cx: &mut TestAppContext,
        plays: Option<u32>,
    ) -> (
        gpui_kit::AnyWindowHandle,
        gpui_kit::Entity<MonitorImageView>,
    ) {
        let frames = [0u8, 255]
            .into_iter()
            .map(|value| {
                image::Frame::from_parts(
                    image::RgbaImage::from_pixel(3, 3, image::Rgba([value, value, value, 255])),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(200, 1),
                )
            })
            .collect::<Vec<_>>();
        let decoded = Arc::new(DecodedMonitorImage {
            image: Arc::new(gpui_kit::RenderImage::new(frames)),
            natural_size: size(gpui_kit::DevicePixels(1), gpui_kit::DevicePixels(1)),
            plays,
            _memory: super::super::image_memory::ImageMemoryLease::reserve(72).unwrap(),
        });
        let handle = cx.add_window(move |window, cx| {
            let source = MonitorImageSource {
                id: "playback-fixture".into(),
                bytes: Arc::from(
                    include_bytes!("../../tests/fixtures/monitor-loop.gif").as_slice(),
                ),
                format: loki_metris_core::ImageFormat::Gif,
            };
            let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx);
            view.timeline = Some(Timeline::new(&decoded));
            view.decoded = Some(decoded);
            view.mounted = true;
            view
        });
        let entity = handle.update(cx, |_, _, cx| cx.entity().clone()).unwrap();
        cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        (handle.into(), entity)
    }

    /// 失焦仍绘制下一帧；隐藏暂停后恢复使用剩余延时。
    #[gpui_kit::test]
    fn inactive_playback_and_hidden_resume_preserve_real_frame(cx: &mut TestAppContext) {
        let (handle, entity) = player(cx, None);
        {
            let mut visual = gpui_kit::VisualTestContext::from_window(handle, cx);
            visual.deactivate_window();
        }
        cx.executor().advance_clock(Duration::from_millis(80));
        cx.run_until_parked();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
        cx.executor().advance_clock(Duration::from_millis(1000));
        cx.run_until_parked();
        assert_eq!(entity.read_with(cx, |view, _| view.frame), 0);
        cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
        cx.executor().advance_clock(Duration::from_millis(119));
        cx.run_until_parked();
        assert_eq!(entity.read_with(cx, |view, _| view.frame), 0);
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            assert!(!window.is_window_active());
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(
            entity.read_with(cx, |view, _| (view.frame, view.painted.get())),
            (1, 1)
        );
    }

    /// 一轮结束绘制末帧且停止任务，继续推进时间不能回到首帧。
    #[gpui_kit::test]
    fn finite_player_paints_last_frame_and_stops(cx: &mut TestAppContext) {
        let (handle, entity) = player(cx, Some(1));
        cx.executor().advance_clock(Duration::from_millis(410));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        assert_eq!(
            entity.read_with(cx, |view, _| (
                view.frame,
                view.painted.get(),
                view.finished
            )),
            (1, 1, true)
        );
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(entity.read_with(cx, |view, _| view.frame), 1);
    }
    /// 上一帧的Canvas不能继续强持已卸载图片，释放后像素和计时都消失。
    #[gpui_kit::test]
    fn previous_canvas_does_not_retain_unmounted_decoded_pixels(cx: &mut TestAppContext) {
        let (handle,entity)=player(cx,None);
        let weak=entity.read_with(cx,|view,_|Arc::downgrade(view.decoded.as_ref().unwrap()));
        cx.update_window(handle,|_,window,cx|entity.update(cx,|view,cx|view.set_mounted(false,window,cx))).unwrap();
        assert!(weak.upgrade().is_none(),"paint closure retained all decoded frames after unmount");
        assert!(entity.read_with(cx,|view,_|view.task.is_none()&&view.started.is_none()));
    }

    /// 两轮结束必须停在最后帧；迟到回调按时间直接定位而不丢失整轮。
    #[test]
    fn finite_timeline_stops_at_last_frame_and_infinite_keeps_running() {
        let mut timeline = Timeline {
            ends: vec![Duration::from_millis(200), Duration::from_millis(400)],
            cycle: Duration::from_millis(400),
            plays: Some(2),
        };
        assert_eq!(
            timeline.at(Duration::from_millis(0)),
            (0, Some(Duration::from_millis(200)))
        );
        assert_eq!(
            timeline.at(Duration::from_millis(450)),
            (0, Some(Duration::from_millis(150)))
        );
        assert_eq!(
            timeline.at(Duration::from_millis(799)),
            (1, Some(Duration::from_millis(1)))
        );
        assert_eq!(timeline.at(Duration::from_millis(800)), (1, None));
        timeline.plays = None;
        assert_eq!(
            timeline.at(Duration::from_millis(800)),
            (0, Some(Duration::from_millis(200)))
        );
    }
}
