//! 通过现有 Kit 观察真实设置标题的辅助属性；静态文字的平台树由 Computer Use 验收。

use gpui_kit::{AppContext, Role, TestAppContext, test::TestWindowExt};

/// 设置章节随语言更新辅助名称，标题没有焦点能力且保持现有文字布局。
#[gpui_kit::test]
fn translated_setting_headings_expose_native_names_without_focus(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    for (locale, labels) in [
        (
            "en-US",
            [
                "Application",
                "Interface language",
                "Appearance",
                "Enabled Agents",
                "Sponsor LokiMetris",
            ],
        ),
        (
            "zh-CN",
            [
                "应用信息",
                "界面语言",
                "外观主题",
                "启用的 Agent",
                "赞助支持",
            ],
        ),
    ] {
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.locale = locale;
                this.navigate(crate::app::Page::Settings, cx);
                cx.notify();
            });
            window.render_frame(cx);
            for (id, label) in [
                "settings-heading-settings.application",
                "settings-heading-settings.language",
                "settings-heading-settings.theme_title",
                "settings-heading-agents",
                "settings-heading-settings.sponsor_title",
            ]
            .into_iter()
            .zip(labels)
            {
                let heading = window.find(id);
                assert_eq!(heading.role(), Some(Role::Heading));
                assert_eq!(heading.label(), Some(label));
                assert_eq!(heading.focused(), None);
                assert!(heading.bounds().size.width > gpui_kit::px(0.));
                assert!(heading.bounds().size.height > gpui_kit::px(0.));
            }
        })
        .expect("读取真实设置标题与辅助名称");
    }
}
