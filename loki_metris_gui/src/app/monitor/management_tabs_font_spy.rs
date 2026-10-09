//! 观察真实 CoreText 接收及返回的 FontId；所有字体注册、度量和字形算法仍由原生系统执行。

use gpui_kit::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, Hsla, LineLayout,
    MissingGlyphSink, Pixels, PlatformTextSystem, RenderGlyphParams, Size, TextRenderingMode,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// 只保留消费者实际送入原生排版的行，不根据预期字重制造排版结果。
#[derive(Clone, Debug)]
pub(super) struct ObservedLine {
    pub text: String,
    pub size: Pixels,
    pub input: Vec<FontId>,
    pub output: Vec<FontId>,
    pub width: Pixels,
}

/// 委托同一个 macOS 文字系统，记录原生身份与消费者排版之间的关联。
pub(super) struct NativeFontSpy {
    pub native: Arc<dyn PlatformTextSystem>,
    fonts: Mutex<HashMap<FontId, Font>>,
    lines: Mutex<Vec<ObservedLine>>,
}

impl NativeFontSpy {
    /// 包装真实平台实例，不替换它的字重匹配或字形度量。
    pub fn new(native: Arc<dyn PlatformTextSystem>) -> Self {
        Self {
            native,
            fonts: Mutex::new(HashMap::new()),
            lines: Mutex::new(Vec::new()),
        }
    }
    /// 切换页面之前只清空观察记录，保留真实字体和 GPUI 排版缓存。
    pub fn clear_lines(&self) {
        self.lines.lock().unwrap().clear();
    }
    /// 返回本轮真实排版的独立快照，断言期间不持有委托器锁。
    pub fn lines(&self) -> Vec<ObservedLine> {
        self.lines.lock().unwrap().clone()
    }
    /// 用实际原生 FontId 反查调用方提交的完整字体描述。
    pub fn font(&self, id: FontId) -> Font {
        self.fonts
            .lock()
            .unwrap()
            .get(&id)
            .expect("实际排版身份必须先被解析")
            .clone()
    }
}

impl PlatformTextSystem for NativeFontSpy {
    /// 与生产 fonts.rs 共用原始字体字节及注册算法。
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui_kit::Result<()> {
        self.native.add_fonts(fonts)
    }
    /// 保留真实缺字通知通路。
    fn set_missing_glyph_sink(&self, sink: Option<Arc<dyn MissingGlyphSink>>) {
        self.native.set_missing_glyph_sink(sink);
    }
    /// 字体枚举仍由系统提供。
    fn all_font_names(&self) -> Vec<String> {
        self.native.all_font_names()
    }
    /// 先解析真实字体身份，再记录请求字重，绝不自造 FontId。
    fn font_id(&self, descriptor: &Font) -> gpui_kit::Result<FontId> {
        let id = self.native.font_id(descriptor)?;
        self.fonts.lock().unwrap().insert(id, descriptor.clone());
        Ok(id)
    }
    /// 原生预热策略不被观察器截断。
    fn prewarm_fonts(&self, ids: &[FontId]) {
        self.native.prewarm_fonts(ids);
    }
    /// 使用真实 CoreText 字体度量。
    fn font_metrics(&self, id: FontId) -> FontMetrics {
        self.native.font_metrics(id)
    }
    /// 字形外接矩形保持原生结果。
    fn typographic_bounds(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Bounds<f32>> {
        self.native.typographic_bounds(id, glyph)
    }
    /// 字符 advance 保持原生结果。
    fn advance(&self, id: FontId, glyph: GlyphId) -> gpui_kit::Result<Size<f32>> {
        self.native.advance(id, glyph)
    }
    /// 字符映射不经过 Noop 或固定字形夹具。
    fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
        self.native.glyph_for_char(id, ch)
    }
    /// 实际字形栅格边界来自同一个原生 FontId。
    fn glyph_raster_bounds(
        &self,
        params: &RenderGlyphParams,
    ) -> gpui_kit::Result<Bounds<DevicePixels>> {
        self.native.glyph_raster_bounds(params)
    }
    /// 保留真实字形栅格算法，不改尺寸、抗锯齿或像素。
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> gpui_kit::Result<(Size<DevicePixels>, Vec<u8>)> {
        self.native.rasterize_glyph(params, bounds)
    }
    /// 同时记录输入和真实 shaping 输出，覆盖原生 fallback 意外改换身份的情况。
    fn layout_line(&self, text: &str, size: Pixels, runs: &[FontRun]) -> LineLayout {
        let layout = self.native.layout_line(text, size, runs);
        self.lines.lock().unwrap().push(ObservedLine {
            text: text.into(),
            size,
            input: runs.iter().map(|run| run.font_id).collect(),
            output: layout.runs.iter().map(|run| run.font_id).collect(),
            width: layout.width,
        });
        layout
    }
    /// 灰度或次像素模式仍由原生系统选择。
    fn recommended_rendering_mode(&self, id: FontId, size: Pixels) -> TextRenderingMode {
        self.native.recommended_rendering_mode(id, size)
    }
    /// 保留原生按颜色选择的字形膨胀参数。
    fn glyph_dilation_for_color(&self, color: Hsla) -> u8 {
        self.native.glyph_dilation_for_color(color)
    }
}
