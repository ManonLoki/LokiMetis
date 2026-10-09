//! 真实 Shell 的两个提交按钮使用历史按钮尺寸，而非组件内层默认字号。

use super::super::state::Route;
use gpui_kit::{
    AppContext, FontWeight, Role, SharedString, TestAppContext, TextRun, px, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, public_monitor_ai_tools};

/// 原生文字排版计算固有宽度，断言实际 border-box 而不读取适配器配置。
fn text_width(window: &gpui_kit::Window, text: &str, size: f32) -> gpui_kit::Pixels {
    let mut font = gpui_kit::font("Geist");
    font.weight = FontWeight::SEMIBOLD;
    let text = SharedString::from(text.to_owned());
    let run = TextRun {
        len: text.len(),
        font,
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(text, px(size), &[run], None)
        .width
}

/// 保存按钮有独立图标与真实三十六像素边界，不能依赖默认三十二像素 Button。
#[gpui_kit::test]
fn monitor_save_button_keeps_historical_border_box(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Management;
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find("monitor-profile-save");
        assert_eq!(button.role(), Some(Role::Button));
        assert_eq!(
            button.label(),
            Some(shell.read(cx).t("monitor.management.save").as_str())
        );
        assert_eq!(button.bounds().size.height, px(36.));
        let icon = gpui_kit::base::test_support::find(
            window,
            button.path(),
            &"historical-button-icon".into(),
        )
        .unwrap();
        assert_eq!(icon.bounds().size.width, px(17.));
        assert_eq!(icon.bounds().size.height, px(17.));
        let expected =
            text_width(window, &shell.read(cx).t("monitor.management.save"), 14.) + px(59.);
        assert!(
            (button.bounds().size.width - expected).abs() <= px(1.),
            "实际保存按钮宽度必须包含17px图标、10px间距和12/18px内边距：{:?} vs {:?}",
            button.bounds(),
            expected
        );
    })
    .unwrap();
    shell.update(cx, |this, cx| {
        this.monitor.catalog.saving = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find("monitor-profile-save");
        let loader = gpui_kit::base::test_support::find(
            window,
            button.path(),
            &"historical-button-loader".into(),
        )
        .unwrap();
        assert_eq!(loader.bounds().size.width, px(20.));
        assert!((loader.bounds().center().x - button.bounds().center().x).abs() <= px(0.51));
    })
    .unwrap();
}
