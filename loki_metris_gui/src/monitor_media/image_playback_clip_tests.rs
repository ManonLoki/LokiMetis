//! 在真实 Metal 测试窗口读回生产播放器像素，覆盖小留白与原视口圆心。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Asset, Empty, Entity, HeadlessAppContext, NoopTextSystem, px,
    test::TestWindowExt,
};

/// 用真实 GIF 编解码形成两帧，尺寸使 Contain 留白四像素、小于圆角半径。
fn gif_source() -> MonitorImageSource {
    let mut bytes = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        encoder
            .encode_frames(
                [[255, 255, 255, 255], [0, 255, 0, 255]]
                    .into_iter()
                    .map(|color| {
                        image::Frame::from_parts(
                            image::RgbaImage::from_pixel(92, 100, image::Rgba(color)),
                            0,
                            0,
                            image::Delay::from_numer_denom_ms(200, 1),
                        )
                    }),
            )
            .unwrap();
    }
    MonitorImageSource {
        id: "rounded-contain-gif".into(),
        bytes: Arc::from(bytes),
        format: loki_metris_core::ImageFormat::Gif,
    }
}

/// 独立测试平台使用真实 Metal atlas，不创建 AppKit 窗口或读取用户图片。
fn player() -> (
    HeadlessAppContext,
    AnyWindowHandle,
    Entity<MonitorImageView>,
    Arc<DecodedMonitorImage>,
    image::RgbaImage,
) {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem),
        Arc::new(()),
        gpui_kit::platform::current_headless_renderer,
    );
    let source = gif_source();
    cx.allow_parking();
    let pending = cx.update(|app| {
        let pending: futures::future::BoxFuture<
            'static,
            Result<Arc<DecodedMonitorImage>, &'static str>,
        > = Box::pin(MonitorImageDecoder::load(source.clone(), app));
        pending
    });
    let decoded = cx
        .foreground_executor()
        .block_with_timeout(Duration::from_secs(5), pending)
        .ok()
        .expect("真实 GIF 解码不能超过测试时限")
        .expect("两帧 GIF 必须成功解码");
    cx.forbid_parking();
    assert_eq!(decoded.image.frame_count(), 2);
    assert_eq!(decoded.natural_size.width.0, 92);
    assert_eq!(decoded.image.size(0).width.0, 94);
    assert_eq!(decoded.image.size(0).height.0, 102);
    let handle: AnyWindowHandle = cx
        .open_window(size(px(100.), px(100.)), |_, app| app.new(|_| Empty))
        .unwrap()
        .into();
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    // Metal Headless 默认不透明清屏；同窗无图基线才能区分背景与图片像素。
    let baseline = cx.capture_screenshot(handle).expect("无图基线必须真实读回");
    let image = decoded.clone();
    let entity = cx
        .update_window(handle, |_, window, app| {
            window.replace_root(app, |window, cx| {
                let mut view = MonitorImageView::new(source, ObjectFit::Contain, window, cx)
                    .with_viewport_corners(Corners::all(px(12.)));
                view.timeline = Some(Timeline::new(&image));
                view.decoded = Some(image);
                view.mounted = true;
                view
            })
        })
        .unwrap();
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    cx.run_until_parked();
    (cx, handle, entity, decoded, baseline)
}

/// 像素采样以逻辑坐标定位，避免把测试平台的二倍缩放混入圆角断言。
fn sample(image: &image::RgbaImage, x: f32, y: f32) -> [u8; 4] {
    let scale = image.width() as f32 / 100.;
    image.get_pixel((x * scale) as u32, (y * scale) as u32).0
}

/// 圆角以原视口为基准；对拟合图或保护边缘自身圆角都会误删左上内部像素。
#[test]
fn contain_rounded_clip_preserves_viewport_circle_and_guard() {
    let (mut cx, handle, entity, decoded, baseline) = player();
    let image = cx
        .capture_screenshot(handle)
        .expect("必须真实执行 Metal 读回");
    assert_eq!(
        sample(&baseline, 50.25, 50.25),
        [0, 0, 0, 255],
        "必须先证明真实 Headless 清屏底色"
    );
    assert_eq!(
        sample(&image, 4.25, 0.25),
        sample(&baseline, 4.25, 0.25),
        "原视口角外必须与无图窗口基线一致"
    );
    assert_eq!(
        sample(&image, 5.25, 4.25),
        [255; 4],
        "原视口角内不能被拟合图自己的圆角误裁"
    );
    assert_eq!(
        sample(&image, 4.25, 50.25),
        [255; 4],
        "保护边缘不能漏出 atlas 邻块或透明缝"
    );
    assert_eq!(
        sample(&image, 2.25, 50.25),
        sample(&baseline, 2.25, 50.25),
        "Contain 留白不能被拉伸填满"
    );
    assert!(entity.read_with(&cx, |view, _| Arc::ptr_eq(
        view.decoded.as_ref().unwrap(),
        &decoded
    )));
}

/// 原生圆角绘制继续使用同一解码 owner 和真实第二帧，不把动画截图静态化。
#[test]
fn rounded_viewport_advances_the_same_gif_owner_to_second_frame() {
    let (mut cx, handle, entity, decoded, baseline) = player();
    let first = cx.capture_screenshot(handle).expect("首帧必须真实读回");
    assert_eq!(sample(&first, 50.25, 50.25), [255; 4]);
    cx.advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    let second = cx.capture_screenshot(handle).expect("第二帧必须真实读回");
    assert_eq!(sample(&second, 50.25, 50.25), [0, 255, 0, 255]);
    assert_eq!(
        sample(&second, 4.25, 0.25),
        sample(&baseline, 4.25, 0.25),
        "第二帧角外仍必须与同窗无图基线一致"
    );
    assert_eq!(entity.read_with(&cx, |view, _| view.frame), 1);
    assert!(entity.read_with(&cx, |view, _| Arc::ptr_eq(
        view.decoded.as_ref().unwrap(),
        &decoded
    )));
}
