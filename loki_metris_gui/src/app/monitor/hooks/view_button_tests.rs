//! 实际 Hooks 页面内容与 writing 回执映射使用历史 xs 按钮合同。

use crate::app::monitor::state::Route;
use gpui_kit::{
    AppContext, FontWeight, Role, SharedString, TestAppContext, TextRun, px, test::TestWindowExt,
};

/// 用原生排版固有宽度核对实际内容层字号，不读取适配器配置。
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

/// Hooks xs 的十二像素文字必须到达实际内容层；根 text_size 不能被内部十四像素覆盖。
#[gpui_kit::test]
fn monitor_hook_write_button_keeps_xs_intrinsic_width(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let locations =
        loki_metris_service::monitor::store::list_hook_config_locations(&Default::default(), root.path());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor.hooks.accept_locations(locations);
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find("monitor-hook-write");
        assert_eq!(button.role(), Some(Role::Button));
        assert_eq!(button.bounds().size.height, px(30.));
        let expected =
            text_width(window, &shell.read(cx).t("monitor.settings.write"), 12.) + px(30.);
        assert!(
            (button.bounds().size.width - expected).abs() <= px(1.),
            "实际Hooks按钮必须用12px文字排版：{:?} vs {:?}",
            button.bounds(),
            expected
        );
    })
    .unwrap();
    shell.update(cx, |this, cx| {
        this.monitor.hooks.editors[0].writing = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(150));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let button = window.find("monitor-hook-write");
        let loader = gpui_kit::base::test_support::find(
            window,
            button.path(),
            &"historical-button-loader".into(),
        )
        .unwrap();
        assert!((loader.bounds().size.width - px(30. / 1.8)).abs() <= px(0.51));
        assert!((loader.bounds().center().x - button.bounds().center().x).abs() <= px(0.51));
    })
    .unwrap();
}

/// 三个目录动作沿用历史 xs 内容层，恢复默认保留独立 subtle 变体。
#[gpui_kit::test]
fn monitor_hook_directory_buttons_keep_historical_xs_width(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let locations =
        loki_metris_service::monitor::store::list_hook_config_locations(&Default::default(), root.path());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor.hooks.accept_locations(locations);
        this.monitor.hooks.editors[0].saved.is_custom = true;
        this.monitor.hooks.editors[0].draft.push_str("/edited");
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (id, key) in [
            ("monitor-hook-choose", "chooseDirectory"),
            ("monitor-hook-save", "saveDirectory"),
            ("monitor-hook-reset", "restoreDirectory"),
        ] {
            let button = window.find(id);
            assert_eq!(button.role(), Some(Role::Button));
            assert_eq!(button.bounds().size.height, px(30.));
            let expected = text_width(
                window,
                &shell.read(cx).t(&format!("monitor.settings.{key}")),
                12.,
            ) + px(30.);
            assert!(
                (button.bounds().size.width - expected).abs() <= px(1.),
                "实际目录动作必须用12px文字排版：{id} {:?} vs {expected:?}",
                button.bounds()
            );
        }
    })
    .unwrap();
}

/// 保存路径与恢复默认共用后台保存，loading 只属于当前已接受的那个操作。
#[gpui_kit::test]
fn monitor_hook_directory_loading_belongs_to_submitted_action(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let locations =
        loki_metris_service::monitor::store::list_hook_config_locations(&Default::default(), root.path());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor.hooks.accept_locations(locations);
        this.monitor.hooks.editors[0].saved.is_custom = true;
        this.monitor.hooks.editors[0].draft.push_str("/edited");
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for restore in [false, true] {
        shell.update(cx, |this, cx| {
            this.monitor.hooks.editors[0].saving = true;
            this.monitor.hooks.editors[0].saving_default = restore;
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(150));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for (id, loading) in [
                ("monitor-hook-save", !restore),
                ("monitor-hook-reset", restore),
            ] {
                let button = window.find(id);
                let label = gpui_kit::base::test_support::find(
                    window,
                    button.path(),
                    &"historical-button-label".into(),
                )
                .unwrap();
                assert_eq!(
                    label.visible(),
                    !loading,
                    "{id} 的真实标签应仅随自身操作隐藏"
                );
            }
        })
        .unwrap();
    }
}
