//! 静态首帧只消费正式平台请求；禁止强制绘制或动画时钟替加载回执补帧。

use super::*;
use crate::monitor::image_decode::{LoadProbe, hold_decoders_for_test};
use gpui_kit::{RequestFrameOptions, TestAppContext};
use std::{cell::Cell, rc::Rc};

/// 与最终候选使用的真实静态 Logo 相同；独立 ID 不复用别的测试缓存。
fn logo() -> MonitorImageSource {
    MonitorImageSource {
        id: uuid::Uuid::new_v4().to_string(),
        bytes: Arc::from(include_bytes!("../../assets/logo.png").as_slice()),
        format: loki_metris_core::ImageFormat::Png,
    }
}

/// 平台只能在收到真实 wake 后送一帧；无需求时立即停止，不主动失效视图。
fn deliver_requested_frames(
    cx: &mut TestAppContext,
    delivered: &mut usize,
    requested: impl Fn() -> usize,
    deliver: impl Fn(),
) {
    for _ in 0..8 {
        cx.run_until_parked();
        let wakes = requested();
        if wakes == *delivered { return; }
        *delivered = wakes;
        deliver();
    }
    panic!("静态图片不得持续请求平台帧");
}

/// 后台真实解码由许可延迟；首图必须自行唤醒第二次投影，不能等鼠标或别的动画。
#[gpui_kit::test]
fn pending_static_logo_paints_using_only_requested_frames(cx: &mut TestAppContext) {
    cx.use_platform_frame_requests();
    let source = logo();
    let probe = LoadProbe::new(&source.id);
    let permits = cx.update(hold_decoders_for_test);
    let handle = cx.add_window(move |window, cx| {
        let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx);
        view.set_mounted(true, window, cx);
        view
    });
    let entity = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    let platform = cx.test_window(handle.into());
    let mut delivered = 0;
    deliver_requested_frames(cx, &mut delivered, || platform.frame_wake_count(),
        || platform.simulate_frame_request(RequestFrameOptions::default()));
    assert!(!entity.read_with(cx, |view, _| view.is_decoded()));
    assert_eq!(probe.started(), 1, "必须沿正式 Asset 进入等待许可的解码任务");
    drop(permits);
    deliver_requested_frames(cx, &mut delivered, || platform.frame_wake_count(),
        || platform.simulate_frame_request(RequestFrameOptions::default()));
    assert!(entity.read_with(cx, |view, _| view.is_decoded()));
    handle.update(cx, |_, window, _| {
        assert!(!window.painted_polychrome_sprites().is_empty(),
            "静态 Logo 解码后须自行绘出首帧，不能依赖强制 render_frame 或后续输入");
    }).unwrap();
    assert_eq!(probe.started(), 1, "首次 ready 通知不能重新启动解码");
    assert!(entity.read_with(cx, |view, _| view.task.is_none() && view.finished),
        "单帧图片不应持有播放或轮询 timer");
    let idle_wakes = platform.frame_wake_count();
    cx.run_until_parked();
    assert_eq!(platform.frame_wake_count(), idle_wakes);
}

/// 在子画布接纳 Ready 后、绘制栈退出前，驱动真实的卸载／暂停／关闭边界。
#[derive(Clone, Copy)]
enum Cancel {
    /// 卸载视图。
    Unmount,
    /// 开始新的播放代次。
    NewGeneration,
    /// 关闭窗口。
    Close,
}

/// 在同一真实绘制栈内撤销已排队的首帧通知，保留计数以核对没有迟到回执。
struct CancelOnReady {
    image: gpui_kit::Entity<MonitorImageView>,
    action: Cancel,
    applied: Rc<Cell<bool>>,
    notices: Rc<Cell<usize>>,
    notices_at_cancel: Rc<Cell<usize>>,
    _observer: Subscription,
}

impl Render for CancelOnReady {
    /// 后续画布只改变生命周期，不调用解码器或主动刷新，首图仍沿生产 prepaint 读取。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let image = self.image.clone();
        let applied = self.applied.clone();
        let notices = self.notices.clone();
        let at_cancel = self.notices_at_cancel.clone();
        let action = self.action;
        div().size_full()
            .when(!applied.get() || matches!(action, Cancel::NewGeneration), |root| root.child(image.clone()))
            .child(canvas(move |_, window, cx| {
                if !applied.get() && image.read(cx).is_decoded() {
                    applied.set(true);
                    at_cancel.set(notices.get());
                    image.update(cx, |view, cx| match action {
                        Cancel::Unmount => view.set_mounted(false, window, cx),
                        Cancel::NewGeneration => view.pause(cx),
                        Cancel::Close => window.remove_window(),
                    });
                }
            }, |_, _, _, _| {}).absolute().inset_0())
    }
}

/// 真实完成通知可计数，取消后的同一通知不得晚到；无人工多轮绘制或解码结果注入。
fn cancelled_first_frame(action: Cancel, cx: &mut TestAppContext) {
    cx.use_platform_frame_requests();
    let source = logo();
    let source_for_view = source.clone();
    let notices = Rc::new(Cell::new(0));
    let applied = Rc::new(Cell::new(false));
    let notices_at_cancel = Rc::new(Cell::new(usize::MAX));
    let (observed, cancelled, at_cancel) = (notices.clone(), applied.clone(), notices_at_cancel.clone());
    let permits = cx.update(hold_decoders_for_test);
    let handle = cx.add_window(move |window, cx| {
        let image = cx.new(|cx| MonitorImageView::new(source_for_view, ObjectFit::Contain, window, cx));
        image.update(cx, |view, cx| view.set_mounted(true, window, cx));
        let counter = observed.clone();
        let observer = cx.observe(&image, move |_, _, _| counter.set(counter.get() + 1));
        CancelOnReady { image, action, applied: cancelled, notices: observed,
            notices_at_cancel: at_cancel, _observer: observer }
    });
    let entity = handle.update(cx, |root, _, _| root.image.clone()).unwrap();
    let platform = cx.test_window(handle.into());
    let mut delivered = 0;
    deliver_requested_frames(cx, &mut delivered, || platform.frame_wake_count(),
        || platform.simulate_frame_request(RequestFrameOptions::default()));
    drop(permits);
    deliver_requested_frames(cx, &mut delivered, || platform.frame_wake_count(),
        || platform.simulate_frame_request(RequestFrameOptions::default()));
    assert!(applied.get(), "必须在正式 prepaint 接纳真实解码之后才取消");
    assert_eq!(notices.get(), notices_at_cancel.get(), "取消之后不能继续交付首帧通知");
    if matches!(action, Cancel::NewGeneration) {
        assert!(entity.read_with(cx, |view, _| view.mounted && view.element_visible && view.is_decoded()));
    } else {
        assert!(!entity.read_with(cx, |view, _| view.is_decoded()));
        assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&source)));
    }
    if matches!(action, Cancel::Close) { assert!(!cx.windows().contains(&handle.into())); }
}

/// 刚读回首图便被卸载的位置，不能由迟到通知复活像素或资源需求。
#[gpui_kit::test]
fn unmount_cancels_queued_static_first_frame_notice(cx: &mut TestAppContext) {
    cancelled_first_frame(Cancel::Unmount, cx);
}

/// 位置仍可见而代次前进时，旧首帧通知也必须失效。
#[gpui_kit::test]
fn newer_playback_generation_cancels_queued_static_notice(cx: &mut TestAppContext) {
    cancelled_first_frame(Cancel::NewGeneration, cx);
}

/// 原生窗关闭后，即使测试仍保留实体，延期通知也不能继续拥有像素。
#[gpui_kit::test]
fn closing_window_cancels_queued_static_first_frame_notice(cx: &mut TestAppContext) {
    cancelled_first_frame(Cancel::Close, cx);
}
