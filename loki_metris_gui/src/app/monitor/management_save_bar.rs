// 本文件是 management.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：保存栏是管理内容后的最后正常流兄弟项，由局部 sticky 负责 bottom12 与页尾回流。；稳定行为键与 core 四行为一致。

impl Shell {
    /// 保存栏是管理内容后的最后正常流兄弟项，由局部 sticky 负责 bottom12 与页尾回流。
    pub(super) fn monitor_profile_save_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.page != crate::app::Page::Monitor
            || self.monitor.route != super::state::Route::Management
        {
            return None;
        }
        let state = &self.monitor.catalog;
        if !state.profiles_loaded
            || self.agent_settings.loading
            || state.profiles_error.is_some()
            || state.images_error.is_some()
            || self.agent_settings.error.is_some()
        {
            return None;
        }
        let tool = self.monitor_tool()?;
        let editor = state
            .editors
            .iter()
            .find(|editor| editor.draft.tool == tool)?;
        let draft = &editor.draft;
        let configured = draft
            .hooks
            .iter()
            .filter(|hook| {
                state
                    .images
                    .iter()
                    .any(|image| image.id == hook.image && image.source.is_ok())
            })
            .count();
        Some(
            div()
                .id("monitor-profile-save-bar")
                .text_color(rgb(if cx.theme().mode.is_dark() {
                    0xc9c9c9
                } else {
                    0x000000
                }))
                .test_support()
                .block_mouse_except_scroll()
                .w_full()
                .flex_shrink_0()
                .p_3()
                .rounded(px(16.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(self.t("monitor.management.displayConfig")),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(if configured == draft.hooks.len() {
                                    self.t("monitor.management.ready")
                                } else {
                                    self.t("monitor.management.remaining").replace(
                                        "%{count}",
                                        &(draft.hooks.len() - configured).to_string(),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .when(state.saved_tool == Some(tool), |row| {
                            row.child(
                                div()
                                    .id("monitor-profile-saved")
                                    .role(Role::Status)
                                    .aria_label(self.t("monitor.management.saved"))
                                    .text_color(rgb(0x20c997))
                                    .child(self.t("monitor.management.saved")),
                            )
                        })
                        .child(
                            SurfaceButton::new(
                                "monitor-profile-save",
                                SurfaceButtonVariant::FilledBrand,
                                cx.theme().mode.is_dark(),
                            )
                            .historical_size(HistoricalButtonSize::Small)
                            .left_icon(
                                gpui_kit::component::Icon::default().data(include_bytes!(
                                    "../../../assets/icons/phosphor-check.svg"
                                )),
                            )
                            .label(self.t("monitor.management.save"))
                            .loading(state.saving)
                            .disabled(!state.images_loaded)
                            .on_click(cx.listener(|this, _, _, cx| this.save_monitor_profile(cx))),
                        ),
                )
                .into_any_element(),
        )
    }
}


/// 稳定行为键与 core 四行为一致。
pub(super) fn behavior_key(behavior: HookBehavior) -> &'static str {
    match behavior {
        HookBehavior::Idle => "idle",
        HookBehavior::Running => "running",
        HookBehavior::Asking => "asking",
        _ => "error",
    }
}
