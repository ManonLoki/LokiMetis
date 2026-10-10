//! 主窗口标题请求属于每个壳层；稳定绘制无需重复进入原生平台。

impl super::Shell {
    /// 版本和支持信息在进程内固定；只有已确认的有效语言改变完整标题。
    pub(super) fn request_window_title(&mut self, window: &mut gpui_kit::Window) {
        if self.last_requested_title_locale == Some(self.locale) {
            return;
        }
        window.set_window_title(&format!("{} {} QQ:2222980", self.name(), self.version()));
        // GPUI setter 没有宿主成功回执，这里只记录已请求的语言。
        self.last_requested_title_locale = Some(self.locale);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::open_shell;
    use crate::preferences::{LanguageChoice, PreferencePatch};
    use gpui_kit::{
        AnyWindowHandle, AppContext, TestAppContext, VisualTestContext, test::TestWindowExt,
    };
    use std::time::Duration;

    /// 真实 TestWindow 的标题读回，与产品 setter 使用同一平台入口。
    fn title(handle: AnyWindowHandle, cx: &mut TestAppContext) -> Option<String> {
        VisualTestContext::from_window(handle, cx).window_title()
    }

    /// 强制根视图再次绘制，避免静态缓存让测试没有经过 Shell.render。
    fn draw(handle: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .expect("绘制真实壳层");
        cx.run_until_parked();
    }

    /// 完整标题必须保留正式名称、编译期版本及支持信息。
    fn expected(name: &str) -> String {
        format!("{name} v{} QQ:2222980", env!("CARGO_PKG_VERSION"))
    }

    /// 哨兵由真实 setter 写入；旧的逐帧请求会覆盖它，不能用内部字段冒充去重。
    #[gpui_kit::test]
    fn repeated_draws_do_not_reset_title_and_new_shell_initializes_its_own(
        cx: &mut TestAppContext,
    ) {
        let (handle, _, _finish) = open_shell(cx, 1440., 900.);
        draw(handle, cx);
        assert_eq!(title(handle, cx), Some(expected("LokiMetris")));
        cx.update_window(handle, |_, window, _| {
            window.set_window_title("stable-title-probe")
        })
        .unwrap();
        for _ in 0..4 {
            draw(handle, cx);
        }
        assert_eq!(title(handle, cx).as_deref(), Some("stable-title-probe"));

        let (second, _, _second_finish) = open_shell(cx, 1440., 900.);
        draw(second, cx);
        assert_eq!(title(second, cx), Some(expected("LokiMetris")));
        assert_eq!(title(handle, cx).as_deref(), Some("stable-title-probe"));
    }

    /// 保存回执到达前不改标题，成功后更新完整身份，之后稳定绘制再次去重。
    #[gpui_kit::test]
    fn language_title_changes_only_after_successful_receipt(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
        draw(handle, cx);
        let writer = shell.read_with(cx, |shell, _| shell.writer.clone());
        let pause = writer.pause_worker_for_test();
        cx.update_window(handle, |_, window, app| {
            shell.update(app, |shell, cx| {
                shell.select_language(LanguageChoice::Chinese, window, cx)
            });
        })
        .unwrap();
        draw(handle, cx);
        assert_eq!(title(handle, cx), Some(expected("LokiMetris")));
        assert_eq!(
            shell.read_with(cx, |shell, _| shell.preference_save.pending),
            Some(PreferencePatch::Language(LanguageChoice::Chinese))
        );
        drop(pause);
        writer.finish();
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        draw(handle, cx);
        assert_eq!(shell.read_with(cx, |shell, _| shell.locale), "zh-CN");
        assert_eq!(title(handle, cx), Some(expected("诡秘神谕")));
        cx.update_window(handle, |_, window, _| {
            window.set_window_title("confirmed-title-probe")
        })
        .unwrap();
        for _ in 0..4 {
            draw(handle, cx);
        }
        assert_eq!(title(handle, cx).as_deref(), Some("confirmed-title-probe"));
    }

    /// 实际 Writer 拒绝新提交后，失败提示不会改变已确认语言和标题。
    #[gpui_kit::test]
    fn rejected_language_write_keeps_confirmed_title(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
        draw(handle, cx);
        shell
            .read_with(cx, |shell, _| shell.writer.clone())
            .finish();
        cx.update_window(handle, |_, window, app| {
            shell.update(app, |shell, cx| {
                shell.select_language(LanguageChoice::Chinese, window, cx)
            });
        })
        .unwrap();
        draw(handle, cx);
        assert_eq!(
            shell.read_with(cx, |shell, _| shell.preference_save.failed),
            Some(PreferencePatch::Language(LanguageChoice::Chinese))
        );
        assert_eq!(shell.read_with(cx, |shell, _| shell.locale), "en-US");
        assert_eq!(title(handle, cx), Some(expected("LokiMetris")));
    }
}
