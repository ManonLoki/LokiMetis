//! 独立主线程提供真实CoreText；Metal读回必须同时包含字形、SVG和背景。

#[cfg(target_os = "macos")]
mod native {
    use gpui_kit::{
        App, AppContext, Bounds, Context, Element, ElementId, GlobalElementId,
        HeadlessAppContext, InspectorElementId, IntoElement, LayoutId, Pixels,
        Point, Render, RenderGlyphParams, Window, component::Icon, div, font,
        point, prelude::*, px, rgb, size, test::TestWindowExt,
    };
    use std::sync::Arc;

    /// 只包装绘制与命中，不改变祖先布局或字体参数。
    struct Scaled<E: Element> {
        /// 被变换的内层元素。
        inner: E,
        /// 绘制与命中的缩放比例。
        scale: f32,
        /// 绘制与命中的位移。
        shift: Point<Pixels>,
    }
    impl<E: Element> IntoElement for Scaled<E> {
        /// 自身即参与布局与绘制的元素，不再额外包装。
        type Element = Self;
        /// 包装层自身进入元素生命周期。
        fn into_element(self) -> Self { self }
    }
    impl<E: Element> Element for Scaled<E> {
        /// 沿用内层元素的布局状态。
        type RequestLayoutState = E::RequestLayoutState;
        /// 沿用内层元素的预绘制状态。
        type PrepaintState = E::PrepaintState;
        /// 真实控件ID继续由内部元素拥有。
        fn id(&self) -> Option<ElementId> { self.inner.id() }
        /// 保留原始检查器位置。
        fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
            self.inner.source_location()
        }
        /// 布局阶段完全沿用未缩放的内部元素。
        fn request_layout(
            &mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>,
            w: &mut Window, cx: &mut App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            self.inner.request_layout(id, inspector, w, cx)
        }
        /// 命中及辅助几何进入与绘制相同的变换。
        fn prepaint(
            &mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>,
            b: Bounds<Pixels>, r: &mut Self::RequestLayoutState, w: &mut Window, cx: &mut App,
        ) -> Self::PrepaintState {
            w.with_element_transform(b.center(), self.scale, self.shift, |w| {
                self.inner.prepaint(id, inspector, b, r, w, cx)
            })
        }
        /// 内部文字、图标和背景共享同一变换域。
        fn paint(
            &mut self, id: Option<&GlobalElementId>, inspector: Option<&InspectorElementId>,
            b: Bounds<Pixels>, r: &mut Self::RequestLayoutState, p: &mut Self::PrepaintState,
            w: &mut Window, cx: &mut App,
        ) {
            w.with_element_transform(b.center(), self.scale, self.shift, |w| {
                self.inner.paint(id, inspector, b, r, p, w, cx)
            });
        }
    }

    /// 各通道独占一种内容，便于在实际像素中区分字形、SVG和背景。
    struct Probe {
        /// 应用到内容的缩放比例。
        scale: f32,
    }
    impl Render for Probe {
        /// 父兄弟使用独立颜色，退出子域后仍必须逐像素不变。
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let panel = div().absolute().left(px(20.)).top(px(32.))
                .w(px(320.)).h(px(112.)).bg(rgb(0x0000ff))
                .child(Icon::default()
                    .data(&br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect x="2" y="4" width="28" height="24" fill="currentColor"/></svg>"#[..])
                    .absolute().left(px(16.)).top(px(28.))
                    .size(px(32.)).text_color(rgb(0x00ff00)))
                .child(div().absolute().left(px(64.)).top(px(24.))
                    .w(px(240.)).h(px(60.)).font_family("Helvetica").text_size(px(32.))
                    .text_color(rgb(0xff0000)).child("MMMMM"));
            div().relative().size_full().bg(rgb(0x101010))
                .child(div().absolute().left(px(0.)).top(px(0.))
                    .w(px(360.)).h(px(16.)).bg(rgb(0xff00ff)))
                .child(Scaled {
                    inner: panel.into_element(), scale: self.scale,
                    shift: point(px(0.), px(if self.scale == 1. { 0. } else { 1. })),
                })
                .child(div().absolute().left(px(0.)).top(px(160.))
                    .w(px(360.)).h(px(16.)).bg(rgb(0xffff00)))
        }
    }

    /// 无非空像素直接失败，禁止Noop字体或只有背景的截图假通过。
    fn color_bounds(image: &image::RgbaImage, channel: usize) -> [u32; 4] {
        let mut b = [u32::MAX, u32::MAX, 0, 0];
        let mut count = 0;
        for (x, y, pixel) in image.enumerate_pixels() {
            let c = pixel.0;
            if c[channel] > 80 && c[(channel + 1) % 3] < 32 && c[(channel + 2) % 3] < 32 {
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                count += 1;
            }
        }
        assert!(count > 8, "必须有真实非空颜色像素：channel={channel}");
        b
    }

    /// 独立进程main保证AppKit主线程身份，调度与截图仍使用确定性测试窗口。
    pub fn run() {
        let platform = gpui_kit::platform::current_platform(true);
        let text = platform.text_system();
        let font_id = text.font_id(&font("Helvetica")).expect("真实原生字体");
        let glyph_id = text.glyph_for_char(font_id, 'M').expect("真实M字形");
        let params = RenderGlyphParams {
            font_id, glyph_id, font_size: px(32.), subpixel_variant: point(0, 0),
            scale_factor: 1., is_emoji: false, subpixel_rendering: false, dilation: 0,
        };
        let bounds = text.glyph_raster_bounds(&params).unwrap();
        let (_, bytes) = text.rasterize_glyph(&params, bounds).unwrap();
        assert!(bytes.iter().any(|v| *v != 0), "禁止Noop字体系统假通过");

        let mut cx = HeadlessAppContext::with_platform(
            text, Arc::new(()), gpui_kit::platform::current_headless_renderer,
        );
        cx.update(gpui_kit::init);
        let probe = cx.new(|_| Probe { scale: 1. });
        let handle = cx.open_window(size(px(360.), px(196.)), |_, _| probe.clone()).unwrap().into();
        cx.update_window(handle, |_, w, app| w.render_frame(app)).unwrap();
        let first = cx.capture_screenshot(handle).expect("真实Metal基线");
        let baseline = [0, 1, 2].map(|c| color_bounds(&first, c));
        let device_scale = first.width() as f64 / 360.;
        let center = [180. * device_scale, 88. * device_scale];

        for scale in [0.985_f32, 0.5] {
            cx.update_entity(&probe, |p, cx| { p.scale = scale; cx.notify(); });
            cx.update_window(handle, |_, w, app| w.render_frame(app)).unwrap();
            let image = cx.capture_screenshot(handle).expect("真实Metal缩放");
            assert_ne!(image.as_raw(), first.as_raw());
            for (channel, base) in baseline.iter().enumerate().take(3) {
                let actual = color_bounds(&image, channel);
                for edge in 0..4 {
                    let axis = edge % 2;
                    let expected = (base[edge] as f64 + 0.5 - center[axis])
                        * scale as f64 + center[axis]
                        + if axis == 1 { device_scale } else { 0. } - 0.5;
                    assert!((actual[edge] as f64 - expected).abs() <= 2.,
                        "文字/图标/背景必须共同变换：scale={scale}, channel={channel}, edge={edge}, actual={}, expected={expected}", actual[edge]);
                }
                if scale == 0.5 {
                    assert!(actual[2] - actual[0] < (base[2] - base[0]) * 3 / 5,
                        "半比例负对照必须检测出内容仍按原尺寸绘制");
                }
            }
            for (x, y, p) in first.enumerate_pixels() {
                let dip_y = y as f64 / device_scale;
                if dip_y < 20. || dip_y >= 160. {
                    assert_eq!(*p, *image.get_pixel(x, y), "祖先及兄弟不得改变");
                }
            }
        }
        cx.update_entity(&probe, |p, cx| { p.scale = 1.; cx.notify(); });
        cx.update_window(handle, |_, w, app| w.render_frame(app)).unwrap();
        let released = cx.capture_screenshot(handle).unwrap();
        assert_eq!(released.as_raw(), first.as_raw(), "退出作用域并释放后像素完全恢复");
    }
}

#[cfg(target_os = "macos")]
/// 遵守Cargo枚举协议，真实测试仅在独立OS主线程执行。
fn main() {
    const NAME: &str = "native_text_icon_and_background_share_element_transform";
    if std::env::args().any(|value| value == "--list") {
        println!("{NAME}: test\n\n1 test, 0 benchmarks");
        return;
    }
    println!("running 1 test");
    native::run();
    println!("test {NAME} ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;");
}

#[cfg(not(target_os = "macos"))]
/// 非Metal宿主只报告不适用，不把未运行计作通过。
fn main() {
    if std::env::args().any(|value| value == "--list") { println!("0 tests, 0 benchmarks"); }
    else { println!("element_transform_metal: Not applicable (requires macOS Metal)"); }
}
