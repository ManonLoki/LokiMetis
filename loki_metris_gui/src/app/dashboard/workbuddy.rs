//! WorkBuddy 的请求、会话、积分和 Trace 独立读模型，避免套用物理 Agent 的指标。

use super::{Shell, UsageData, format};
use gpui_kit::{AnyElement, Context, FontWeight, component::ActiveTheme, div, prelude::*, px};

impl Shell {
    /// 与参考产品一致投影所选六窗口；缺失来源由上层查询门禁处理。
    pub(super) fn workbuddy_overview(&self, data: &UsageData, selected: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(value) = data.workbuddy.as_ref().and_then(|snapshot| snapshot.windows.get(selected)) else {
            return div().into_any_element();
        };
        let columns = if self.viewport_width >= 1200. { 4 } else { 2 };
        let credits = value.credits.map(|credits| format!("{credits:.2}")).unwrap_or_else(|| self.t("dashboard.not_provided"));
        let cards = div().w_full().grid().grid_cols(columns).gap_4()
            .child(self.hero_metric("workbuddy.sessions", format::compact(value.session_count), None, cx))
            .child(self.hero_metric("workbuddy.requests", format::compact(value.request_count), None, cx))
            .child(self.hero_metric("dashboard.tokens", format::compact(value.tokens), Some(value.tokens), cx))
            .child(self.hero_metric("workbuddy.credits", credits, None, cx));
        let error_rate = if value.trace_total_count == 0 { self.t("workbuddy.not_applicable") }
            else { format!("{:.1}%", value.trace_error_count as f64 / value.trace_total_count as f64 * 100.) };
        let metrics = [
            ("dashboard.input", format::compact(value.input_tokens)),
            ("dashboard.cache_read", format::compact(value.cached_input_tokens)),
            ("dashboard.uncached", format::compact(value.uncached_input_tokens)),
            ("dashboard.output", format::compact(value.output_tokens)),
            ("workbuddy.top_level", format::compact(value.top_level_request_count)),
            ("workbuddy.subagent", format::compact(value.subagent_request_count)),
            ("workbuddy.average_session", format!("{:.1} {}", value.average_session_duration_seconds / 60., self.t("workbuddy.minutes"))),
            ("workbuddy.traces", format::compact(value.trace_total_count)),
            ("workbuddy.trace_error_rate", error_rate),
            ("workbuddy.trace_cancelled", format::compact(value.trace_cancelled_count)),
            ("workbuddy.trace_duration", format!("{:.0} {}", value.trace_average_duration_ms, self.t("workbuddy.milliseconds"))),
        ];
        let range = self.t(&format!("dashboard.{}", ["today", "yesterday", "this_week", "last_week", "this_month", "last_month"][selected]));
        div().w_full().flex().flex_col().gap_4().child(cards)
            .child(self.panel(cx)
                .child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{range} {}", self.t("workbuddy.breakdown"))))
                .child(div().w_full().grid().grid_cols(3).gap_4().children(metrics.into_iter().map(|(key, value)| {
                    div().px_4().py(px(14.4)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(cx.theme().muted)
                        .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(self.t(key)))
                        .child(div().id(gpui_kit::SharedString::from(key.to_owned())).role(gpui_kit::Role::Label)
                            .aria_label(self.t(key)).aria_value(value.clone()).text_size(px(16.)).font_weight(FontWeight::EXTRA_BOLD).child(value))
                }))))
            .into_any_element()
    }
}
