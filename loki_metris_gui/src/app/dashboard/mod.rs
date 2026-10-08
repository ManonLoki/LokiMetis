//! 用量看板的原生视图；会话筛选由根实体保留，数字全部来自领域查询。

mod format;
mod workbuddy;
mod statistics;
mod charts;
mod plot;
mod workbuddy_charts;
mod state;
mod calls;
mod sources;
mod usage_settings;
use statistics::DetailState;
pub(super) use statistics::Route;

use super::Shell;
use crate::usage_service::{UsageData, UsageService, UsageQuery, UsageResponse};
use gpui_kit::{AnyElement, Context, Div, FontWeight, IntoElement, Task,
    assets::IconName, base::TestSupportExt as _, component::{ActiveTheme, button::{Button, ButtonVariants}},
    div, prelude::*, px, rgb};
use loki_metris_core::{LocalIndexState, SourceClientKind, Freshness, Completeness, Confidence};
use std::sync::Arc;

/// 看板进程内会话保持客户端和自然日窗口，不把筛选写入业务库。
pub(super) struct Dashboard {
    pub client: usize,
    pub ranges: [usize; 5],
    pub progress: Option<crate::backend::local_index::ScanProgress>,
    pub route: Route,
    pub detail: DetailState,
    pub chart: charts::ChartState,
    pub calls: calls::CallsState,
    pub sources: sources::SourcesState,
    pub usage_settings:usage_settings::UsageSettingsState,
    pub scanning: bool,
    pub scanning_client:Option<SourceClientKind>,
    pub scan_error: Option<&'static str>,
    pub watch_task: Option<Task<()>>,
    pub data: Option<UsageData>,
    pub error: Option<&'static str>,
    pub loading: bool,
    pub revision: u64,
    pub last_read: std::time::Instant,
    pub service: Option<Arc<UsageService>>,
    pub task: Option<Task<()>>,
    pub copied: bool,
    pub copied_metric: Option<&'static str>,
    pub manual_pending:bool,
    pub manual_task:Option<Task<()>>,
    pub copy_task: Option<Task<()>>,
}

impl Default for Dashboard {
    /// 首次与参考 App 一样选中 Codex、今日和概览；业务数据尚未查询。
    fn default() -> Self {
        Self { client: 1, ranges: [0; 5], progress: None, route: Route::Overview, detail: DetailState::default(), chart: charts::ChartState::default(), calls: calls::CallsState::default(), sources: sources::SourcesState::default(), usage_settings:Default::default(), scanning: false, scan_error: None, watch_task: None, data: None, error: None,
            loading: false, revision: 0, last_read: std::time::Instant::now(), service: None, task: None, copied: false, copied_metric: None, copy_task: None,manual_pending:false,manual_task:None,scanning_client:None }
    }
}

impl Shell {
    /// 启动时只接入独立服务，测试根视图不隐式触碰真实用户目录。
    pub fn attach_usage(&mut self, service: Arc<UsageService>, cx: &mut Context<Self>) {
        self.dashboard.service = Some(service.clone());
        self.load_agent_settings(cx);
        if let Err(error)=service.start_usage_lifecycle() {self.dashboard.scan_error=Some(error);}
        self.dashboard.watch_task = Some(cx.spawn(async move |entity, cx| {
            let mut revision = service.scan_status().revision;
            loop {
                let status = service.scan_status();
                let discovery=service.discovery_view();
                if entity.update(cx, |this, cx| {
                    let changed=this.dashboard.scanning!=status.running || this.dashboard.scanning_client!=status.client || this.dashboard.progress!=status.progress || this.dashboard.sources.discovery!=discovery;
                    this.dashboard.sources.discovery=discovery;
                    this.dashboard.scanning = status.running;
                    this.dashboard.scanning_client=status.client;
                    this.dashboard.progress = status.progress;
                    if revision != status.revision {
                        revision = status.revision;
                        this.dashboard.scan_error = status.error;
                        if this.dashboard.route!=Route::Settings {this.reload_usage(false, cx);}
                    } else if this.window_active && this.window_visible && this.page == super::Page::Dashboard && matches!(this.dashboard.route,Route::Statistics|Route::Charts)
                        && !this.dashboard.loading && this.dashboard.last_read.elapsed() >= std::time::Duration::from_secs(10) {
                        this.reload_usage(false,cx);
                    }
                    if this.page==super::Page::Dashboard&&this.dashboard.route==Route::Settings&&!this.dashboard.usage_settings.loading&&!this.dashboard.usage_settings.pending.contains(&true)&&this.dashboard.usage_settings.service_revision!=service.usage_settings_revision() {this.load_usage_settings(cx);}
                    if !this.agent_settings.loading&&!this.agent_settings.pending&&this.agent_settings.service_revision!=service.agent_settings_revision() {this.load_agent_settings(cx);}
                    if this.page==super::Page::Monitor && this.window_visible {this.refresh_monitor(false,cx);}
                    if changed {cx.notify();}
                }).is_err() { return }
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
            }
        }));
        self.reload_usage(false, cx);
    }

    /// 异步完成后用请求代数丢弃旧响应；切 Agent 不会被旧页面查询覆盖。
    pub(in crate::app) fn reload_usage(&mut self, scan: bool, cx: &mut Context<Self>) {
        if self.agent_settings.value.is_none()||self.agent_settings.dashboard_indices().is_empty() {
            self.dashboard.revision=self.dashboard.revision.wrapping_add(1);self.dashboard.task.take();
            self.dashboard.data=None;self.dashboard.loading=false;self.dashboard.error=None;cx.notify();return;
        }
        if self.dashboard.route==Route::Calls {self.load_calls(false,cx);return}
        if self.dashboard.route==Route::Sources {self.load_sources(cx);return}
        if self.dashboard.route==Route::Settings {self.dashboard.revision+=1;self.dashboard.task.take();self.dashboard.error=None;self.dashboard.loading=false;self.load_usage_settings(cx);return}
        self.dashboard.revision += 1;
        self.dashboard.task.take();
        self.dashboard.loading = false;
        self.dashboard.error = None;
        self.dashboard.last_read = std::time::Instant::now();
        self.dashboard.progress = None;
        let Some(service) = &self.dashboard.service else { cx.notify(); return };
        let client = match self.dashboard.client {
            1 => Some(SourceClientKind::Codex),
            2 => Some(SourceClientKind::ClaudeCode),
            3 => Some(SourceClientKind::GrokBuildCli),
            4 => Some(SourceClientKind::WorkBuddy),
            _ => None,
        };
        if scan {
            if let Err(error) = service.request_scan(client, loki_metris_core::ScanStartOrigin::ExplicitUser) {
                self.dashboard.scan_error = Some(error);
            }
        }
        let query = match self.dashboard.route {
            Route::Statistics => {
                let dimension = self.statistics_dimension();
                UsageQuery::Statistics(client.unwrap_or(SourceClientKind::Codex),
                    loki_metris_core::LocalUsageWindow::OVERVIEW_WINDOWS[self.dashboard.detail.ranges[self.dashboard.client]], dimension)
            }
            Route::Charts => UsageQuery::Chart(client,loki_metris_core::LocalUsageWindow::OVERVIEW_WINDOWS[self.dashboard.chart.ranges[self.dashboard.client]],self.chart_dimension()),
            _ => UsageQuery::Overview(client),
        };
        let mut work = match service.query(query) {
            Ok(work) => work,
            Err(error) => { self.dashboard.error = Some(error); cx.notify(); return }
        };
        let revision = self.dashboard.revision;
        self.dashboard.loading = true;
        self.dashboard.error = None;
        self.dashboard.task = Some(cx.spawn(async move |entity, cx| {
            let result = loop {
                match work.try_recv() {
                    Ok(result) => break result,
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => break Err("usage_scan_cancelled"),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {},
                }
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            };
            let _ = entity.update(cx, |this, cx| {
                if this.dashboard.revision != revision { return }
                this.dashboard.loading = false;
                this.dashboard.last_read = std::time::Instant::now();
                match result {
                    Ok(UsageResponse::Overview(data)) => { this.dashboard.data = Some(data); this.dashboard.error = None; }
                    Ok(UsageResponse::Statistics(data)) => { this.dashboard.detail.statistics = Some(data); this.dashboard.detail.models = None; }
                    Ok(UsageResponse::WorkbuddyDetails(data, models)) => { this.dashboard.detail.statistics = Some(data); this.dashboard.detail.models = Some(models); }
                    Ok(UsageResponse::Chart(data)) => {this.dashboard.chart.data=Some(data);this.dashboard.chart.workbuddy=None;}
                    Ok(UsageResponse::Workbuddy(data)) => {this.dashboard.chart.workbuddy=Some(data);this.dashboard.chart.data=None;}
                    Ok(_) => this.dashboard.error = Some("usage_query_failed"),
                    Err(error) => this.dashboard.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// All 没有物理存储；所有页面共用同一客户端身份映射。
    pub(super) fn usage_client(&self)->Option<SourceClientKind> {
        match self.dashboard.client {1=>Some(SourceClientKind::Codex),2=>Some(SourceClientKind::ClaudeCode),
            3=>Some(SourceClientKind::GrokBuildCli),4=>Some(SourceClientKind::WorkBuddy),_=>None}
    }

    /// 客户端变更立即清除旧客户端快照，保留每客户端自己的窗口选择。
    pub(in crate::app) fn select_usage_client(&mut self, index: usize, cx: &mut Context<Self>) {
        if index!=0&&!self.agent_settings.dashboard_indices().contains(&index) {return}
        if self.dashboard.client == index { return }
        if (1..=3).contains(&index)&&self.dashboard.usage_settings.physical_client!=index {self.dashboard.usage_settings.reset();self.dashboard.usage_settings.physical_client=index;}
        self.dashboard.client = index;
        self.dashboard.copied = false;
        self.dashboard.copied_metric = None;
        self.dashboard.copy_task.take();
        self.dashboard.data = None;
        self.dashboard.detail.statistics = None;
        self.dashboard.detail.models = None;
        self.dashboard.chart.data=None;self.dashboard.chart.workbuddy=None;
        self.dashboard.calls.page=None;self.dashboard.calls.loading_next=false;self.dashboard.calls.next_failed=false;self.dashboard.calls.replacement_failed=false;
        self.dashboard.sources.reset();
        for hover in &self.dashboard.chart.hover {hover.set(None);}
        if index==0 {self.dashboard.usage_settings.reset();}
        if (index == 0 && matches!(self.dashboard.route,Route::Statistics|Route::Charts|Route::Sources|Route::Settings)) || (index==4 && self.dashboard.route==Route::Calls) { self.dashboard.route = Route::Overview; }
        self.reload_usage(false, cx);
        cx.notify();
    }

    /// 业务页顶栏沿用参考产品的横向导航与右侧客户端分段。
    pub(super) fn dashboard_page(&self, window:&gpui_kit::Window, cx: &mut Context<Self>) -> AnyElement {
        let mut items = vec![(Route::Overview, "overview")];
        if self.dashboard.client==0 {items.push((Route::Calls,"calls"));}
        else {items.extend([(Route::Statistics,"usage"),(Route::Charts,"charts"),(Route::Sources,"sources"),(Route::Settings,"settings")]);}
        let tabs = div().flex().items_center().gap_1().children(items.into_iter().map(|(route, key)| {
            let selected = self.dashboard.route == route;
            Button::new(gpui_kit::SharedString::from(format!("dashboard-{key}"))).ghost().h(px(40.))
                .accessibility_label(self.t(&format!("dashboard.{key}"))).px_3()
                .child(div().text_size(px(14.08)).font_weight(FontWeight::BOLD)
                    .text_color(if selected { cx.theme().accent_foreground } else { cx.theme().muted_foreground }).child(self.t(&format!("dashboard.{key}"))))
                .when(selected, |button| button.bg(cx.theme().accent).text_color(cx.theme().accent_foreground))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.dashboard.route != route { this.dashboard.sources.reset();this.dashboard.usage_settings.reset();this.dashboard.route = route; this.reload_usage(false, cx); }
                }))
        }));
        let agents = div().flex().items_center().rounded(px(16.)).bg(cx.theme().secondary).p_1()
            .children([self.t("dashboard.all"), "Codex".into(), "Claude Code".into(), "Grok".into(), "WorkBuddy".into()]
                .into_iter().enumerate().filter(|(index,_)|*index==0||self.agent_settings.dashboard_indices().contains(index)).map(|(index, label)| {
                    Button::new(("usage-client", index)).ghost().accessibility_label(label.clone()).h(px(23.)).px(px(6.))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD)
                            .text_color(if self.dashboard.client == index { cx.theme().foreground } else { cx.theme().muted_foreground }).child(label))
                        .when(self.dashboard.client == index, |button| button.font_weight(FontWeight::BOLD).text_color(cx.theme().foreground))
                        .on_click(cx.listener(move |this, _, _, cx| this.select_usage_client(index, cx)))
                }));
        if self.agent_settings.value.is_none() {
            return div().w_full().flex().flex_col().gap_4()
                .child(self.t(if self.agent_settings.loading {"dashboard.loading"}else{"error.monitor.settingsReadFailed"}))
                .when(!self.agent_settings.loading,|page|page.child(Button::new("agent-settings-retry").outline().label(self.t("dashboard.retry"))
                    .on_click(cx.listener(|this,_,_,cx|this.load_agent_settings(cx)))))
                .into_any_element();
        }
        let empty=self.agent_settings.dashboard_indices().is_empty();
        div().id("dashboard").test_support().w_full().flex().flex_col().gap_5()
            .child(div().w_full().flex().flex_wrap().items_center().justify_between().gap_3().px_5().pt(px(10.4)).pb(px(8.8))
                .border_b_1().border_color(cx.theme().border).bg(cx.theme().tab_bar).child(tabs).when(!empty,|bar|bar.child(agents)))
            .when(empty,|column|column.child(self.panel(cx)
                .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(self.t("shell.noEnabledAgents.title")))
                .child(self.t("shell.noEnabledAgents.body"))))
            .when(self.dashboard.route == Route::Overview && self.dashboard.loading && self.dashboard.data.is_none(), |column| column.child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("dashboard.loading"))
                .when_some(self.dashboard.progress.as_ref(), |line, progress| line.child(format!(" {} / {} · {} {} · {} {}",
                    progress.roots_completed, progress.roots_total, progress.files_scanned, self.t("dashboard.files"), progress.calls_added, self.t("dashboard.calls"))))))
            .when(!empty&&self.dashboard.route!=Route::Settings && (self.dashboard.error.is_some() || self.dashboard.scan_error.is_some()), |column| column.child(self.usage_error(cx)))
            .when(!empty&&self.dashboard.error.is_none(), |column| column.child(match self.dashboard.route {
                Route::Calls => self.usage_calls(window,cx),
                Route::Sources => self.usage_sources(cx),
                Route::Settings=>self.usage_settings_page(cx),
            Route::Statistics => self.usage_statistics(cx),
            Route::Charts => self.usage_charts(cx),
                Route::Overview => self.usage_overview(cx),
            }))
            .into_any_element()
    }

    /// 失败保留明确重试动作，错误分类不暴露用户路径或会话内容。
    fn usage_error(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = match self.dashboard.error.or(self.dashboard.scan_error) {
            Some("usage_sources_not_configured") => "dashboard.not_scanned",
            Some("usage_workbuddy_absent") => "dashboard.workbuddy_absent",
            Some("usage_scan_cancelled") => "dashboard.cancelled",
            _ => "dashboard.error",
        };
        div().w_full().p_4().rounded_lg().border_1().border_color(cx.theme().danger)
            .flex().items_center().justify_between().gap_3().child(self.t(key))
            .child(Button::new("usage-retry").outline().label(self.t("dashboard.retry"))
                .on_click(cx.listener(|this, _, _, cx| this.reload_usage(false, cx))))
            .into_any_element()
    }

    /// 六个自然日窗口仅选择已有 core 事实；没有可信索引时不伪造零指标。
    fn usage_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.dashboard.ranges[self.dashboard.client];
        let ranges = div().w_full().flex().rounded(px(12.)).bg(cx.theme().secondary).p_1()
            .children(["today", "yesterday", "this_week", "last_week", "this_month", "last_month"]
                .into_iter().enumerate().map(|(index, key)| {
                    Button::new(("usage-window", index)).ghost().flex_1().h(px(28.))
                        .accessibility_label(self.t(&format!("dashboard.{key}")))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD)
                            .text_color(if selected == index { cx.theme().foreground } else { cx.theme().muted_foreground }).child(self.t(&format!("dashboard.{key}"))))
                        .when(selected == index, |button| button.font_weight(FontWeight::BOLD).text_color(cx.theme().foreground))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.dashboard.ranges[this.dashboard.client] = index; cx.notify();
                        }))
                }));
        let mut page = div().w_full().px_2().flex().flex_col().gap_4().child(ranges);
        let Some(data) = &self.dashboard.data else { return page.into_any_element() };
        if self.dashboard.client == 4 { return page.child(self.workbuddy_overview(data, selected, cx)).into_any_element(); }
        if matches!(data.records.index_state, LocalIndexState::NotScanned | LocalIndexState::NeedsRescan) {
            return page.child(self.panel(cx).child(self.t("dashboard.not_scanned"))
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
        page = page.child(self.panel(cx)
            .child(div().flex().items_center().justify_between()
                .child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(format!("{} {}", self.t(&format!("dashboard.{}", ["today", "yesterday", "this_week", "last_week", "this_month", "last_month"][selected])), self.t("dashboard.token_composition"))))
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
        page.child(self.panel(cx)
            .child(div().flex().items_center().justify_between().child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight::SEMIBOLD).child(self.t("dashboard.coverage")))
                .child(self.quality_badge(self.t("dashboard.enabled_summary"), cx.theme().warning)))
            .child(div().w_full().grid().grid_cols(columns).gap_4().children(coverage.into_iter().map(|(key, value)| {
                div().flex_1().min_w_0().px_4().py(px(14.4)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(cx.theme().muted)
                    .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(self.t(&format!("dashboard.{key}"))))
                    .child(div().id(gpui_kit::SharedString::from(format!("coverage-{key}"))).role(gpui_kit::Role::Label)
                        .aria_label(self.t(&format!("dashboard.{key}"))).aria_value(value.clone()).text_size(px(16.)).font_weight(FontWeight::EXTRA_BOLD).child(value))
            }))))
            .into_any_element()
    }

    /// 大指标保持调用/Token 等宽和真实精确值复制，父卡片没有复制事件。
    fn hero_metric(&self, label: &str, value: String, precise: Option<u64>, cx: &mut Context<Self>) -> AnyElement {
        self.panel(cx).relative().overflow_hidden().flex_1().min_w_0().h(px(176.)).gap_0()
            .bg(gpui_kit::linear_gradient(180., gpui_kit::linear_color_stop(rgb(0xd7952c), 3. / 176.), gpui_kit::linear_color_stop(cx.theme().secondary, 3. / 176.)))
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
                            cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                            let _ = entity.update(cx, |this, cx| { this.dashboard.copied = false; cx.notify(); });
                        }));
                        cx.notify();
                    })))))
            .into_any_element()
    }

    /// 缺失指标保留“未提供”；精确值和紧凑值来自同一个整数。
    fn token_cell(&self, key: &'static str, value: Option<u64>, cx: &Context<Self>) -> AnyElement {
        let missing = self.t("dashboard.not_provided");
        div().flex_1().min_w_0().px_4().py(px(14.4)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(cx.theme().muted)
            .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(self.t(&format!("dashboard.{key}"))))
            .child(div().id(gpui_kit::SharedString::from(format!("token-{key}"))).role(gpui_kit::Role::Label)
                .aria_label(self.t(&format!("dashboard.{key}"))).aria_value(value.map(format::exact).unwrap_or_else(|| missing.clone()))
                .font_family("Geist Mono").text_right().text_size(px(16.)).line_height(gpui_kit::relative(1.55)).font_weight(FontWeight::BOLD).child(value.map(format::compact).unwrap_or_else(|| missing.clone())))
            .child(div().font_family("Geist Mono").text_right().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(value.map(format::exact).unwrap_or(missing)))
            .into_any_element()
    }

    /// 质量标记只投影 core 的事实元数据，不推断缺失数据为完整覆盖。
    pub(in crate::app) fn quality_badge(&self, label: String, color: gpui_kit::Hsla) -> AnyElement {
        div().px(px(10.)).h(px(20.)).flex().items_center().rounded_full().border_1().border_color(color.opacity(0.4))
            .bg(color.opacity(0.10)).text_color(color).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD)
            .child(label).into_any_element()
    }

    /// 参考页面的卡片尺寸与留白归属展示层，所有颜色沿用完整语义主题。
    fn panel(&self, cx: &Context<Self>) -> Div {
        div().w_full().flex().flex_col().gap_4().p_5().rounded(px(20.))
            .border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
    }
}
