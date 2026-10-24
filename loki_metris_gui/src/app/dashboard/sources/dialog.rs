//! 目录弹窗持有稳定草稿和焦点，关闭草稿不取消已经提交的后台写入。

use super::{Shell, SourceButton, SourceButtonVariant};
use loki_metris_service::usage_service::SourceOperation;
use gpui_kit::{
    AnyElement, Context, Entity, FocusHandle, Focusable, FontWeight, Role, Subscription, Window,
    base::{Dialog, DialogBackdrop, DialogPopup, TestSupportExt as _},
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::*,
    px,
};
use loki_metris_core::SourceRootSummary;

#[cfg(test)]
#[path = "dialog_a11y_tests.rs"]
mod a11y_tests;

/// 弹窗只持有别名草稿和目标身份，不把原始目录暴露给输入控件。
pub(in crate::app) struct SourceDialog {
    /// 目标数据根。
    root: SourceRootSummary,
    /// 是移除确认弹窗；为 false 时是改名弹窗。
    remove: bool,
    /// 别名输入框。
    input: Entity<InputState>,
    /// 弹窗焦点。
    focus: FocusHandle,
    /// 打开弹窗前的焦点，关闭时恢复。
    previous_focus: Option<FocusHandle>,
    /// 输入变化订阅。
    _subscription: Subscription,
}

impl Shell {
    /// 统一壳层使用同一个模态开关隐藏后台辅助树，来源页不创建另一套隔离容器。
    pub(in crate::app) fn source_modal_open(&self) -> bool {
        self.dashboard.sources.modal.is_some()
    }

    /// 输入实体只在打开时创建一次，保留中文输入法与焦点状态。
    pub(super) fn open_source_dialog(
        &mut self,
        root: SourceRootSummary,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous_focus = window.focused(cx);
        let focus = cx.focus_handle();
        let label = self.t("sources.dialog.renameLabel");
        let value = root.alias.clone();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder(label);
            input.set_value(value, window, cx);
            input
        });
        let subscription = cx.subscribe(&input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        if remove {
            window.focus(&focus, cx);
        } else {
            window.focus(&input.focus_handle(cx), cx);
        }
        self.dashboard.sources.modal = Some(SourceDialog {
            root,
            remove,
            input,
            focus,
            previous_focus,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// 取消、遮罩和 Escape 统一恢复最初触发控件焦点。
    fn close_source_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(dialog) = self.dashboard.sources.modal.take()
            && let Some(focus) = dialog.previous_focus {
                window.focus(&focus, cx);
            }
        cx.notify();
    }

    /// 原版确认立即关闭弹窗；写锁竞争失败由页面反馈承接。
    fn confirm_source_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &self.dashboard.sources.modal else {
            return;
        };
        if self.dashboard.sources.refresh_failed {
            return;
        }
        let alias = dialog.input.read(cx).value().trim().to_owned();
        if !dialog.remove && (alias.is_empty() || alias == dialog.root.alias) {
            return;
        }
        let operation = if dialog.remove {
            SourceOperation::Remove {
                root_id: dialog.root.id.clone(),
            }
        } else {
            SourceOperation::Rename {
                root_id: dialog.root.id.clone(),
                alias,
            }
        };
        self.close_source_dialog(window, cx);
        self.change_source(operation, cx);
    }

    /// 无动画的居中模态使用 occlude popup 防止内部点击落到背景关闭处理。
    pub(in crate::app) fn source_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.dashboard.sources.modal.as_ref()?;
        let title = self.t(if modal.remove {
            "sources.dialog.removeTitle"
        } else {
            "sources.dialog.renameTitle"
        });
        let label = self.t(if modal.remove {
            "sources.dialog.removeConfirm"
        } else {
            "sources.dialog.save"
        });
        let description = if modal.remove {
            self.t("sources.dialog.removeBody")
                .replace("{{alias}}", &modal.root.alias)
                .replace("{{client}}", self.sources_client_name())
        } else {
            self.t("sources.dialog.renameLabel")
        };
        let value = modal.input.read(cx).value();
        let disabled = self.dashboard.sources.refresh_failed
            || (!modal.remove && (value.trim().is_empty() || value.trim() == modal.root.alias));
        // DialogPopup焦点包装器不转发辅助语义，实际内容Div承载命名模态。
        let mut popup = div()
            .id("sources-dialog-surface")
            .role(Role::Dialog)
            .aria_label(title.clone())
            .aria_description(description.clone())
            .test_support()
            .w_full()
            .p_4()
            .rounded(px(16.))
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("sources-dialog-title")
                            .role(Role::Heading)
                            .aria_level(1)
                            .aria_label(title.clone())
                            .test_support()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(16.))
                            .child(title),
                    )
                    .child(
                        Button::new("sources-dialog-close")
                            .ghost()
                            .label("×")
                            .accessibility_label(self.t("about.close"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_source_dialog(window, cx)
                            })),
                    ),
            );
        let description = div()
            .id("sources-dialog-description")
            .role(Role::Label)
            .aria_label(description.clone())
            .test_support()
            .child(description);
        if modal.remove {
            popup = popup.child(description);
        } else {
            popup = popup.child(
                div().flex().flex_col().gap_1().child(description).child(
                    Input::new(&modal.input)
                        .id("sources-dialog-input")
                        .aria_label(self.t("sources.dialog.renameLabel"))
                        .disabled(self.dashboard.sources.refresh_failed),
                ),
            );
        }
        popup = popup.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    SourceButton::new(
                        "sources-dialog-cancel",
                        SourceButtonVariant::SubtleBrand,
                        cx.theme().mode.is_dark(),
                    )
                    .h(px(36.))
                    .px(px(18.))
                    .rounded(px(12.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .label(self.t("sources.dialog.cancel"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_source_dialog(window, cx)),
                    ),
                )
                .child(
                    SourceButton::new(
                        "sources-dialog-confirm",
                        if modal.remove {
                            SourceButtonVariant::FilledRed
                        } else {
                            SourceButtonVariant::FilledBrand
                        },
                        cx.theme().mode.is_dark(),
                    )
                    .h(px(36.))
                    .px(px(18.))
                    .rounded(px(12.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .label(label)
                    .disabled(disabled)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.confirm_source_dialog(window, cx)),
                    ),
                ),
        );
        let weak = cx.entity().downgrade();
        let confirm = weak.clone();
        let confirm_focus = modal.focus.clone();
        let input_focus = (!modal.remove).then(|| modal.input.focus_handle(cx));
        Some(
            Dialog::new(cx)
                .focus_handle(modal.focus.clone())
                .close_on_escape(true)
                .close_on_backdrop_press(true)
                .backdrop(
                    DialogBackdrop::new()
                        .absolute()
                        .inset_0()
                        .bg(gpui_kit::black().opacity(0.55)),
                )
                .popup(DialogPopup::new().w(px(440.)).max_w_full().child(popup))
                .on_open_change(move |open, _, window, cx| {
                    if !open {
                        let _ = weak.update(cx, |this, cx| this.close_source_dialog(window, cx));
                    }
                })
                .on_ok(move |_, window, cx| {
                    // 子按钮拥有Enter；容器与原生别名输入继续保留默认确认。
                    if !confirm_focus.is_focused(window)
                        && !input_focus
                            .as_ref()
                            .is_some_and(|focus| focus.is_focused(window))
                    {
                        cx.propagate();
                        return false;
                    }
                    let _ = confirm.update(cx, |this, cx| this.confirm_source_dialog(window, cx));
                    false
                })
                .into_any_element(),
        )
    }
}
