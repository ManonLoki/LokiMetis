//! 实际 Metal 像素验证白色三侧圆环与1.2s线性旋转，不以测试平台图元假称GPU通过。

use super::*;
use gpui_kit::{
    AppContext, Context, HeadlessAppContext, NoopTextSystem, Render, div, prelude::*, size,
    test::TestWindowExt,
};
use std::{sync::Arc, time::Duration};

/// 只画一个真实loading控件，避免其它图形污染圆环读回。
struct Probe;
impl Render for Probe {
    /// 初始loading立即采用最终布局，动画只改变圆环方向。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(20.)).child(
            SurfaceButton::new("oval-probe", SurfaceButtonVariant::FilledBrand, false)
                .historical_size(HistoricalButtonSize::XSmall)
                .loading(true)
                .label("Write"),
        )
    }
}

/// 在同一原生渲染器里推进真实时钟并读回，四分之一圈与整圈必须分别不同和相同。
#[test]
fn historical_oval_is_white_and_rotates_on_actual_metal_clock() {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem),
        Arc::new(crate::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let handle = cx
        .open_window(size(px(180.), px(100.)), |_, app| app.new(|_| Probe))
        .unwrap()
        .into();
    let bounds = cx
        .update_window(handle, |_, window, app| {
            window.render_frame(app);
            let button = window.find("oval-probe");
            gpui_kit::base::test_support::find(
                window,
                button.path(),
                &"historical-button-loader".into(),
            )
            .unwrap()
            .bounds()
        })
        .unwrap();
    let sample = |image: &image::RgbaImage, x: f32, y: f32| {
        let scale = image.width() as f32 / 180.;
        image.get_pixel((x * scale) as u32, (y * scale) as u32).0
    };
    let x = f32::from(bounds.origin.x) + 1.25;
    let y = f32::from(bounds.center().y);
    let first = cx.capture_screenshot(handle).expect("实际Metal首帧读回");
    assert_ne!(sample(&first, x, y), [255; 4], "初始透明侧位于圆环左边");
    assert_eq!(
        sample(
            &first,
            f32::from(bounds.center().x),
            f32::from(bounds.origin.y) + 1.25
        ),
        [255; 4],
        "Loader白色不继承Kit整按钮0.8透明度"
    );
    cx.advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    let quarter = cx
        .capture_screenshot(handle)
        .expect("实际Metal四分之一圈读回");
    assert_eq!(
        sample(&quarter, x, y),
        [255; 4],
        "300ms线性旋转90度后左侧必须变为可见圆环"
    );
    cx.advance_clock(Duration::from_millis(900));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    let full = cx.capture_screenshot(handle).expect("实际Metal整圈读回");
    assert_eq!(
        full.as_raw(),
        first.as_raw(),
        "1200ms整圈必须返回相同真实像素"
    );
}
