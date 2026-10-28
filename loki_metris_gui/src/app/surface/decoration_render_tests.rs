//! 真实Metal读回检查彩边、圆角和减少动态时保留的静态层级，不启动产品服务。

use super::*;
use gpui_kit::{AppContext, Context, HeadlessAppContext, IntoElement, NoopTextSystem, Render,
    canvas, div, prelude::*, size};
use std::sync::Arc;

/// 三档材质同屏绘制，只由测试控制光轨相位，屏幕读回仍执行生产绘制函数。
struct Probe {
    /// 测试主题。
    dark: bool,
    /// 主卡的确定性光轨相位。
    phase: f32,
}

impl Render for Probe {
    /// 真实边缘绘制放在媒体之后，验证媒体覆盖卡片时彩边仍然可见。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dark = self.dark;
        let phase = self.phase;
        div().size_full().bg(rgb(if dark { 0x111315 } else { 0xf3f1ed })).p(px(24.)).flex().gap(px(20.))
            .children([CardPriority::Primary, CardPriority::Secondary, CardPriority::Quiet].into_iter().map(move |priority| {
                canvas(|_, _, _| (), move |bounds, _, window, _| {
                    window.paint_quad(fill(bounds, rgb(if dark { 0x1e1c1a } else { 0xfffdf9 })).corner_radii(px(20.)));
                    paint_wash(bounds, px(20.), priority, dark, window);
                    let mut decoration = CardDecoration { phase, last_tick: None };
                    decoration.paint(bounds, px(20.), priority, dark, false, false, window);
                }).w(px(260.)).h(px(176.))
            }))
    }
}

/// 可选输出只写测试指定目录，便于检查真实GPU结果，不把截图当成候选验收。
fn save_preview(image: &image::RgbaImage, dark: bool, phase: u8) {
    if let Some(directory) = std::env::var_os("LOKI_METRIS_CARD_PREVIEW_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("创建卡片像素回归输出目录");
        image.save(directory.join(format!("cards-{}-{phase}.png", if dark { "dark" } else { "light" })))
            .expect("保存真实Metal卡片截图");
    }
}

/// 主卡相位改变边缘像素，次卡与辅助卡像素保持稳定，且卡片外与正文区不被流光污染。
#[test]
fn actual_metal_tracks_move_only_on_primary_edges_and_keep_corners_clear() {
    for dark in [false, true] {
        let mut cx = HeadlessAppContext::with_platform(Arc::new(NoopTextSystem),
            Arc::new(crate::assets::Assets), gpui_kit::platform::current_headless_renderer);
        cx.update(gpui_kit::init);
        let handle = cx.open_window(size(px(868.), px(224.)), |_, app| app.new(|_| Probe { dark, phase: 0. }))
            .expect("创建卡片Metal测试窗口");
        let first = cx.capture_screenshot(handle.into()).expect("读取初始彩边像素");
        save_preview(&first, dark, 0);
        cx.update_window(handle.into(), |view, _, app| {
            view.downcast::<Probe>().expect("卡片探针实体").update(app, |probe, cx| { probe.phase = 0.18; cx.notify(); });
        }).unwrap();
        let second = cx.capture_screenshot(handle.into()).expect("读取移动后的彩边像素");
        save_preview(&second, dark, 1);
        let scale = first.width() as f32 / 868.;
        let pixel = |image: &image::RgbaImage, x: f32, y: f32| image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
        let background = pixel(&first, 2., 2.);
        assert_eq!(pixel(&first, 24., 24.), background, "圆角之外不能漏光");
        assert_eq!(pixel(&second, 24., 24.), background);
        assert_eq!(pixel(&first, 150., 100.), pixel(&second, 150., 100.), "流光不能覆盖正文区");
        let mut primary_changed = 0;
        for y in 0..first.height() {
            for x in 0..first.width() {
                if first.get_pixel(x, y) != second.get_pixel(x, y) {
                    assert!((x as f32) < 284. * scale, "静止次卡和辅助卡不应跟随主卡改变");
                    primary_changed += 1;
                }
            }
        }
        assert!(primary_changed > 50, "主卡的原生光轨必须产生真实像素变化");
    }
}
