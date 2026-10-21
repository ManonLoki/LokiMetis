//! 工作台的提供商用量卡。只展示快照，读取发生在用量服务的后台任务里。

use super::Shell;
use crate::usage_service::provider_quota::{QuotaCard, QuotaCardPhase};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Role, base::TestSupportExt as _, component::{ActiveTheme, button::Button}, div, prelude::*, px, rgb};
use loki_metris_core::{ProviderQuota, QuotaProvider, QuotaWindow, QuotaWindowKind};

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

    /// 任一窗口有数据即可挂载卡片；全部不可用时隐藏面板。
    pub(super) fn monitor_quota_board(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let quotas: Vec<_> = self.monitor.quota.cards.iter().filter_map(visible_quota).collect();
        if quotas.is_empty() {
            return None;
        }
        let now = unix_now();
        let mut grid = div().w_full().grid().grid_cols(if self.viewport_width >= 768. { 2 } else { 1 }).gap_3();
        for quota in quotas {
            grid = grid.child(self.quota_provider_cell(quota, now, cx));
        }
        Some(super::super::surface::SpotlightCard::new("monitor-quota", cx.theme().mode.is_dark()).test_support().gap_4().p_4()
            .child(div().w_full().flex().items_center().justify_between().gap_3()
                .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(self.t("monitor.quota.title")))
                .child(Button::new("monitor-quota-retry").label(self.t("monitor.quota.retry")).on_click(cx.listener(|this, _, _, cx| this.retry_provider_quota(cx)))))
            .child(grid)
            .into_any_element())
    }

    /// 名称与可选套餐标签共用固定高标题行，避免套餐信息推低额度进度条。
    fn quota_provider_cell(&self, quota: &ProviderQuota, now_unix: i64, cx: &Context<Self>) -> AnyElement {
        let label = quota_card_label(quota, now_unix, |key| self.t(key));
        let id = quota_element_id(quota.provider);
        let name = super::monitor_tool_display_name(quota.provider.ai_tool());
        let mut heading = div().w_full().min_w_0().h(px(20.)).flex().items_center().gap(px(8.))
            .child(div().id(format!("{id}-name")).test_support().role(Role::Label).aria_label(name)
                .flex_shrink_0().text_size(px(14.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).child(name));
        if let Some(plan) = plan_line(&quota.plan, &|key| self.t(key)) {
            heading = heading.child(div().id(format!("{id}-plan")).test_support().role(Role::Label).aria_label(plan.clone())
                .min_w_0().max_w(px(160.)).h(px(20.)).px(px(8.)).flex().items_center().rounded(px(6.))
                .border_1().border_color(cx.theme().border).bg(cx.theme().muted)
                .text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(plan)));
        }
        let mut cell = div().id(id).test_support().role(Role::Label).aria_label(label).w_full().min_w_0().p(px(14.)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(rgb(if cx.theme().mode.is_dark() { 0x26231f } else { 0xf0ebe3 })).flex().flex_col().gap_3().child(heading);
        for window in quota.windows() {
            cell = cell.child(self.quota_window_bar(id, window, now_unix, cx));
        }
        cell.into_any_element()
    }

    /// 进度长度表示剩余比例；不超过20%为红色、不超过50%为黄色，其余为绿色。
    fn quota_window_bar(&self, provider_id: &'static str, window: QuotaWindow, now_unix: i64, cx: &Context<Self>) -> AnyElement {
        let key = window_key(window.kind);
        let kind = window_slug(window.kind);
        let label = window_line(&self.t(key), window, now_unix, &|key| self.t(key));
        let remaining = window.remaining_tenths();
        let color = if remaining <= 200 { cx.theme().danger } else if remaining <= 500 { cx.theme().warning } else { cx.theme().success };
        div().w_full().flex().flex_col().gap_1()
            .child(div().text_size(px(13.)).text_color(cx.theme().muted_foreground).child(label.clone()))
            .child(div().id(format!("{provider_id}-{kind}")).test_support().role(Role::ProgressIndicator).aria_label(label)
                .w_full().h(px(8.)).rounded_full().overflow_hidden().bg(cx.theme().border)
                .child(div().id(format!("{provider_id}-{kind}-fill")).test_support()
                    .w(gpui_kit::relative(f32::from(remaining) / 1000.)).h_full().rounded_full().bg(color)))
            .into_any_element()
    }
}

/// 将后台阶段映射为可展示额度，等待和失败隐藏，已有周或月额度独立展示。
fn visible_quota(card: &QuotaCard) -> Option<&ProviderQuota> {
    match &card.phase {
        QuotaCardPhase::Ready(quota) if quota.has_usage_window() => Some(quota),
        _ => None,
    }
}

/// 卡片的完整辅助标签，包含百分比和重置时间，颜色不是唯一信息来源。
pub(super) fn quota_card_label(quota: &ProviderQuota, now_unix: i64, t: impl Fn(&str) -> String) -> String {
    let mut lines = vec![super::monitor_tool_display_name(quota.provider.ai_tool()).to_owned()];
    if let Some(plan) = plan_line(&quota.plan, &t) {
        lines.push(plan);
    }
    for window in quota.windows() {
        lines.push(window_line(&t(window_key(window.kind)), window, now_unix, &t));
    }
    lines.join(" ")
}

/// 窗口类型对应的翻译键。
fn window_key(kind: QuotaWindowKind) -> &'static str {
    match kind {
        QuotaWindowKind::FiveHour => "monitor.quota.fiveHour",
        QuotaWindowKind::Weekly => "monitor.quota.weekly",
        QuotaWindowKind::Monthly => "monitor.quota.monthly",
    }
}

/// 窗口类型对应的稳定元素标识。
fn window_slug(kind: QuotaWindowKind) -> &'static str {
    match kind {
        QuotaWindowKind::FiveHour => "five-hour",
        QuotaWindowKind::Weekly => "weekly",
        QuotaWindowKind::Monthly => "monthly",
    }
}

/// 套餐名只在展示层规范Pro写法，其它名称保留来源拼写，不改变同步数据。
fn plan_line(plan: &Option<String>, t: &impl Fn(&str) -> String) -> Option<String> {
    plan.as_ref().map(|plan| {
        let label = if plan.eq_ignore_ascii_case("pro") { "Pro" } else { plan.as_str() };
        t("monitor.quota.plan").replace("%{plan}", label)
    })
}

/// 展示真实窗口的剩余比例和可选重置时间。
fn window_line(label: &str, window: QuotaWindow, now_unix: i64, t: &impl Fn(&str) -> String) -> String {
    let mut line = format!("{label} {}", percent_text(window.remaining_tenths()));
    if let Some(reset) = window.reset_at_unix.and_then(|reset| reset_text(reset, now_unix, t)) {
        line.push(' ');
        line.push_str(&reset);
    }
    line
}

/// 以注入时钟生成大致重置时间，已经到期时沿用即将重置提示。
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

/// 将千分比格式化为最多一位小数的百分比。
fn percent_text(tenths: u16) -> String {
    let whole = tenths / 10;
    let fraction = tenths % 10;
    if fraction == 0 { format!("{whole}%") } else { format!("{whole}.{fraction}%") }
}

/// 提供商对应的稳定卡片标识。
fn quota_element_id(provider: QuotaProvider) -> &'static str {
    match provider {
        QuotaProvider::Codex => "monitor-quota-codex",
        QuotaProvider::Claude => "monitor-quota-claude",
        QuotaProvider::Cursor => "monitor-quota-cursor",
        QuotaProvider::Grok => "monitor-quota-grok",
        QuotaProvider::WorkBuddy => "monitor-quota-workbuddy",
    }
}

/// 读取当前Unix秒，仅用于展示重置时间。
fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
