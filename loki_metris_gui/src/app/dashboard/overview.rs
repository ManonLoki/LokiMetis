//! 概览页：六个自然日窗口的调用、Token 构成与覆盖卡片，数值全部来自 core 窗口事实。

use super::format;
use crate::app::Shell;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement,
    assets::IconName, base::TestSupportExt as _, component::{ActiveTheme, button::{Button, ButtonVariants}},
    div, prelude::*, px, rgb};
use loki_metris_core::{LocalIndexState, Freshness, Completeness, Confidence};

impl Shell {
    /// “全部”概览里某个来源暂时读不到时的黄色提示：标题说明部分数据缺失，正文说明总量不含它并给出重试。
    fn partial_data_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        div().id("overview-partial-notice").test_support().role(gpui_kit::Role::Alert).w_full().p_4().rounded_lg()
            .border_1().border_color(cx.theme().warning.opacity(0.4)).bg(cx.theme().warning.opacity(0.1))
            .flex().items_center().justify_between().gap_3()
            .child(div().flex().flex_col().gap_1()
                .child(div().font_weight(FontWeight::BOLD).child(self.t("dashboard.partial_title")))
                .child(div().text_color(cx.theme().muted_foreground).child(self.t("dashboard.workbuddy_unavailable"))))
            .child(Button::new("overview-partial-retry").outline().label(self.t("dashboard.retry"))
                .on_click(cx.listener(|this, _, _, cx| this.reload_usage(false, cx))))
            .into_any_element()
    }

    /// 所有概览共用满宽等分的日期筛选；窗口切换后的加载方式由看板按客户端决定。
    pub(super) fn overview_window_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let client = self.dashboard.client;
        let selected = self.dashboard.ranges[client];
        div().id("usage-windows").test_support().w_full().flex().rounded(px(12.)).bg(cx.theme().secondary).p_1()
            .children(super::NATURAL_WINDOW_KEYS
                .into_iter().enumerate().map(|(index, key)| {
                    Button::new(("usage-window", index)).ghost().flex_1().h(px(28.))
                        .accessibility_label(self.t(&format!("dashboard.{key}")))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected == index { cx.theme().foreground } else { cx.theme().muted_foreground }).child(self.t(&format!("dashboard.{key}"))))
                        .when(selected == index, |button| button.font_weight(FontWeight::BOLD).text_color(cx.theme().foreground))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.dashboard.client == client { this.select_overview_window(index, cx); }
                        }))
                })).into_any_element()
    }

    /// 六个自然日窗口仅选择已有 core 事实；没有可信索引时不伪造零指标。
    pub(super) fn usage_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.dashboard.ranges[self.dashboard.client];
        let mut page = div().w_full().px_2().flex().flex_col().gap_4().child(self.overview_window_controls(cx));
        let Some(data) = &self.dashboard.data else { return page.into_any_element() };
        if self.dashboard.client == 0 && data.workbuddy_unavailable {
            // “全部”缺少 WorkBuddy 时必须明示，避免把少算的总量当成完整结果。
            page = page.child(self.partial_data_notice(cx));
        }
        if self.dashboard.client == 4 { return page.child(self.workbuddy_overview(data, selected, cx)).into_any_element(); }
        if matches!(data.records.index_state, LocalIndexState::NotScanned | LocalIndexState::NeedsRescan) {
            return page.child(self.panel("overview-not-scanned", cx).child(self.t("dashboard.not_scanned"))
                .child(Button::new("overview-scan").outline().label(self.t("dashboard.scan"))
                    .on_click(cx.listener(|this, _, _, cx| this.reload_usage(true, cx)))))
                .into_any_element();
        }
        let Some(window) = data.records.windows.get(selected) else { return page.into_any_element() };
        let columns = if self.viewport_width >= 1408. { 6 } else { 3 };
        let value = &window.fact.value;
        let tokens = &value.tokens;
        let total = tokens.total_tokens;
        page = page.child(div().w_full().flex().gap_4()
            .child(self.hero_metric("dashboard.calls", format::exact(value.call_count), None, cx))
            .child(self.hero_metric("dashboard.tokens", format::compact(total), Some(total), cx)));
        let fields = [
            ("input", Some(tokens.input_tokens)), ("cache_read", tokens.cached_input_tokens),
            ("cache_write", tokens.cache_write_input_tokens), ("uncached", tokens.uncached_input_tokens()),
            ("output", Some(tokens.output_tokens)), ("reasoning", tokens.reasoning_output_tokens),
        ];
        page = page.child(self.spot_panel("overview-token-composition", cx).test_support()
            .child(div().flex().items_center().justify_between()
                .child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(format!("{} {}", self.t(&format!("dashboard.{}", super::NATURAL_WINDOW_KEYS[selected])), self.t("dashboard.token_composition"))))
                .child(div().flex().items_center().gap_2()
                    .child(self.quality_badge(self.t(&format!("dashboard.provider_{}", self.dashboard.client)), cx.theme().warning))
                    .child(self.quality_badge(self.t(match window.fact.freshness { Freshness::Fresh => "dashboard.fresh", Freshness::Stale => "dashboard.stale", Freshness::Expired => "dashboard.expired", Freshness::Unknown => "dashboard.unknown" }), cx.theme().muted_foreground))
                    .child(self.quality_badge(self.t(match window.fact.completeness { Completeness::Complete => "dashboard.complete", Completeness::Partial => "dashboard.partial", Completeness::Unknown => "dashboard.unknown" }), if window.fact.completeness == Completeness::Complete { rgb(0x40c057).into() } else { rgb(0xfd7e14).into() }))
                    .child(self.quality_badge(self.t(match window.fact.confidence { Confidence::Exact => "dashboard.local_fact", Confidence::Derived => "dashboard.derived", Confidence::Suspected => "dashboard.suspected" }), rgb(0x339af0).into()))))
            .child(div().w_full().grid().grid_cols(columns).gap_4().children(fields.into_iter().map(|(key, value)| self.token_cell(key, value, cx)))));
        let coverage = [
            ("cache_ratio", value.cache_read_basis_points.map(|v| format!("{:.1}%", f64::from(v) / 100.)).unwrap_or_else(|| self.t("dashboard.not_provided"))),
            ("cache_calls", value.cached_read_call_count.map(format::exact).unwrap_or_else(|| self.t("dashboard.not_provided"))),
            ("threads", format::exact(value.thread_count)), ("roots", format::exact(value.root_count)),
            ("files", format::exact(value.source_count)), ("duplicates", format::exact(value.cross_root_duplicate_source_count)),
        ];
        page.child(self.spot_panel("overview-coverage", cx).priority(crate::app::surface::CardPriority::Quiet).test_support()
            .child(div().flex().items_center().justify_between().child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(self.t("dashboard.coverage")))
                .child(self.quality_badge(self.t("dashboard.enabled_summary"), cx.theme().warning)))
            .child(div().w_full().grid().grid_cols(columns).gap_4().children(coverage.into_iter().map(|(key, value)| {
                crate::app::surface::inset_card(format!("coverage-cell-{key}"), cx.theme().mode.is_dark()).flex_1().min_w_0().px_4().py(px(14.4))
                    .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(self.t(&format!("dashboard.{key}"))))
                    .child(div().id(gpui_kit::SharedString::from(format!("coverage-{key}"))).role(gpui_kit::Role::Label)
                        .aria_label(self.t(&format!("dashboard.{key}"))).aria_value(value.clone()).text_size(px(16.)).font_weight(FontWeight::EXTRA_BOLD).child(value))
            }))))
            .into_any_element()
    }

    /// 大指标保持调用/Token 等宽和真实精确值复制，父卡片没有复制事件。
    pub(super) fn hero_metric(&self, label: &str, value: String, precise: Option<u64>, cx: &mut Context<Self>) -> AnyElement {
        self.spot_panel(gpui_kit::SharedString::from(format!("hero-{label}")), cx)
            .priority(if matches!(label, "dashboard.calls" | "dashboard.tokens" | "workbuddy.requests") {
                crate::app::surface::CardPriority::Primary
            } else { crate::app::surface::CardPriority::Secondary })
            .overflow_hidden().flex_1().min_w_0().h(px(176.)).gap_0()
            .top_accent(px(3.), rgb(0xd7952c).into())
            .test_support()
            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child(self.t(label)))
            .child(div().id(gpui_kit::SharedString::from(label.to_owned())).role(gpui_kit::Role::Label)
                .aria_label(self.t(label)).aria_value(value.clone()).mt_1().text_size(px(44.8))
                .line_height(gpui_kit::relative(1.55)).font_weight(FontWeight::EXTRA_BOLD).child(value))
            .when_some(precise, |card, total| card.child(div().flex().items_center().gap_1()
                .child(div().id("precise-total").role(gpui_kit::Role::Label).aria_label(self.t("dashboard.exact"))
                    .aria_value(format::exact(total)).font_family("Geist Mono").text_xs().text_color(cx.theme().muted_foreground)
                    .child(format!("{}：{} Token", self.t("dashboard.exact"), format::exact(total))))
                .child(Button::new("copy-total").ghost().size(px(22.)).p_0()
                    .child(gpui_kit::component::Icon::new(if self.dashboard.copied { IconName::Check } else { IconName::Copy }).size(px(14.)))
                    .when(self.dashboard.copied, |button| button.text_color(cx.theme().success))
                    .tooltip(self.t(if self.dashboard.copied { "dashboard.copied" } else { "dashboard.copy" }))
                    .accessibility_label(self.t(if self.dashboard.copied { "dashboard.copied" } else { "dashboard.copy" }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(format::exact(total)));
                        this.dashboard.copied = true;
                        this.dashboard.copy_task = Some(cx.spawn(async |entity, cx| {
                            cx.background_executor().timer(super::COPY_FEEDBACK).await;
                            let _ = entity.update(cx, |this, cx| { this.dashboard.copied = false; cx.notify(); });
                        }));
                        cx.notify();
                    })))))
            .into_any_element()
    }

    /// 缺失指标保留“未提供”；精确值和紧凑值来自同一个整数。
    fn token_cell(&self, key: &'static str, value: Option<u64>, cx: &Context<Self>) -> AnyElement {
        let missing = self.t("dashboard.not_provided");
        crate::app::surface::inset_card(format!("token-cell-{key}"), cx.theme().mode.is_dark()).flex_1().min_w_0().px_4().py(px(14.4))
            .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(self.t(&format!("dashboard.{key}"))))
            .child(div().id(gpui_kit::SharedString::from(format!("token-{key}"))).role(gpui_kit::Role::Label)
                .aria_label(self.t(&format!("dashboard.{key}"))).aria_value(value.map(format::exact).unwrap_or_else(|| missing.clone()))
                .font_family("Geist Mono").text_left().text_size(px(16.)).line_height(gpui_kit::relative(1.55)).font_weight(FontWeight::BOLD).child(value.map(format::compact).unwrap_or_else(|| missing.clone())))
            .child(div().font_family("Geist Mono").text_left().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(value.map(format::exact).unwrap_or(missing)))
            .into_any_element()
    }
}
