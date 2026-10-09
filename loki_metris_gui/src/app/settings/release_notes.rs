//! 设置页应用信息入口拥有受控更新日志模态与焦点恢复。

use super::super::Shell;
use gpui_kit::{
    AnyElement, Context, FocusHandle, FontWeight, IntoElement, Window,
    base::{Dialog, DialogBackdrop, DialogPopup, TestSupportExt as _},
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    div,
    prelude::*,
    px,
};

/// 模态仅拥有原生焦点，不镜像构建嵌入的发布数据。
pub(in crate::app) struct ReleaseNotesModal {
    focus: FocusHandle,
    previous: Option<FocusHandle>,
}

impl Shell {
    /// 打开时记录实际焦点；同一窗口只能存在一个更新日志模态。
    pub(in crate::app) fn open_release_notes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.release_notes_modal.is_some() {
            return;
        }
        let previous = window.focused(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.release_notes_modal = Some(ReleaseNotesModal { focus, previous });
        cx.notify();
    }

    /// 所有关闭路径恢复打开前焦点，Escape与点击关闭产生相同结果。
    fn close_release_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = self.release_notes_modal.take() {
            if let Some(previous) = modal.previous {
                window.focus(&previous, cx);
            }
            cx.notify();
        }
    }

    /// 根层模态约束Tab、遮罩及Escape；Enter不会误关闭只读日志。
    pub(in crate::app) fn release_notes_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.release_notes_modal.as_ref()?;
        let close = cx.entity().downgrade();
        let content_height = (window.viewport_size().height - px(160.)).max(px(120.));
        let surface = div()
            .id("release-notes-surface")
            .test_support()
            .w_full()
            .max_h(window.viewport_size().height - px(64.))
            .p_4()
            .rounded(px(12.))
            .border_1()
            .border_color(if cx.theme().mode.is_dark() {
                gpui_kit::rgb(0x424242)
            } else {
                gpui_kit::rgb(0xdee2e6)
            })
            .bg(if cx.theme().mode.is_dark() {
                gpui_kit::rgb(0x242424)
            } else {
                gpui_kit::rgb(0xffffff)
            })
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .id("release-notes-header")
                    .test_support()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(gpui_kit::relative(1.))
                            .font_weight(FontWeight::NORMAL)
                            .child(self.t("about.release_notes")),
                    )
                    .child(
                        Button::new("release-notes-close")
                            .ghost()
                            .w(px(28.))
                            .h(px(28.))
                            .child(
                                gpui_kit::component::Icon::default()
                                    .data(include_bytes!("../../../assets/icons/close.svg"))
                                    .size(px(19.6)),
                            )
                            .accessibility_label(self.t("about.close"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_release_notes(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("release-notes-scroll")
                    .test_support()
                    .max_h(content_height)
                    .overflow_y_scroll()
                    .child(self.release_notes_panel(cx)),
            );
        let popup = DialogPopup::new().w(px(620.)).max_w_full().child(surface);
        Some(
            Dialog::new(cx)
                .focus_handle(modal.focus.clone())
                .on_ok(|_, _, cx| {
                    // 只读区域不确认；让聚焦关闭按钮收到完整按键事件并正常点击。
                    cx.propagate();
                    false
                })
                .backdrop(
                    DialogBackdrop::new()
                        .absolute()
                        .inset_0()
                        .bg(gpui_kit::black().opacity(0.6)),
                )
                .popup(popup)
                .on_open_change(move |open, _, window, cx| {
                    if !open {
                        let _ = close.update(cx, |this, cx| this.close_release_notes(window, cx));
                    }
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        AppContext, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke, TestAppContext, test::TestWindowExt,
    };

    /// 测试宿主的simulate_keystrokes仅发送按下，补齐松开以覆盖原生按钮点击。
    fn press_and_release(
        handle: gpui_kit::AnyWindowHandle,
        key: &str,
        cx: &mut TestAppContext,
    ) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_event(
                KeyDownEvent {
                    keystroke: keystroke.clone(),
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
            window.render_frame(cx);
        })
        .unwrap();
    }

    /// 弹窗实际绘制表面在宽窄窗口保持620px居中，关闭控件位于表面右上角。
    #[gpui_kit::test]
    fn release_notes_surface_keeps_reference_width_and_center(cx: &mut TestAppContext) {
        for width in [960., 1440.] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 900.);
            cx.update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| {
                    this.navigate(crate::app::Page::Settings, cx);
                    this.open_release_notes(window, cx);
                });
                window.render_frame(cx);
                let surface = window.find("release-notes-surface").bounds();
                assert_eq!(surface.size.width, px(620.), "{surface:?}");
                assert_eq!(surface.origin.x, px((width - 620.) / 2.));
                let close = window.find("release-notes-close").bounds();
                assert!(close.right() <= surface.right());
                assert!(close.origin.x >= surface.right() - px(48.));
            })
            .unwrap();
        }
    }

    /// 设置按钮打开真正模态；内部点击和Enter保留，关闭按钮恢复原焦点。
    #[gpui_kit::test]
    fn release_notes_modal_close_restores_focus_and_body_does_not_dismiss(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        let previous = cx
            .update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| this.navigate(crate::app::Page::Settings, cx));
                window.render_frame(cx);
                let previous = cx.focus_handle();
                window.focus(&previous, cx);
                window.click("settings-release-notes", cx);
                window.render_frame(cx);
                assert!(shell.read(cx).release_notes_modal.is_some());
                assert!(window.try_find("release-notes-scroll").is_some());
                window.click("release-notes-empty", cx);
                assert!(shell.read(cx).release_notes_modal.is_some());
                previous
            })
            .unwrap();
        press_and_release(handle, "enter", cx);
        assert!(shell.read_with(cx, |this, _| this.release_notes_modal.is_some()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("release-notes-close", cx);
            assert!(shell.read(cx).release_notes_modal.is_none());
            assert_eq!(window.focused(cx), Some(previous));
        })
        .unwrap();
    }

    /// Tab后Enter和Space都可点击关闭按钮，并恢复打开前的实际焦点。
    #[gpui_kit::test]
    fn release_notes_keyboard_close_restores_focus(cx: &mut TestAppContext) {
        for key in ["enter", "space"] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
            let previous = cx
                .update_window(handle, |_, window, cx| {
                    shell.update(cx, |this, cx| this.navigate(crate::app::Page::Settings, cx));
                    window.render_frame(cx);
                    let previous = cx.focus_handle();
                    window.focus(&previous, cx);
                    shell.update(cx, |this, cx| this.open_release_notes(window, cx));
                    window.render_frame(cx);
                    previous
                })
                .unwrap();
            cx.simulate_keystrokes(handle, "tab");
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            press_and_release(handle, key, cx);
            cx.update_window(handle, |_, window, cx| {
                assert!(shell.read(cx).release_notes_modal.is_none(), "{key}");
                assert_eq!(window.focused(cx), Some(previous));
            })
            .unwrap();
        }
    }

    /// Escape和遮罩关闭同一状态；离页也卸载模态，不把日志插入页面流。
    #[gpui_kit::test]
    fn release_notes_modal_escape_backdrop_and_navigation_are_consistent(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 960., 640.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.navigate(crate::app::Page::Settings, cx);
                this.open_release_notes(window, cx);
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.simulate_keystrokes(handle, "escape");
        assert!(shell.read_with(cx, |this, _| this.release_notes_modal.is_none()));
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| this.open_release_notes(window, cx));
            window.render_frame(cx);
            let bounds = window.find("dialog-0").bounds();
            assert_eq!(bounds.size, window.viewport_size());
            window.click_at("dialog-0", gpui_kit::point(px(5.), px(5.)), cx);
            assert!(shell.read(cx).release_notes_modal.is_none());
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.open_release_notes(window, cx);
                this.navigate(crate::app::Page::Dashboard, cx);
                assert!(this.release_notes_modal.is_none());
            })
        })
        .unwrap();
    }
}
