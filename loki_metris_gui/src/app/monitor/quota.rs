//! 工作台的提供商用量卡。只展示快照，读取发生在用量服务的后台任务里。

use super::Shell;
use crate::usage_service::provider_quota::{QuotaCard, QuotaCardPhase};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Role, base::TestSupportExt as _, component::{ActiveTheme, button::Button}, div, prelude::*, px, rgb};
use loki_metris_core::{QuotaFailure, QuotaProvider, QuotaWindow};

impl Shell {
    /// 复制用量快照并按五分钟节奏触发后台读取。测试构建不会访问本机登录态。
    pub(super) fn sync_provider_quota(&mut self, force: bool) -> bool {
        let Some(service) = &self.dashboard.service else { return false };
        service.kick_provider_quota(force);
        let next = service.provider_quota_snapshot();
        if next == self.monitor.quota {
            return false;
        }
        self.monitor.quota = next;
        true
    }

    /// 重新读取用量。按钮自己持有点击，卡片空白处没有动作。
    pub(super) fn retry_provider_quota(&mut self, cx: &mut Context<Self>) {
        if self.sync_provider_quota(true) {
            cx.notify();
        }
    }

    /// 四张提供商卡。中继状态缺失时仍然显示。
    pub(super) fn monitor_quota_board(&self, cx: &Context<Self>) -> AnyElement {
        let now = unix_now();
        let mut grid = div().w_full().grid().grid_cols(if self.viewport_width >= 768. { 2 } else { 1 }).gap_3();
        for card in &self.monitor.quota.cards {
            grid = grid.child(self.quota_provider_cell(card, now, cx));
        }
        super::super::surface::SpotlightCard::new("monitor-quota", cx.theme().mode.is_dark()).test_support().gap_4().p_4()
            .child(div().w_full().flex().items_center().justify_between().gap_3()
                .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(self.t("monitor.quota.title")))
                .child(Button::new("monitor-quota-retry").label(self.t("monitor.quota.retry")).on_click(cx.listener(|this, _, _, cx| this.retry_provider_quota(cx)))))
            .child(grid)
            .into_any_element()
    }

    fn quota_provider_cell(&self, card: &QuotaCard, now_unix: i64, cx: &Context<Self>) -> AnyElement {
        let label = quota_card_label(card, now_unix, |key| self.t(key));
        let mut cell = div().id(quota_element_id(card.provider)).test_support().role(Role::Label).aria_label(label.clone()).w_full().min_w_0().p(px(14.)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(rgb(if cx.theme().mode.is_dark() { 0x26231f } else { 0xf0ebe3 })).flex().flex_col().gap_1()
            .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child(super::monitor_tool_display_name(card.provider.ai_tool())));
        for line in quota_card_lines(card, now_unix, |key| self.t(key)) {
            cell = cell.child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child(line));
        }
        cell.into_any_element()
    }
}

/// 卡片的完整辅助标签，测试与界面共用同一句。
pub(super) fn quota_card_label(card: &QuotaCard, now_unix: i64, t: impl Fn(&str) -> String) -> String {
    let mut lines = vec![super::monitor_tool_display_name(card.provider.ai_tool()).to_owned()];
    lines.extend(quota_card_lines(card, now_unix, t));
    lines.join(" ")
}

fn quota_card_lines(card: &QuotaCard, now_unix: i64, t: impl Fn(&str) -> String) -> Vec<String> {
    let mut lines = Vec::new();
    match &card.phase {
        QuotaCardPhase::Pending => lines.push(t("monitor.quota.pending")),
        QuotaCardPhase::Failed(failure) => lines.push(t(failure_key(*failure))),
        QuotaCardPhase::NoWindow { plan } => {
            if let Some(plan) = plan_line(plan, &t) { lines.push(plan); }
            lines.push(t("monitor.quota.noWindow"));
        }
        QuotaCardPhase::Ready(quota) => {
            if let Some(plan) = plan_line(&quota.plan, &t) { lines.push(plan); }
            lines.push(window_line(&t("monitor.quota.fiveHour"), quota.five_hour, now_unix, &t));
            lines.push(window_line(&t("monitor.quota.weekly"), quota.weekly, now_unix, &t));
        }
    }
    lines
}

fn plan_line(plan: &Option<String>, t: &impl Fn(&str) -> String) -> Option<String> {
    plan.as_ref().map(|plan| t("monitor.quota.plan").replace("%{plan}", plan))
}

fn window_line(label: &str, window: Option<QuotaWindow>, now_unix: i64, t: &impl Fn(&str) -> String) -> String {
    let Some(window) = window else {
        return format!("{label} {}", t("monitor.quota.windowAbsent"));
    };
    let mut line = format!("{label} {}", percent_text(window.remaining_tenths()));
    if let Some(reset) = window.reset_at_unix.and_then(|reset| reset_text(reset, now_unix, t)) {
        line.push(' ');
        line.push_str(&reset);
    }
    line
}

fn reset_text(reset_at: i64, now_unix: i64, t: &impl Fn(&str) -> String) -> Option<String> {
    let delta = reset_at.saturating_sub(now_unix);
    if delta < 3_600 {
        return Some(t("monitor.quota.resetSoon"));
    }
    if delta >= 86_400 {
        return Some(t("monitor.quota.resetDays").replace("%{count}", &(delta / 86_400).to_string()));
    }
    Some(t("monitor.quota.resetHours").replace("%{count}", &(delta / 3_600).to_string()))
}

fn percent_text(tenths: u16) -> String {
    let whole = tenths / 10;
    let fraction = tenths % 10;
    if fraction == 0 { format!("{whole}%") } else { format!("{whole}.{fraction}%") }
}

fn failure_key(failure: QuotaFailure) -> &'static str {
    match failure {
        QuotaFailure::MissingCredentials => "monitor.quota.missingCredentials",
        QuotaFailure::CredentialLocked => "monitor.quota.credentialLocked",
        QuotaFailure::Unauthorized => "monitor.quota.unauthorized",
        QuotaFailure::Transport => "monitor.quota.transport",
        QuotaFailure::Malformed => "monitor.quota.malformed",
    }
}

fn quota_element_id(provider: QuotaProvider) -> &'static str {
    match provider {
        QuotaProvider::Codex => "monitor-quota-codex",
        QuotaProvider::Cursor => "monitor-quota-cursor",
        QuotaProvider::Grok => "monitor-quota-grok",
        QuotaProvider::WorkBuddy => "monitor-quota-workbuddy",
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::{QuotaWindowKind, ProviderQuota};

    #[test]
    fn reset_and_remaining_text_follow_the_injected_clock() {
        let t = |key: &str| match key {
            "monitor.quota.resetDays" => "%{count} 天后重置".to_owned(),
            "monitor.quota.resetHours" => "%{count} 小时后重置".to_owned(),
            "monitor.quota.resetSoon" => "即将重置".to_owned(),
            "monitor.quota.windowAbsent" => "暂无".to_owned(),
            other => other.to_owned(),
        };
        let window = QuotaWindow { kind: QuotaWindowKind::FiveHour, used_tenths: 275, reset_at_unix: Some(86_400 + 3_600) };
        assert_eq!(window_line("5 小时剩余", Some(window), 0, &t), "5 小时剩余 72.5% 1 天后重置");
        assert_eq!(reset_text(10, 0, &t).as_deref(), Some("即将重置"));
        assert_eq!(reset_text(7_200, 0, &t).as_deref(), Some("2 小时后重置"));
        assert_eq!(window_line("本周剩余", None, 0, &t), "本周剩余 暂无");
    }

    /// 用量卡在中继尚未成功时仍然可读；点击卡片或重试都不改掉已有数字。
    #[gpui_kit::test]
    fn quota_board_renders_windows_without_relay_status(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, _cx| {
                this.page = crate::app::Page::Monitor;
                this.monitor.status = None;
                this.monitor.quota.cards[0].phase = QuotaCardPhase::Ready(ProviderQuota {
                    provider: QuotaProvider::Codex,
                    plan: Some("plus".to_owned()),
                    five_hour: Some(QuotaWindow { kind: QuotaWindowKind::FiveHour, used_tenths: 270, reset_at_unix: None }),
                    weekly: Some(QuotaWindow { kind: QuotaWindowKind::Weekly, used_tenths: 40, reset_at_unix: None }),
                });
                this.monitor.quota.cards[1].phase = QuotaCardPhase::NoWindow { plan: None };
                this.monitor.quota.cards[3].phase = QuotaCardPhase::Failed(QuotaFailure::CredentialLocked);
            });
            window.render_frame(cx);
            assert!(window.try_find("monitor-received").is_none());
            let codex_element = window.find("monitor-quota-codex");
            let codex = codex_element.label().unwrap_or_default().to_owned();
            assert!(codex.contains("73%"), "{codex}");
            assert!(codex.contains("96%"), "{codex}");
            assert!(codex.contains("plus"), "{codex}");
            let cursor_element = window.find("monitor-quota-cursor");
            let cursor = cursor_element.label().unwrap_or_default().to_owned();
            assert!(cursor.contains("Connected"), "{cursor}");
            let workbuddy_element = window.find("monitor-quota-workbuddy");
            let workbuddy = workbuddy_element.label().unwrap_or_default().to_owned();
            assert!(workbuddy.contains("encrypted"), "{workbuddy}");
            let codex_bounds = window.find("monitor-quota-codex").bounds();
            let cursor_bounds = window.find("monitor-quota-cursor").bounds();
            assert_eq!(codex_bounds.origin.y, cursor_bounds.origin.y);
            assert!(cursor_bounds.origin.x > codex_bounds.right());
            let before_element = window.find("monitor-quota-codex");
            let before = before_element.label().map(str::to_owned);
            window.click("monitor-quota-codex", cx);
            window.click("monitor-quota-retry", cx);
            window.render_frame(cx);
            let after_element = window.find("monitor-quota-codex");
            assert_eq!(after_element.label().map(str::to_owned), before);
        }).unwrap();
    }
}
