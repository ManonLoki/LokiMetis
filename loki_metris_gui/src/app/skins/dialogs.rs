//! 原生皮肤模态保留输入法、恢复焦点，并将确认绑定到打开时的资源身份。

use super::state::Modal;
use super::{ModalKind, Shell};
use gpui_kit::{
    AnyElement, Context, Focusable, FontWeight, IntoElement, Role, Window,
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

impl Shell {
    /// 壳层只读取模态存在性以隔离背景辅助树，不暴露皮肤内部状态类型。
    pub(in crate::app) fn skin_modal_open(&self) -> bool {
        self.skins.modal.is_some()
    }

    /// 原生文件面板关闭后仅恢复仍有效的主窗，避免焦点落到桌宠浮窗。
    pub(super) fn restore_skin_picker_focus(&self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let handle = self.native_window;
        let epoch = self
            .skins
            .authority
            .load(std::sync::atomic::Ordering::Acquire);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                let current = entity.read(cx);
                if current.page == super::Page::Skins
                    && current
                        .skins
                        .authority
                        .load(std::sync::atomic::Ordering::Acquire)
                        == epoch
                {
                    cx.activate(true);
                    window.activate_window();
                }
            });
        });
    }

    /// 创建对话框每次重建两项输入；此前生成的提示词仍保留。
    pub(super) fn open_skin_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.skin_writable() {
            return;
        }
        let inputs = ["skins.create.name", "skins.create.author"].map(|key| {
            let placeholder = self.t(key);
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .validate(|value, _| value.encode_utf16().count() <= 80)
            })
        });
        self.skins.input_subscriptions = inputs
            .iter()
            .map(|input| {
                cx.subscribe(input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        let first = inputs[0].focus_handle(cx);
        self.open_skin_dialog(ModalKind::Create(inputs), window, cx);
        window.focus(&first, cx);
    }
    /// 同步打开携带当前窗口焦点；确认窗口不拥有领域权威状态。
    pub(super) fn open_skin_dialog(
        &mut self,
        kind: ModalKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.skin_writable() {
            return;
        }
        let focus = cx.focus_handle();
        let previous = window.focused(cx);
        window.focus(&focus, cx);
        self.skins.modal = Some(Modal {
            kind,
            focus,
            previous,
        });
        cx.notify();
    }
    /// 回执可能来自无当前Window作用域，用主窗口句柄延后创建模态。
    pub(super) fn defer_skin_dialog(&mut self, kind: ModalKind, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let handle = self.native_window;
        let epoch = self
            .skins
            .authority
            .load(std::sync::atomic::Ordering::Acquire);
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                entity.update(cx, |this, cx| {
                    if this
                        .skins
                        .authority
                        .load(std::sync::atomic::Ordering::Acquire)
                        != epoch
                    {
                        return;
                    }
                    let host = match &kind {
                        ModalKind::Appearance(_, intent) | ModalKind::Restart(intent, _) => {
                            Some(intent.host)
                        }
                        _ => None,
                    };
                    if this.page == super::Page::Skins
                        && host.is_none_or(|host| {
                            this.skins.host == Some(host) && this.skin_hosts().contains(&host)
                        })
                    {
                        this.open_skin_dialog(kind, window, cx);
                    }
                });
            });
        });
    }
    /// 成功回执在主窗恢复焦点；不在异步无Window作用域借用窗口。
    pub(super) fn defer_close_skin_dialog(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let handle = self.native_window;
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                entity.update(cx, |this, cx| this.close_skin_dialog_now(window, cx))
            });
        });
    }
    /// 导入关闭先等待后台取消，其余模态只释放草稿。
    pub(super) fn close_skin_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.skins.busy {
            return;
        }
        if matches!(
            self.skins.modal.as_ref().map(|modal| &modal.kind),
            Some(ModalKind::Import)
        ) {
            self.cancel_skin_import(window, cx);
        } else {
            self.close_skin_dialog_now(window, cx);
        }
    }
    /// 关闭只回收草稿与焦点，已提交文件事务由服务继续收尾。
    pub(super) fn close_skin_dialog_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(modal) = self.skins.modal.take() {
            if let Some(focus) = modal.previous {
                window.focus(&focus, cx);
            }
        }
        self.skins.input_subscriptions.clear();
        cx.notify();
    }
    /// 三种资源确认及宿主确认复用受控模态；内部点击由popup阻止穿透。
    pub(in crate::app) fn skin_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.skins.modal.as_ref()?;
        let (title, description, confirm, danger) = match &modal.kind {
            ModalKind::Create(_) => (
                self.t("skins.create.title"),
                self.t("skins.create.description"),
                self.t("skins.create.scaffold"),
                false,
            ),
            ModalKind::Delete(items) => (
                self.t("skins.delete.title"),
                self.t("skins.delete.description")
                    .replace("%{count}", &items.len().to_string()),
                self.t("skins.action.delete"),
                true,
            ),
            ModalKind::Convert(skin) => (
                self.t("skins.convert.title"),
                self.t("skins.convert.description")
                    .replace("%{name}", &skin.name),
                self.t("skins.dialog.confirm"),
                false,
            ),
            ModalKind::Appearance(check, _) => (
                self.t("skins.appearance.title"),
                self.t("skins.appearance.description").replace(
                    "%{mode}",
                    &self.t(
                        if check.effective_mode == loki_metris_core::ColorMode::Light {
                            "skins.mode.light"
                        } else {
                            "skins.mode.dark"
                        },
                    ),
                ),
                self.t("skins.appearance.continue"),
                false,
            ),
            ModalKind::Import => (
                self.t("skins.import.title"),
                String::new(),
                String::new(),
                false,
            ),
            ModalKind::Restart(intent, mode) => (
                self.t(
                    if *mode == super::state::RestartMode::RecoverWindowsWorkBuddy {
                        "skins.restart.workbuddy_title"
                    } else {
                        "skins.restart.title"
                    },
                ),
                self.t(
                    if *mode == super::state::RestartMode::RecoverWindowsWorkBuddy {
                        "skins.restart.workbuddy_description"
                    } else {
                        "skins.restart.description"
                    },
                )
                .replace("%{name}", &intent.instance_label),
                self.t("skins.dialog.confirm"),
                false,
            ),
        };
        let mut valid = self.skin_writable();
        // gpui-base 0.7.1的焦点包装器不转发辅助语义，实际Div独立承载模态名称。
        let mut popup = div()
            .id("skin-dialog-surface")
            .role(Role::Dialog)
            .aria_label(title.clone())
            .aria_description(description.clone())
            .test_support()
            .w(px(
                if matches!(modal.kind, ModalKind::Create(_) | ModalKind::Import) {
                    620.
                } else {
                    480.
                },
            ))
            .max_w_full()
            .max_h(px(760.))
            .p(px(16.))
            .rounded(px(12.))
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
                    .gap_3()
                    .child(
                        div()
                            .id("skin-dialog-title")
                            .role(Role::Heading)
                            .aria_level(1)
                            .aria_label(title.clone())
                            .test_support()
                            .font_weight(FontWeight::NORMAL)
                            .text_size(px(16.))
                            .child(title),
                    )
                    .child(
                        Button::new("skin-dialog-close")
                            .ghost()
                            .label("×")
                            .accessibility_label(self.t("about.close"))
                            .disabled(self.skins.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_skin_dialog(window, cx)
                            })),
                    ),
            )
            .when(!description.is_empty(), |popup| popup.child(
                div()
                    .id("skin-dialog-description")
                    .role(Role::Label)
                    .aria_label(description.clone())
                    .test_support()
                    .text_size(px(14.))
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            ));
        if let ModalKind::Create(inputs) = &modal.kind {
            for (index, input) in inputs.iter().enumerate() {
                valid &=
                    loki_metris_core::normalize_skin_creator_text(&input.read(cx).value()).is_ok();
                let label = self.t(if index == 0 {
                    "skins.create.name"
                } else {
                    "skins.create.author"
                });
                popup = popup.child(
                    div().flex().flex_col().gap_1().child(label.clone()).child(
                        Input::new(input)
                            .aria_label(label)
                            .disabled(self.skins.busy),
                    ),
                );
            }
            popup = popup.child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        Button::new("skin-dialog-confirm")
                            .primary()
                            .label(self.t("skins.create.scaffold"))
                            .disabled(!valid)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.create_skin(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("skin-prompt")
                            .outline()
                            .label(self.t("skins.create.codex_prompt"))
                            .disabled(!self.skin_writable())
                            .on_click(cx.listener(|this, _, _, cx| this.skin_prompt(cx))),
                    ),
            );
            if let Some(prompt) = &self.skins.prompt {
                popup = popup
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(self.t("skins.create.prompt_title"))
                            .child(
                                Button::new("skin-prompt-copy")
                                    .outline()
                                    .label(if self.skins.copied {
                                        self.t("skins.copied")
                                    } else {
                                        self.t("skins.create.copy")
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(prompt) = &this.skins.prompt {
                                            cx.write_to_clipboard(
                                                gpui_kit::ClipboardItem::new_string(prompt.clone()),
                                            );
                                            this.skins.copied = true;
                                            cx.notify();
                                        }
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("skin-prompt-text")
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .p_3()
                            .rounded(px(10.))
                            .bg(cx.theme().background)
                            .font_family("Geist Mono")
                            .text_size(px(12.))
                            .child(prompt.clone()),
                    );
            }
        }
        if let ModalKind::Appearance(check, _) = &modal.kind {
            popup = popup.child(
                div().flex().flex_col().gap_2().children(
                    check
                        .differences
                        .iter()
                        .chain(&check.unreadable)
                        .map(|difference| {
                            div().child(format!(
                                "{}: {} → {}",
                                difference.label,
                                difference
                                    .current_value
                                    .clone()
                                    .unwrap_or_else(|| self.t("skins.appearance.unreadable")),
                                difference.expected_value
                            ))
                        }),
                ),
            );
        }
        if matches!(modal.kind, ModalKind::Import) {
            popup = popup.child(self.skin_import_body(cx));
        } else if !matches!(modal.kind, ModalKind::Create(_)) {
            popup = popup.child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("skin-dialog-cancel")
                            .outline()
                            .label(self.t("skins.dialog.cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_skin_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("skin-dialog-confirm")
                            .primary()
                            .label(confirm)
                            .disabled(!valid)
                            .when(danger, |button| button.bg(cx.theme().danger))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if matches!(
                                    this.skins.modal.as_ref().map(|modal| &modal.kind),
                                    Some(ModalKind::Create(_))
                                ) {
                                    this.create_skin(window, cx)
                                } else {
                                    this.confirm_skin_resource(window, cx)
                                }
                            })),
                    ),
            );
        }
        let confirm = cx.entity().downgrade();
        let close = cx.entity().downgrade();
        let default_confirm_focus = modal.focus.clone();
        let input_confirm_focus: Vec<gpui_kit::FocusHandle> = match &modal.kind {
            ModalKind::Create(inputs) => inputs.iter().map(|input| input.focus_handle(cx)).collect(),
            _ => Vec::new(),
        };
        Some(
            Dialog::new(cx)
                .items_start()
                .pt(px(45.))
                .focus_handle(modal.focus.clone())
                .on_ok(move |_, window, cx| {
                    // 子按钮拥有Enter；输入框与容器仍保留原有默认确认，失效快照不阻止取消。
                    if !default_confirm_focus.is_focused(window)
                        && !input_confirm_focus.iter().any(|focus| focus.is_focused(window))
                    {
                        cx.propagate();
                        return false;
                    }
                    let _ = confirm.update(cx, |this, cx| {
                        if !this.skin_writable() {
                            return;
                        }
                        match this.skins.modal.as_ref().map(|modal| &modal.kind) {
                            Some(ModalKind::Create(_)) => this.create_skin(window, cx),
                            Some(ModalKind::Import) => this.commit_skin_import(cx),
                            Some(_) => this.confirm_skin_resource(window, cx),
                            None => {}
                        }
                    });
                    false
                })
                .close_on_escape(!self.skins.busy)
                .close_on_backdrop_press(!self.skins.busy)
                .backdrop(
                    DialogBackdrop::new()
                        .absolute()
                        .inset_0()
                        .bg(gpui_kit::black().opacity(0.55)),
                )
                .popup(DialogPopup::new().max_w_full().child(popup))
                .on_open_change(move |open, _, window, cx| {
                    if !open {
                        let _ = close.update(cx, |this, cx| this.close_skin_dialog(window, cx));
                    }
                })
                .into_any_element(),
        )
    }
}
