//! 导入确认展示后台真实逐项结果，信任勾选只绑定当前脚本集合。

use super::Shell;
use gpui_kit::{
    AnyElement, Context, IntoElement, Role,
    base::{Checkbox, CheckboxState},
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
    },
    div,
    prelude::*,
    px,
};

impl Shell {
    /// 独立正文供受控模态复用，失败项目与成功候选同屏保留。
    pub(super) fn skin_import_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut body = div().flex().flex_col().gap_4();
        let Some(draft) = &self.skins.import else {
            return body.into_any_element();
        };
        let Some(batch) = &draft.batch else {
            return body.into_any_element();
        };
        body = body.child(
            self.t("skins.import.summary")
                .replace("%{total}", &batch.total_files.to_string())
                .replace("%{ready}", &batch.items.len().to_string()),
        );
        let legacy_count = batch
            .items
            .iter()
            .filter(|item| {
                draft.selected.contains(&item.item_id)
                    && item.skin.package_type == loki_metris_core::SkinPackageType::LegacySkin
            })
            .count();
        if legacy_count > 0 {
            let weak = cx.entity().downgrade();
            body = body.child(
                div()
                    .p_4()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(cx.theme().danger.opacity(0.3))
                    .bg(cx.theme().danger.opacity(0.08))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_color(cx.theme().danger)
                            .child(self.t("skins.code_trust.import_title")),
                    )
                    .child(
                        self.t("skins.code_trust.import_description")
                            .replace("%{count}", &legacy_count.to_string()),
                    )
                    .child(self.t("skins.code_trust.not_safety_proof"))
                    .child(
                        Checkbox::new("skin-import-trust")
                            .checked(draft.trusted)
                            .disabled(self.skins.busy)
                            .accessibility_label(self.t("skins.code_trust.import_acknowledgement"))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(import_check(draft.trusted, cx))
                            .child(self.t("skins.code_trust.import_acknowledgement"))
                            .on_change(move |state, _, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    if !this.skins.busy {
                                        if let Some(draft) = &mut this.skins.import {
                                            draft.trusted = state == CheckboxState::Checked;
                                        }
                                        cx.notify();
                                    }
                                });
                            }),
                    ),
            );
        }
        let mut items = div()
            .id("skin-import-items")
            .max_h(px(320.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3();
        for item in &batch.items {
            let id = item.item_id.clone();
            let weak = cx.entity().downgrade();
            items = items.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Checkbox::new(format!("skin-import-{}", item.item_id))
                            .checked(draft.selected.contains(&item.item_id))
                            .disabled(self.skins.busy)
                            .accessibility_label(format!(
                                "{} · {}",
                                item.skin.name, item.skin.author
                            ))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(import_check(draft.selected.contains(&item.item_id), cx))
                            .child(format!("{} · {}", item.skin.name, item.skin.author))
                            .on_change(move |state, _, _, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.select_skin_import(
                                        &id,
                                        state == CheckboxState::Checked,
                                        cx,
                                    )
                                });
                            }),
                    )
                    .child(
                        div()
                            .pl_6()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(item.archive_name.clone()),
                    ),
            );
        }
        for item in &batch.skipped {
            items = items.child(
                div()
                    .p_3()
                    .rounded(px(10.))
                    .bg(gpui_kit::rgb(0xfab005).opacity(0.09))
                    .id(format!("skin-import-skipped-{}", item.archive_name))
                    .role(Role::Alert)
                    .child(div().child(item.archive_name.clone()))
                    .child(div().mt_1().text_size(px(13.)).child(item.message.clone())),
            );
        }
        body = body.child(items);
        let valid = self.skin_writable()
            && !draft.selected.is_empty()
            && (legacy_count == 0 || draft.trusted);
        body.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("skin-import-cancel")
                        .outline()
                        .label(self.t("skins.dialog.cancel"))
                        .disabled(self.skins.busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.cancel_skin_import(window, cx)),
                        ),
                )
                .child(
                    Button::new("skin-import-commit")
                        .primary()
                        .label(
                            self.t("skins.import.commit")
                                .replace("%{count}", &draft.selected.len().to_string()),
                        )
                        .disabled(!valid)
                        .on_click(cx.listener(|this, _, _, cx| this.commit_skin_import(cx))),
                ),
        )
        .into_any_element()
    }
}

/// 勾选图形保持独立背景，标签由同一可访问控件拥有。
fn import_check(checked: bool, cx: &Context<Shell>) -> AnyElement {
    div()
        .w(px(18.))
        .h(px(18.))
        .flex_shrink_0()
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(if checked {
            cx.theme().primary
        } else {
            cx.theme().background
        })
        .text_color(cx.theme().primary_foreground)
        .flex()
        .items_center()
        .justify_center()
        .child(if checked { "✓" } else { "" })
        .into_any_element()
}
