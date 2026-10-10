//! 通知设置仅投影应用 owner 的内存状态，权限查询和重试只来自明确动作。
use super::*;
use gpui_kit::component::{Icon, IconName};
use loki_metris_core::{
    Completeness, Confidence, TodayAvailability, usage_milestones::MilestoneIssue,
};

#[cfg(test)]
mod tests;

impl Shell {
    /// 本地偏好、系统权限和持久队列分别显示，等待回执时保留原勾选。
    pub(super) fn settings_notifications(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = crate::native::notification_state(cx);
        self.settings_notification_card(&state, cx)
    }

    /// 详情只拥有会话内展开状态；收起仍显示需要处理的故障及恢复入口。
    fn settings_notification_card(
        &self,
        state: &crate::native::notifications::State,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let progress = &state.progress;
        let expanded = self.notification_details_expanded;
        let focus_color = cx.theme().primary;
        let changed =
            cx.listener(|_, enabled: &bool, _, cx| crate::native::set_notification(*enabled, cx));
        let permission =
            crate::native::notifications::permission_label(self.locale, state.permission);
        let summary = rust_i18n::t!(
            "notification.summary",
            locale = self.locale,
            pending = progress.pending,
            submitted = progress.submitted,
            unknown = progress.unknown
        )
        .to_string();
        let history = rust_i18n::t!(
            "notification.history",
            locale = self.locale,
            baseline = progress.baseline,
            suppressed = progress.suppressed,
            expired = progress.expired
        )
        .to_string();
        let waiting = state.enabled
            && (progress.error.is_some()
                || progress.snapshot.as_ref().is_none_or(|s| {
                    s.availability != TodayAvailability::Available
                        || s.completeness != Completeness::Complete
                        || s.confidence != Confidence::Exact
                }));
        let feedback = if progress.unknown_id.is_some() {
            Some("notification.unknown")
        } else if progress.error == Some("notification_history_rebuilt") {
            Some("notification.rebuilt")
        } else if progress
            .error
            .is_some_and(|e| e != "notification_snapshot_unavailable")
        {
            Some("notification.storage")
        } else if matches!(
            progress.issue,
            Some(MilestoneIssue::Rejected | MilestoneIssue::RetryExhausted)
        ) {
            Some("notification.rejected")
        } else if waiting {
            Some("notification.waiting")
        } else if progress.issue == Some(MilestoneIssue::SourceChanged) {
            Some("notification.source_rebuilt")
        } else if progress.baseline > 0 || progress.issue == Some(MilestoneIssue::Baseline) {
            Some("notification.baseline")
        } else {
            None
        };
        let retry_key = if progress.unknown_id.is_some() {
            "notification.retry_unknown"
        } else {
            "notification.retry"
        };
        let attention = feedback.is_some_and(|key| {
            !matches!(key, "notification.baseline" | "notification.source_rebuilt")
        });
        let permission_paused = state.enabled
            && state.error.is_none()
            && state.permission
                != loki_metris_core::usage_milestones::NotificationPermission::Allowed;
        let recovery = attention
            || state.error.is_some()
            || permission_paused
            || (state.enabled && !state.host_available);
        self.settings_plain_surface(20., cx)
            .id("notification-card")
            .test_support()
            .gap_3()
            .child(
                div()
                    .id("notification-header")
                    .test_support()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        gpui_kit::base::Button::new("notification-details-toggle")
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .focus_visible(move |style| {
                                style.shadow(vec![gpui_kit::base::box_shadow(
                                    0.,
                                    0.,
                                    0.,
                                    2.,
                                    focus_color,
                                )])
                            })
                            .h(px(26.))
                            .px_0()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .accessibility_label(self.t(if expanded {
                                "notification.hide_details"
                            } else {
                                "notification.show_details"
                            }))
                            .aria_expanded(expanded)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(self.t("settings.system_notification"))
                                    .child(
                                        Icon::new(if expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(px(14.)),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notification_details_expanded =
                                    !this.notification_details_expanded;
                                cx.notify();
                            })),
                    )
                    .child(
                        switches::settings_switch(
                            "notification-switch",
                            self.t("settings.system_notification"),
                            state.enabled,
                            state.busy,
                            cx,
                        )
                        .on_change(move |enabled, _, window, cx| changed(&enabled, window, cx)),
                    ),
            )
            .when(expanded, |card| {
                card.child(
                    div()
                        .id("notification-details")
                        .test_support()
                        .role(Role::Region)
                        .aria_label(self.t("notification.details"))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    rust_i18n::t!(
                                        "notification.permission_label",
                                        locale = self.locale,
                                        status = permission
                                    )
                                    .to_string(),
                                ),
                        )
                        .child(div().text_sm().child(summary))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(history),
                        ),
                )
            })
            .when(state.busy, |card| card.child(self.t("notification.busy")))
            .when(state.error.is_some(), |card| {
                use crate::native::notifications::Error;
                let (title, body) = match state.error {
                    Some(Error::Save) => (
                        "settings.capability_error_title",
                        "settings.system_notification_error",
                    ),
                    Some(Error::Denied) => {
                        ("notification.paused", "notification.denied_intent_saved")
                    }
                    _ => (
                        "notification.paused",
                        "notification.permission_intent_saved",
                    ),
                };
                card.child(self.capability_feedback(title, body, None, cx))
            })
            .when(permission_paused, |card| {
                let body = if state.permission
                    == loki_metris_core::usage_milestones::NotificationPermission::Denied
                {
                    "notification.denied_intent_saved"
                } else {
                    "notification.permission_intent_saved"
                };
                card.child(self.capability_feedback("notification.paused", body, None, cx))
            })
            .when(state.enabled && !state.host_available, |card| {
                card.child(
                    div()
                        .id("notification-host-feedback")
                        .role(Role::Status)
                        .text_sm()
                        .child(self.t("notification.host_unavailable")),
                )
            })
            .when_some(feedback.filter(|_| expanded || attention), |card, key| {
                card.child(
                    div()
                        .id("notification-feedback")
                        .test_support()
                        .role(Role::Status)
                        .aria_label(self.t(key))
                        .text_sm()
                        .child(self.t(key)),
                )
            })
            .when(expanded || recovery, |card| {
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("notification-retry")
                                .outline()
                                .label(self.t(retry_key))
                                .accessibility_label(self.t(retry_key))
                                .disabled(state.busy || !state.enabled)
                                .on_click(cx.listener(|_, _, _, cx| {
                                    crate::native::retry_notifications(cx)
                                })),
                        )
                        .child(
                            Button::new("notification-settings")
                                .ghost()
                                .label(self.t("notification.open_settings"))
                                .on_click(|_, _, cx| {
                                    crate::native::notifications::open_settings(cx)
                                }),
                        ),
                )
            })
            .into_any_element()
    }
}
