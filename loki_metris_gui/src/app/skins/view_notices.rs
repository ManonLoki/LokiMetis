//! 皮肤页顶部提示区：导入准备进度、查询错误、操作反馈与批量删除入口。

use super::*;

/// 按出现顺序收集提示元素；用 `child` 追加，写法与页面容器一致，便于原样迁移。
#[derive(Default)]
struct Notices(Vec<AnyElement>);

impl Notices {
    /// 追加一个提示元素。
    fn child(mut self, element: impl IntoElement) -> Self {
        self.0.push(element.into_any_element());
        self
    }
}

impl Shell {
    /// 依次产出：导入准备进度、查询错误（带重试）、操作反馈（可关闭）、批量删除按钮。
    /// 初次读取失败不展示伪造空库，刷新失败保留已确认卡片并提供重试。
    pub(super) fn skins_notices(&self, writable: bool, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut page = Notices::default();
        if let Some(draft) = &self.skins.import
            && draft.batch.is_none() {
                let (processed, total) = draft.lease.session.progress();
                page = page.child(
                    div().id("skin-import-progress").role(Role::Status).child(
                        self.t("skins.progress.preparing")
                            .replace("%{processed}", &processed.to_string())
                            .replace("%{total}", &total.to_string()),
                    ),
                );
            }
        if let Some(error) = self
            .skins
            .catalog_error
            .as_ref()
            .or(self.skins.host_error.as_ref())
        {
            page = page.child(
                div()
                    .id("skins-query-error")
                    .role(Role::Alert)
                    .p_4()
                    .rounded(px(12.))
                    .bg(cx.theme().danger.opacity(0.1))
                    .text_color(cx.theme().danger)
                    .child(self.t("skins.error.title"))
                    .child(div().mt_1().child(self.skin_error(error)))
                    .child(
                        Button::new("skins-query-retry")
                            .outline()
                            .mt_2()
                            .label(self.t("skins.action.refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_skins(true, cx))),
                    ),
            );
        }
        if let Some(feedback) = &self.skins.feedback {
            let (message, color) = match feedback {
                Ok(success) => (
                    match success {
                        SkinSuccess::Created { name } => self.t("skins.notice.created").replace("%{name}", name),
                        SkinSuccess::Converted { name } => self.t("skins.notice.converted").replace("%{name}", name),
                        SkinSuccess::Deleted { count } => self.t("skins.notice.deleted").replace("%{count}", &count.to_string()),
                        SkinSuccess::Exported => self.t("skins.notice.exported"),
                        SkinSuccess::Stopped => self.t("skins.notice.stopped"),
                        SkinSuccess::Applied { name } => self.t("skins.notice.applied").replace("%{name}", name),
                        SkinSuccess::Imported { count } => self.t("skins.notice.imported").replace("%{count}", &count.to_string()),
                    },
                    rgb(0x12b886),
                ),
                Err(error) => (self.skin_error(error), rgb(0xfa5252)),
            };
            page = page.child(
                div()
                    .id("skins-feedback")
                    .test_support()
                    .role(Role::Alert)
                    .aria_label(message.clone())
                    .p_4()
                    .rounded(px(16.))
                    .border_1()
                    .border_color(color.opacity(0.3))
                    .bg(color.opacity(0.09))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(message)
                    .child(
                        Button::new("skins-dismiss-feedback")
                            .ghost()
                            .label("×")
                            .accessibility_label(self.t("skins.dismiss"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.skins.feedback = None;
                                cx.notify();
                            })),
                    ),
            );
        }
        if !self.skins.selected.is_empty() {
            page = page.child(
                Button::new("skins-delete-selected")
                    .outline()
                    .text_color(cx.theme().danger)
                    .label(
                        self.t("skins.action.delete_selected")
                            .replace("%{count}", &self.skins.selected.len().to_string()),
                    )
                    .disabled(!writable)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_skin_dialog(
                            ModalKind::Delete(this.skins.selected.clone()),
                            window,
                            cx,
                        )
                    })),
            );
        }
        page.0
    }
}
