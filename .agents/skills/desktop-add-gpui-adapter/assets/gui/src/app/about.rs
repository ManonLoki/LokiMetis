//! 关于页仅显示当前应用身份与真实本地状态；初始化没有远程更新或发布记录。

use super::Shell;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, base::TestSupportExt as _, component::{ActiveTheme, button::Button}, div, img, prelude::*, px};

impl Shell {
    /// 关于信息、检查状态与更新日志按钮都在实际页面内接线。
    pub(super) fn about_page(&self, cx: &mut Context<Self>) -> AnyElement {
        div().w_full().flex().flex_col().gap_6()
            .child(self.card("about.application", "about.description", cx)
                .child(div().flex().items_center().gap_5()
                    .child(img(crate::assets::LOGO).size(px(72.)))
                    .child(div().flex_1().min_w_0().flex().flex_col().gap_2()
                        .child(div().w_full().min_w_0().whitespace_normal().text_size(px(26.)).font_weight(FontWeight::BOLD).child(self.name()))
                        .child(self.version())))
                .child(div().w_full().min_w_0().whitespace_normal().text_sm().text_color(cx.theme().muted_foreground).child(format!("{}: @@OWNER@@", self.t("about.author"))))
                .child(div().flex().gap_3().flex_wrap()
                    .child(Button::new("about-update-check").outline().label(self.t("about.check_updates"))
                        .on_click(cx.listener(|this, _, _, cx| { this.show_update_status = true; cx.notify(); })))
                    .child(Button::new("about-release-notes").outline().label(self.t("about.release_notes"))
                        .on_click(cx.listener(|this, _, _, cx| { this.show_release_notes = !this.show_release_notes; cx.notify(); }))))
                .when(self.show_update_status, |card| card.child(div().w_full().min_w_0().whitespace_normal().text_sm().child(self.t("about.updater_unavailable")))))
            .when(self.show_release_notes, |page| page.child(self.release_notes_panel(cx)))
            .child(self.card("about.contact_author_title", "about.support_thanks", cx)
                .child("QQ @@BRAND_CONTACT@@"))
            .child(self.card("about.local_first", "about.local_first_detail", cx))
            .child(self.card("about.disclaimer_title", "about.disclaimer_1", cx)
                .child(div().w_full().min_w_0().whitespace_normal().text_sm().child(self.t("about.disclaimer_2")))
                .child(div().w_full().min_w_0().whitespace_normal().text_sm().child(self.t("about.disclaimer_3"))))
            .into_any_element()
    }

    /// 候选构建嵌入已经验证的双语条目，普通构建保持真实空状态且不读取磁盘或网络。
    fn release_notes_panel(&self, cx: &Context<Self>) -> AnyElement {
        let notes = crate::release_notes::RELEASE_NOTES;
        div().id("release-notes").test_support().w_full().min_w_0().flex_shrink_0().flex().flex_col().gap_4().p_6().rounded_lg()
            .border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(self.t("about.release_notes")))
            .when(notes.is_empty(), |panel| panel.child(div().id("release-notes-empty").test_support().whitespace_normal().text_sm().child(self.t("about.no_releases"))))
            .children(notes.iter().enumerate().map(|(index, (date, version, features, fixes))| {
                div().id(("release-note", index)).test_support().w_full().min_w_0().flex().flex_col().gap_2()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(format!("{date} v{}", version.trim_start_matches(['v', 'V']))))
                    .child(self.release_note_group("about.release_features", features, cx))
                    .child(self.release_note_group("about.release_fixes", fixes, cx))
            }))
            .into_any_element()
    }

    /// 同一条目按当前界面语言选译文；分组为空时明示无变更，禁止捏造历史。
    fn release_note_group(&self, heading: &str, entries: &[(&str, &str)], cx: &Context<Self>) -> AnyElement {
        div().w_full().min_w_0().flex().flex_col().gap_1()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(self.t(heading)))
            .when(entries.is_empty(), |group| group.child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("about.no_changes"))))
            .children(entries.iter().map(|(zh, en)| {
                div().w_full().min_w_0().whitespace_normal().text_sm().child(if self.locale == "zh-CN" { (*zh).to_owned() } else { (*en).to_owned() })
            }))
            .into_any_element()
    }
}
