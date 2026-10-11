//! 用真实编辑器和 Metal 验证颜色投影；字形遮罩固定，仅隔离颜色通路，不证明 CoreText 字形一致。

use crate::{
    app::{Page, Shell, monitor::state::Route},
    preferences::{LanguageChoice, Preferences, ThemeChoice, Writer},
};
use gpui_kit::{
    AppContext, Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId,
    HeadlessAppContext, LineLayout, NoopTextSystem, Pixels, PlatformTextSystem, RenderGlyphParams,
    Size, TextRenderingMode, WindowBounds, WindowOptions, component::Theme, point, px, rgb, size,
    test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, public_monitor_ai_tools};
use std::{borrow::Cow, sync::Arc};

/// 固定单色字形遮罩，使测试只观察真实 TextElement 传给 GPU 的颜色，而非字体抗锯齿差异。
struct InkMask;
impl PlatformTextSystem for InkMask {
    /// 字体资源解析留给生产字体测试，此处仅使用确定性形状。
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui_kit::Result<()> {
        NoopTextSystem.add_fonts(fonts)
    }
    /// 测试不枚举或注册系统字体。
    fn all_font_names(&self) -> Vec<String> {
        NoopTextSystem.all_font_names()
    }
    /// 复用 Noop 的固定字体身份。
    fn font_id(&self, descriptor: &Font) -> gpui_kit::Result<FontId> {
        NoopTextSystem.font_id(descriptor)
    }
    /// 固定度量与现有 native 布局测试一致。
    fn font_metrics(&self, id: FontId) -> FontMetrics {
        NoopTextSystem.font_metrics(id)
    }
    /// 排版范围保持 Noop 的公开契约。
    fn typographic_bounds(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Bounds<f32>> {
        NoopTextSystem.typographic_bounds(id, glyph)
    }
    /// 字符间距保持 Noop 的公开契约。
    fn advance(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Size<f32>> {
        NoopTextSystem.advance(id, glyph)
    }
    /// 测试仅输入普通 Latin 字符，不依赖 emoji 的彩色纹理路径。
    fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
        NoopTextSystem.glyph_for_char(id, ch)
    }
    /// 提供可读回的非空固定矩形遮罩，避免 Noop 空字形令颜色断言虚通过。
    fn glyph_raster_bounds(&self, _: &RenderGlyphParams) -> gpui_kit::Result<Bounds<DevicePixels>> {
        Ok(Bounds::new(
            point(DevicePixels(0), DevicePixels(-8)),
            size(DevicePixels(4), DevicePixels(8)),
        ))
    }
    /// 完全覆盖的灰度遮罩使实际 sprite 颜色可以按原字节精确断言。
    fn rasterize_glyph(
        &self,
        _: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> gpui_kit::Result<(Size<DevicePixels>, Vec<u8>)> {
        Ok((
            bounds.size,
            vec![255; (bounds.size.width.0 * bounds.size.height.0) as usize],
        ))
    }
    /// 内容、换行和真实编辑器仍走同一公开排版 API。
    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        NoopTextSystem.layout_line(text, font_size, runs)
    }
    /// 此测试只验证单色 glyph 的实际 Metal 着色。
    fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }
}

/// 成功与 panic 均清理测试自己的偏好队列，不访问实际用户配置。
struct Finish(Writer);
impl Drop for Finish {
    /// 队列在测试退出时有界完成。
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// 在真实输入内容区域搜索精确颜色；无 glyph 或错误 placeholder 色都会失败。
fn contains(image: &image::RgbaImage, bounds: Bounds<Pixels>, color: [u8; 4]) -> bool {
    let scale = image.width() as f32 / 1440.;
    let left = (f32::from(bounds.left()) * scale).ceil() as u32;
    let top = (f32::from(bounds.top()) * scale).ceil() as u32;
    let right = (f32::from(bounds.right()) * scale).floor() as u32;
    let bottom = (f32::from(bounds.bottom()) * scale).floor() as u32;
    (top..bottom).any(|y| (left..right).any(|x| image.get_pixel(x, y).0 == color))
}

/// 完整生产 Shell 的 placeholder、正文、选区和 caret 都必须保持各自真实颜色通路。
#[test]
fn shell_textarea_projects_placeholder_selection_and_caret_on_metal() {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(InkMask),
        Arc::new(crate::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(gpui_kit::init);
    let preferences = Preferences {
        language: LanguageChoice::English,
        theme: ThemeChoice::Light,
        ..Default::default()
    };
    let writer = Writer::in_memory(preferences.clone());
    let _finish = Finish(writer.clone());
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1440.), px(1400.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut shell = Shell::new(preferences, writer, window, cx);
                    shell.agent_settings.value = Some(crate::monitor::settings::MonitorSettings {
                        enabled_ai_tools: public_monitor_ai_tools().collect(),
                        ..Default::default()
                    });
                    shell.page = Page::Monitor;
                    shell.monitor.route = Route::Management;
                    shell.monitor.catalog.accept_profiles(
                        public_monitor_ai_tools()
                            .map(AiProfileDraft::default_for)
                            .collect(),
                    );
                    shell.monitor.catalog.images_loaded = true;
                    shell
                })
            },
        )
        .unwrap()
    });
    // Headless 的平台窗口初始 inactive；真实 caret/selection 只在激活窗口绘制。
    cx.update_window(handle, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    for (theme, placeholder, text) in [
        (ThemeChoice::Light, [173, 181, 189, 255], [0, 0, 0, 255]),
        (
            ThemeChoice::Dark,
            [105, 105, 105, 255],
            [201, 201, 201, 255],
        ),
    ] {
        let (input, bounds) = cx
            .update_window(handle, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.preferences.theme = theme;
                    shell.apply_theme(window, cx);
                    cx.notify();
                });
                window.render_frame(cx);
                let input = shell.read(cx).monitor.catalog.editors[0]
                    .inputs
                    .as_ref()
                    .unwrap()[0]
                    .clone();
                input.update(cx, |input, cx| input.set_value("", window, cx));
                window.blur(cx);
                window.render_frame(cx);
                (input.clone(), input.read(cx).text_bounds().unwrap())
            })
            .unwrap();
        let empty = cx
            .capture_screenshot(handle)
            .expect("实际 Metal placeholder 读回");
        assert!(
            contains(&empty, bounds, placeholder),
            "实际编辑器 placeholder 未投影历史颜色 {placeholder:?}"
        );
        cx.update_window(handle, |_, window, cx| {
            input.update(cx, |input, cx| input.set_value("MMMM", window, cx));
            window.render_frame(cx);
        })
        .unwrap();
        let filled = cx.capture_screenshot(handle).expect("实际 Metal 正文读回");
        assert!(
            contains(&filled, bounds, text),
            "正文色不能被 placeholder 投影覆盖"
        );
        cx.update_window(handle, |_, window, cx| {
            Theme::update(cx, |theme| {
                theme.colors.caret = rgb(0x14facc).into();
                theme.colors.selection = rgb(0xee33a5).into();
            });
            input.update(cx, |input, cx| input.focus(window, cx));
            window.render_frame(cx);
            let caret = input.read(cx).cursor_layout().expect("真实原生 caret").0;
            assert!(
                window.painted_quads().iter().any(|quad| {
                    let expected = caret.scale(window.scale_factor());
                    // paint_quad 会把每条边独立吸附到设备像素，最多改变一个设备像素。
                    let delta =
                        |a: gpui_kit::ScaledPixels, b: gpui_kit::ScaledPixels| (a - b).0.abs() < 1.;
                    delta(quad.bounds.left(), expected.left())
                        && delta(quad.bounds.right(), expected.right())
                        && delta(quad.bounds.top(), expected.top())
                        && delta(quad.bounds.bottom(), expected.bottom())
                        && quad.background.as_solid() == Some(rgb(0x14facc).into())
                }),
                "完整编辑器投影必须保留当前 caret token，caret={caret:?}，quads={:?}",
                window
                    .painted_quads()
                    .into_iter()
                    .filter(|quad| quad.bounds.intersects(&bounds.scale(window.scale_factor())))
                    .map(|quad| (quad.bounds, quad.background))
                    .collect::<Vec<_>>()
            );
            window.press("cmd-a", cx);
            assert_eq!(input.read(cx).selected_range(), 0..4);
        })
        .unwrap();
        let selected = cx.capture_screenshot(handle).expect("实际 Metal 选区读回");
        assert!(
            contains(&selected, bounds, [238, 51, 165, 255]),
            "完整编辑器投影必须保留当前 selection token"
        );
    }
}
