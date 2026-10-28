//! 用量看板的原生视图；会话筛选由根实体保留，数字全部来自领域查询。

mod format;
mod workbuddy;
mod statistics;
mod charts;
mod plot;
mod workbuddy_charts;
mod state;
mod cursor_account;
mod calls;
mod overview;
mod metric_display;
mod sources;
mod usage_settings;
#[cfg(test)]
mod material_tests;
use statistics::DetailState;
pub(super) use statistics::Route;

use super::Shell;
use loki_metris_service::usage_service::{UsageData, UsageService, UsageQuery, UsageResponse};
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Task,
    base::TestSupportExt as _, component::{ActiveTheme, button::{Button, ButtonVariants}},
    div, prelude::*, px};
use loki_metris_core::SourceClientKind;
use std::sync::Arc;

/// 复制精确值后对勾图标的保留时长，到期恢复为复制图标。
const COPY_FEEDBACK: std::time::Duration = std::time::Duration::from_millis(1500);

/// 六个自然日窗口的稳定顺序与翻译键后缀；下标即 core 窗口事实的索引。
const NATURAL_WINDOW_KEYS: [&str; 6] = ["today", "yesterday", "this_week", "last_week", "this_month", "last_month"];

/// Cursor 账户在看板客户端下标中的固定槽位，按单窗口查询且不参与本机子页。
pub(in crate::app) const CURSOR_CLIENT: usize = 5;

/// 语义色胶囊的唯一外观：二十像素高、浅色底与淡描边，标识、辅助语义和交互由调用方附加。
pub(in crate::app) fn tone_badge<E: Styled>(element: E, color: gpui_kit::Hsla) -> E {
    element.px(px(10.)).h(px(20.)).flex().items_center().rounded_full().border_1().border_color(color.opacity(0.4))
        .bg(color.opacity(0.10)).text_color(color).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD)
}

/// 看板进程内会话保持客户端和自然日窗口，不把筛选写入业务库。
pub(super) struct Dashboard {
    /// 看板客户端下标：0 全部、1 Codex、2 Claude Code、3 Grok、4 WorkBuddy、5 Cursor账户；本地子页仍只使用前五槽。
    pub client: usize,
    /// 各客户端概览页选中的自然日窗口下标（对应 `NATURAL_WINDOW_KEYS`）。
    pub ranges: [usize; 6],
    /// Cursor 账户查询快照，不参与本机总量。
    pub cursor_account: Option<Arc<loki_metris_core::cursor_usage::CursorUsageSummary>>,
    /// 在途扫描的进度。
    pub progress: Option<loki_metris_service::usage_service::ScanProgress>,
    /// 当前子页。
    pub route: Route,
    /// 统计页的筛选与结果。
    pub detail: DetailState,
    /// 图表页的筛选、结果与悬浮。
    pub chart: charts::ChartState,
    /// 调用页的筛选与分页结果。
    pub calls: calls::CallsState,
    /// 数据源页状态。
    pub sources: sources::SourcesState,
    /// 用量设置页状态。
    pub usage_settings:usage_settings::UsageSettingsState,
    /// 后台有扫描在途。
    pub scanning: bool,
    /// 在途扫描的 Agent；全部 Agent 扫描时为 None。
    pub scanning_client:Option<SourceClientKind>,
    /// 最近一轮扫描的失败类别。
    pub scan_error: Option<&'static str>,
    /// 每轮检查扫描状态、设置修订与 10 秒定时刷新的前台任务。
    pub watch_task: Option<Task<()>>,
    /// 当前客户端的概览数据；切换客户端时清空。
    pub data: Option<UsageData>,
    /// 最近一次查询的失败类别。
    pub error: Option<&'static str>,
    /// 查询在途。
    pub loading: bool,
    /// 查询代次；每次发起查询或切换子页递增，旧代次结果被丢弃。
    pub revision: u64,
    /// 最近一次发起或完成查询的时刻，用于 10 秒定时刷新。
    pub last_read: std::time::Instant,
    /// 用量服务；测试根视图不接入时为 None。
    pub service: Option<Arc<UsageService>>,
    /// 在途查询任务。
    pub task: Option<Task<()>>,
    /// 概览总 Token 精确值刚被复制，显示对勾。
    pub copied: bool,
    /// 统计摘要卡中刚被复制精确值的指标键。
    pub copied_metric: Option<&'static str>,
    /// 手动选择目录检查在途，期间不接受重复请求。
    pub manual_pending:bool,
    /// 手动目录检查任务。
    pub manual_task:Option<Task<()>>,
    /// 复制反馈到期后恢复图标的定时任务。
    pub copy_task: Option<Task<()>>,
}

impl Default for Dashboard {
    /// 首次与参考 App 一样选中 Codex、今日和概览；业务数据尚未查询。
    fn default() -> Self {
        Self { client: 1, ranges: [0; 6], cursor_account: None, progress: None, route: Route::Overview, detail: DetailState::default(), chart: charts::ChartState::default(), calls: calls::CallsState::default(), sources: sources::SourcesState::default(), usage_settings:Default::default(), scanning: false, scan_error: None, watch_task: None, data: None, error: None,
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
                let visible = match entity.update(cx, |this, cx| {
                    let changed=this.dashboard.scanning!=status.running || this.dashboard.scanning_client!=status.client || this.dashboard.progress!=status.progress || this.dashboard.sources.discovery!=discovery;
                    this.dashboard.sources.discovery=discovery;
                    this.dashboard.scanning = status.running;
                    this.dashboard.scanning_client=status.client;
                    this.dashboard.progress = status.progress;
                    if revision != status.revision {
                        revision = status.revision;
                        this.dashboard.scan_error = status.error;
                        if this.dashboard.route!=Route::Settings {this.reload_usage(false, cx);}
                    } else if this.window_active && this.window_visible && this.page == super::Page::Dashboard && matches!(this.dashboard.route,Route::Overview|Route::Statistics|Route::Charts)
                        && !this.dashboard.loading && this.dashboard.last_read.elapsed() >= std::time::Duration::from_secs(10) {
                        this.reload_usage(false,cx);
                    }
                    if this.page==super::Page::Dashboard&&this.dashboard.route==Route::Settings&&!this.dashboard.usage_settings.loading&&!this.dashboard.usage_settings.pending.contains(&true)&&this.dashboard.usage_settings.service_revision!=service.usage_settings_revision() {this.load_usage_settings(cx);}
                    if !this.agent_settings.loading&&!this.agent_settings.pending&&this.agent_settings.service_revision!=service.agent_settings_revision() {this.load_agent_settings(cx);}
                    if this.page==super::Page::Monitor && this.window_visible {this.refresh_monitor(false,cx);}
                    if this.page==super::Page::Skins && this.window_visible {this.refresh_skins(false,cx);}
                    this.refresh_devices(cx);
                    if changed {cx.notify();}
                    this.window_visible
                }) { Ok(visible) => visible, Err(_) => return };
                // 窗口不可见时没有界面等待这些状态，降低主线程唤醒频率。
                let period = if visible { 500 } else { 2_000 };
                cx.background_executor().timer(std::time::Duration::from_millis(period)).await;
            }
        }));
        self.reload_usage(false, cx);
    }

    /// 本机概览一次载入全部窗口，换窗口只需重绘；账户概览按窗口查询，丢弃旧窗口后重新读取。
    pub(super) fn select_overview_window(&mut self, index: usize, cx: &mut Context<Self>) {
        let client = self.dashboard.client;
        if self.dashboard.ranges[client] == index { return }
        self.dashboard.ranges[client] = index;
        if client == CURSOR_CLIENT {
            self.dashboard.cursor_account = None;
            self.reload_cursor_account(cx);
        } else { cx.notify(); }
    }

    /// 异步完成后用请求代数丢弃旧响应；切 Agent 不会被旧页面查询覆盖。
    pub(in crate::app) fn reload_usage(&mut self, scan: bool, cx: &mut Context<Self>) {
        if self.agent_settings.value.is_none()||self.agent_settings.dashboard_indices().is_empty() {
            self.dashboard.revision=self.dashboard.revision.wrapping_add(1);self.dashboard.task.take();
            self.dashboard.data=None;self.dashboard.loading=false;self.dashboard.error=None;cx.notify();return;
        }
        if self.dashboard.client == CURSOR_CLIENT { self.reload_cursor_account(cx); return; }
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
            3 => Some(SourceClientKind::Grok),
            4 => Some(SourceClientKind::WorkBuddy),
            _ => None,
        };
        if scan
            && let Err(error) = service.request_scan(client, loki_metris_core::ScanStartOrigin::ExplicitUser) {
                self.dashboard.scan_error = Some(error);
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
        let work = match service.query(query) {
            Ok(work) => work,
            Err(error) => { self.dashboard.error = Some(error); cx.notify(); return }
        };
        let revision = self.dashboard.revision;
        self.dashboard.loading = true;
        self.dashboard.error = None;
        self.dashboard.task = Some(cx.spawn(async move |entity, cx| {
            let result = crate::receipt::poll_receipt(work, cx.background_executor().clone(), std::time::Duration::from_millis(150))
                .await
                .unwrap_or(Err("usage_scan_cancelled"));
            let _ = entity.update(cx, |this, cx| {
                if this.dashboard.revision != revision { return }
                this.dashboard.loading = false;
                this.dashboard.last_read = std::time::Instant::now();
                match result {
                    Ok(UsageResponse::Overview(data)) => { this.dashboard.data = Some(data); this.dashboard.error = None; }
                    Ok(UsageResponse::Statistics(data)) => { this.dashboard.detail.statistics = Some(data); this.dashboard.detail.models = None; }
                    Ok(UsageResponse::WorkBuddyDetails(data, models)) => { this.dashboard.detail.statistics = Some(data); this.dashboard.detail.models = Some(models); }
                    Ok(UsageResponse::Chart(data)) => {this.dashboard.chart.data=Some(data);this.dashboard.chart.workbuddy=None;}
                    Ok(UsageResponse::WorkBuddy(data)) => {this.dashboard.chart.workbuddy=Some(data);this.dashboard.chart.data=None;}
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
            3=>Some(SourceClientKind::Grok),4=>Some(SourceClientKind::WorkBuddy),_=>None}
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
        self.dashboard.cursor_account = None;
        self.dashboard.detail.statistics = None;
        self.dashboard.detail.models = None;
        self.dashboard.chart.data=None;self.dashboard.chart.workbuddy=None;
        self.dashboard.calls.page=None;self.dashboard.calls.loading_next=false;self.dashboard.calls.next_failed=false;self.dashboard.calls.replacement_failed=false;
        self.dashboard.sources.reset();
        for hover in &self.dashboard.chart.hover {hover.set(None);}
        if index==0 {self.dashboard.usage_settings.reset();}
        if index == CURSOR_CLIENT || (index == 0 && matches!(self.dashboard.route,Route::Statistics|Route::Charts|Route::Sources|Route::Settings)) || (index==4 && self.dashboard.route==Route::Calls) { self.dashboard.route = Route::Overview; }
        self.reload_usage(false, cx);
        cx.notify();
    }

    /// 业务页顶栏沿用参考产品的横向导航与右侧客户端分段。
    pub(super) fn dashboard_page(&self, window:&gpui_kit::Window, cx: &mut Context<Self>) -> AnyElement {
        let mut items = vec![(Route::Overview, "overview")];
        if self.dashboard.client==0 {items.push((Route::Calls,"calls"));}
        else if self.dashboard.client != CURSOR_CLIENT {items.extend([(Route::Statistics,"usage"),(Route::Charts,"charts"),(Route::Sources,"sources"),(Route::Settings,"settings")]);}
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
            .children([self.t("dashboard.all"), SourceClientKind::Codex.display_name().into(), SourceClientKind::ClaudeCode.display_name().into(),
                SourceClientKind::Grok.display_name().into(), SourceClientKind::WorkBuddy.display_name().into(), self.t("cursor_account.title")]
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
            .when(empty,|column|column.child(self.panel("dashboard-no-agents", cx)
                .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(self.t("shell.noEnabledAgents.title")))
                .child(self.t("shell.noEnabledAgents.body"))))
            .when(self.dashboard.client != CURSOR_CLIENT && self.dashboard.route == Route::Overview && self.dashboard.loading && self.dashboard.data.is_none(), |column| column.child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("dashboard.loading"))
                .when_some(self.dashboard.progress.as_ref(), |line, progress| line.child(format!(" {} / {} · {} {} · {} {}",
                    progress.roots_completed, progress.roots_total, progress.files_scanned, self.t("dashboard.files"), progress.calls_added, self.t("dashboard.calls"))))))
            .when(!empty&&self.dashboard.client != CURSOR_CLIENT&&self.dashboard.route!=Route::Settings && (self.dashboard.error.is_some() || self.dashboard.scan_error.is_some()), |column| column.child(self.usage_error(cx)))
            .when(!empty && self.dashboard.client == CURSOR_CLIENT, |column| column.child(self.cursor_account_page(cx)))
            .when(!empty&&self.dashboard.client != CURSOR_CLIENT&&self.dashboard.error.is_none(), |column| column.child(match self.dashboard.route {
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

    /// 质量标记只投影 core 的事实元数据，不推断缺失数据为完整覆盖。
    pub(in crate::app) fn quality_badge(&self, label: String, color: gpui_kit::Hsla) -> AnyElement {
        tone_badge(div(), color).child(label).into_any_element()
    }

    /// 参考页面的卡片尺寸与留白归属展示层，所有颜色沿用完整语义主题。
    fn panel(&self, id: impl Into<gpui_kit::ElementId>, cx: &Context<Self>) -> super::surface::StaticCard {
        super::surface::StaticCard::new(id, cx.theme().mode.is_dark(), super::surface::StaticMaterial::Panel)
            .priority(super::surface::CardPriority::Quiet).gap_4().p_5()
    }

    /// 仅历史spot-panel卡片使用聚光材质；筛选、表格和静态内容保留普通panel。
    fn spot_panel(&self, id: impl Into<gpui_kit::ElementId>, cx: &Context<Self>) -> super::surface::SpotlightCard {
        super::surface::SpotlightCard::new(id, cx.theme().mode.is_dark()).priority(crate::app::surface::CardPriority::Secondary)
            .gap_4().p_5()
    }
}
