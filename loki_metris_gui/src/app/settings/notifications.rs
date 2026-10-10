//! 通知设置仅投影应用 owner 的内存状态，权限查询和重试只来自明确动作。
use super::*;
use loki_metris_core::{
    Completeness, Confidence, TodayAvailability, usage_milestones::MilestoneIssue,
};
impl Shell {
    /// 本地偏好、系统权限和持久队列分别显示，等待回执时保留原勾选。
    pub(super) fn settings_notifications(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = crate::native::notification_state(cx);
        let progress = &state.progress;
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
        self.settings_plain_surface(20., cx)
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("settings.system_notification")),
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
            )
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
            .when(state.enabled && !state.host_available, |card| {
                card.child(
                    div()
                        .id("notification-host-feedback")
                        .role(Role::Status)
                        .text_sm()
                        .child(self.t("notification.host_unavailable")),
                )
            })
            .when_some(feedback, |card, key| {
                card.child(
                    div()
                        .id("notification-feedback")
                        .role(Role::Status)
                        .text_sm()
                        .child(self.t(key)),
                )
            })
            .child(
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
                            .on_click(
                                cx.listener(|_, _, _, cx| crate::native::retry_notifications(cx)),
                            ),
                    )
                    .child(
                        Button::new("notification-settings")
                            .ghost()
                            .label(self.t("notification.open_settings"))
                            .on_click(|_, _, cx| crate::native::notifications::open_settings(cx)),
                    ),
            )
            .into_any_element()
    }
}
