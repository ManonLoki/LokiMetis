//! 本地发布说明仅消费构建嵌入的真实双语条目。

use super::super::Shell;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, base::TestSupportExt as _,
    component::ActiveTheme, div, prelude::*, px,
};

impl Shell {
    /// 候选构建嵌入已经验证的双语条目，普通构建保持真实空状态且不读取磁盘或网络。
    pub(in crate::app) fn release_notes_panel(&self, cx: &Context<Self>) -> AnyElement {
        let notes = crate::release_notes::RELEASE_NOTES;
        div()
            .id("release-notes")
            .test_support()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_5()
            .when(notes.is_empty(), |panel| {
                panel.child(
                    div()
                        .id("release-notes-empty")
                        .test_support()
                        .whitespace_normal()
                        .text_sm()
                        .child(self.t("about.no_releases")),
                )
            })
            .children(notes.iter().take(5).enumerate().map(
                |(index, (date, version, features, fixes))| {
                    div()
                        .id(("release-note", index))
                        .test_support()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded(px(12.))
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(gpui_kit::relative(1.35))
                                .font_weight(FontWeight(650.))
                                .child(format!(
                                    "{date} · v{}",
                                    version.trim_start_matches(['v', 'V'])
                                )),
                        )
                        .child(self.release_note_group("about.release_features", features, cx))
                        .child(self.release_note_group("about.release_fixes", fixes, cx))
                },
            ))
            .into_any_element()
    }

    /// 同一条目按当前界面语言选译文；分组为空时明示无变更，禁止捏造历史。
    fn release_note_group(
        &self,
        heading: &str,
        entries: &[(&str, &str)],
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t(heading)),
            )
            .when(entries.is_empty(), |group| {
                group.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.t("about.no_changes")),
                )
            })
            .children(entries.iter().take(10).map(|(zh, en)| {
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child("•")
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(16.))
                            .child(if self.locale == "zh-CN" {
                                (*zh).to_owned()
                            } else {
                                (*en).to_owned()
                            }),
                    )
            }))
            .into_any_element()
    }
}
