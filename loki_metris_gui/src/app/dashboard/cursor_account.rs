//! 独立账户视图：原样展示 core 的分项和覆盖，日期筛选保持进程会话。

use super::{CURSOR_CLIENT, Shell, format, UsageQuery, UsageResponse};
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px,
    base::TestSupportExt as _, component::{ActiveTheme,
    table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow}}};
use loki_metris_core::cursor_usage::{CursorTokenCounts, CursorUsageGroup, ReportedTokens};

impl Shell {
    /// 复用查询邮箱和页面代次，账户结果不会落入本机 Overview 响应。
    pub(super) fn reload_cursor_account(&mut self, cx: &mut Context<Self>) {
        if self.dashboard.client != CURSOR_CLIENT { return }
        self.dashboard.revision += 1;
        self.dashboard.task.take();
        self.dashboard.last_read = std::time::Instant::now();
        self.dashboard.error = None;
        self.dashboard.loading = false;
        self.dashboard.scan_error = None;
        let Some(service) = &self.dashboard.service else { cx.notify(); return };
        let window = loki_metris_core::LocalUsageWindow::OVERVIEW_WINDOWS[self.dashboard.ranges[CURSOR_CLIENT]];
        let work = match service.query(UsageQuery::CursorAccount(window)) {
            Ok(work) => work,
            Err(error) => { self.dashboard.error = Some(error); cx.notify(); return; }
        };
        let revision = self.dashboard.revision;
        self.dashboard.loading = true;
        self.dashboard.task = Some(cx.spawn(async move |entity, cx| {
            let result = crate::usage_service::poll_receipt(work, cx.background_executor().clone(),
                std::time::Duration::from_millis(100)).await.unwrap_or(Err("usage_scan_cancelled"));
            let _ = entity.update(cx, |this, cx| {
                if this.dashboard.revision != revision || this.dashboard.client != CURSOR_CLIENT { return }
                this.dashboard.loading = false;
                this.dashboard.last_read = std::time::Instant::now();
                match result {
                    Ok(UsageResponse::CursorAccount(data)) => this.dashboard.cursor_account = Some(data),
                    result => {
                        this.dashboard.cursor_account = None;
                        this.dashboard.error = Some(result.err().unwrap_or("cursor_usage_transport"));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 账户概览沿用通用日期筛选和定时读取，不增加独立说明卡或刷新动作。
    pub(super) fn cursor_account_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut page = div().id("cursor-account-page").test_support().w_full().px_2().flex().flex_col().gap_4()
            .child(self.overview_window_controls(cx));
        if let Some(error) = self.dashboard.error {
            let key = match error {
                "cursor_usage_credentials" => "credentials",
                "cursor_usage_unauthorized" => "unauthorized",
                "cursor_usage_incomplete" => "incomplete",
                "cursor_usage_malformed" | "cursor_usage_overflow" => "malformed",
                _ => "transport",
            };
            page = page.child(div().text_color(cx.theme().danger).child(self.t(&format!("cursor_account.{key}"))));
        }
        if self.dashboard.loading {
            page = page.child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("dashboard.loading")));
        }
        let Some(data) = &self.dashboard.cursor_account else { return page.into_any_element() };
        page = page.child(div().flex().flex_wrap().gap_3().text_sm().text_color(cx.theme().muted_foreground)
            .child(format!("{} {} · {} {} / {}", self.t("cursor_account.events"), format::exact(data.counts.events),
                self.t("cursor_account.with_details"), format::exact(data.counts.events - data.counts.without_usage), format::exact(data.counts.events)))
            .child(format!("{} {}", self.t("statistics.observed"), format::observed(data.observed_at_ms, self.locale))))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t("cursor_account.partial_notice")));
        let counts = &data.counts;
        page = page.child(div().grid().grid_cols(if self.viewport_width >= 1200. {4} else {2}).gap_4()
            .children([("input", &counts.input), ("output", &counts.output), ("cache_read", &counts.cache_read), ("cache_write", &counts.cache_write)]
                .into_iter().map(|(key, count)| self.panel(cx)
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(self.t(&format!("cursor_account.{key}"))))
                    .child(div().text_size(px(24.)).font_weight(FontWeight::SEMIBOLD).child(self.cursor_tokens(count)))
                    .child(div().text_xs().child(self.cursor_coverage(count))))));
        page = page.child(self.cursor_group_table("cursor_account.daily", &data.daily, true, cx))
            .child(self.cursor_group_table("cursor_account.models", &data.models, false, cx));
        page.into_any_element()
    }

    /// 没有任何返回值时显示缺失；只有确实查询到空窗口才显示零。
    fn cursor_tokens(&self, field: &ReportedTokens) -> String {
        if field.reported == 0 && field.missing > 0 { self.t("dashboard.not_provided") }
        else { format::exact(field.tokens) }
    }

    /// 每字段分别显示覆盖数，部分合计不能被误读为完整总量。
    fn cursor_coverage(&self, field: &ReportedTokens) -> String {
        format!("{} {} / {}", self.t("cursor_account.reported"), field.reported, field.reported + field.missing)
    }

    /// 日期与模型复用同一分项列；不计算 GUI 侧总量。
    fn cursor_group_table(&self, title: &'static str, groups: &[CursorUsageGroup], daily: bool, cx: &Context<Self>) -> AnyElement {
        let head = TableHeader::new().child(TableRow::new().children([
            if daily { "statistics.date" } else { "statistics.model" }, "cursor_account.input", "cursor_account.output",
            "cursor_account.cache_read", "cursor_account.cache_write", "cursor_account.events"]
            .into_iter().map(|key| TableHead::new().px_3().py_3().child(self.t(key)))));
        let body = TableBody::new().children(groups.iter().map(|group| {
            let label = match &group.label {
                Some(label) if daily => format::calendar(label, self.locale),
                Some(label) => label.clone(),
                None => self.t("statistics.unknown_model"),
            };
            let row = TableRow::new().child(TableCell::new().px_3().py_3().child(label));
            self.cursor_count_cells(row, &group.counts)
        }));
        let table = Table::new().w_full().child(head).child(body);
        self.panel(cx).child(div().font_weight(FontWeight::SEMIBOLD).child(self.t(title)))
            .child(div().id(SharedString::from(title)).test_support().w_full().overflow_x_scroll().child(div().min_w(px(820.)).child(table)))
            .into_any_element()
    }

    /// 只投影五个数字列及字段覆盖，不在行或单元格代理刷新动作。
    fn cursor_count_cells(&self, row: TableRow, counts: &CursorTokenCounts) -> TableRow {
        row.children([&counts.input, &counts.output, &counts.cache_read, &counts.cache_write].into_iter().map(|field|
            TableCell::new().px_3().py_3().text_right().child(div().child(self.cursor_tokens(field))
                .when(field.missing > 0, |cell| cell.child(div().text_xs().child(self.cursor_coverage(field)))))))
            .child(TableCell::new().px_3().py_3().text_right().child(format::exact(counts.events)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, TestAppContext, point, test::TestWindowExt};
    use loki_metris_core::{AiTool, cursor_usage::{CursorUsagePages, CursorUsageRange}};

    /// 构造部分上报且包含安全模型名的真实 core 汇总。
    fn summary() -> std::sync::Arc<loki_metris_core::cursor_usage::CursorUsageSummary> {
        let at = "2026-10-22T00:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
        let mut pages = CursorUsagePages::new(CursorUsageRange { start_ms: at, end_ms: at + 1 }).unwrap();
        pages.push(1, &serde_json::json!({"totalUsageEventsCount":1,"usageEventsDisplay":[{
            "usageEventId":"fixture","timestamp":at.to_string(),"model":"model-a",
            "tokenUsage":{"inputTokens":12,"outputTokens":3,"cacheReadTokens":100}
        }]}).to_string()).unwrap();
        std::sync::Arc::new(pages.finish(at,&jiff::tz::TimeZone::UTC).unwrap())
    }

    /// Cursor 使用两张汇总表和通用日期筛选；空白区不代理按钮动作，离页保留窗口。
    #[gpui_kit::test]
    fn cursor_account_view_has_shared_owned_window_buttons(cx: &mut TestAppContext) {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 1400.);
        shell.update(cx, |this, cx| {
            this.agent_settings.value = Some(crate::monitor::settings::MonitorSettings {
                enabled_ai_tools: vec![AiTool::Cursor], ..Default::default()
            });
            this.dashboard.route = super::super::Route::Sources;
            this.select_usage_client(5, cx);
            assert_eq!(this.dashboard.route, super::super::Route::Overview);
            this.dashboard.cursor_account = Some(summary());
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("cursor-account-page").bounds().size.width > px(800.));
            assert!(window.find("cursor_account.daily").bounds().size.height > px(0.));
            assert!(window.find("cursor_account.models").bounds().size.height > px(0.));
            let revision = shell.read(cx).dashboard.revision;
            assert!(window.try_find("cursor-account-refresh").is_none());
            assert_eq!(window.find("usage-windows").bounds().origin.y, window.find("cursor-account-page").bounds().origin.y);
            window.click_at("usage-windows", point(px(1.),px(1.)), cx);
            assert_eq!(shell.read(cx).dashboard.revision, revision);
            window.click(("usage-window",2usize),cx);
            assert_eq!(shell.read(cx).dashboard.ranges[5],2);
            assert_eq!(shell.read(cx).dashboard.ranges[1],0);
            shell.update(cx,|this,cx| {
                this.page=crate::app::Page::Settings;
                this.page=crate::app::Page::Dashboard;
                assert_eq!(this.dashboard.ranges[5],2);
                this.dashboard.cursor_account=Some(summary());
                cx.notify();
            });
        }).unwrap();
        assert_eq!(super::super::Dashboard::default().ranges[5],0);
    }

    /// 宽窄窗口下账户和本机日期条位置、等分宽度及高度一致，失败时筛选仍可用。
    #[gpui_kit::test]
    fn cursor_account_window_geometry_matches_local_overview(cx: &mut TestAppContext) {
        for width in [960., 1440.] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 900.);
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let local_bar = window.find("usage-windows").bounds();
                let local_buttons: Vec<_> = (0usize..6).map(|index| window.find(("usage-window", index)).bounds()).collect();
                shell.update(cx, |this, cx| {
                    this.select_usage_client(5, cx);
                    this.dashboard.error = Some("cursor_usage_incomplete");
                    cx.notify();
                });
                window.render_frame(cx);
                let cursor_bar = window.find("usage-windows").bounds();
                assert_eq!(cursor_bar.origin.x, local_bar.origin.x);
                assert_eq!(cursor_bar.size, local_bar.size);
                for (index, local) in local_buttons.iter().enumerate() {
                    let bounds = window.find(("usage-window", index)).bounds();
                    assert_eq!(bounds.origin - cursor_bar.origin, local.origin - local_bar.origin);
                    assert_eq!(bounds.size, local.size);
                    assert_eq!(bounds.size.height, px(28.));
                    let width_difference = bounds.size.width - local_buttons[0].size.width;
                    assert!(width_difference > px(-1.) && width_difference < px(1.));
                }
                assert!(window.try_find("cursor-account-refresh").is_none());
                window.click(("usage-window", 1usize), cx);
                assert_eq!(shell.read(cx).dashboard.ranges[5], 1);
                assert!(shell.read(cx).dashboard.error.is_none());
            }).unwrap();
        }
    }

    /// 中文、英文翻译和缺失字段投影不将部分合计改写为零或完整总量。
    #[gpui_kit::test]
    fn cursor_account_translations_and_missing_values(cx: &mut TestAppContext) {
        let (_,shell,_finish)=crate::app::tests::open_shell(cx,960.,900.);
        shell.update(cx,|this,cx| {
            let data=summary();
            assert_eq!(this.cursor_tokens(&data.counts.cache_write),this.t("dashboard.not_provided"));
            assert_eq!(this.cursor_tokens(&data.counts.input),"12");
            for locale in ["zh-CN","en-US"] {
                for key in ["title","partial_notice","input","output","cache_read","cache_write","daily","models","credentials","unauthorized","incomplete","malformed","transport"] {
                    let key=format!("cursor_account.{key}");
                    assert_ne!(rust_i18n::t!(&key,locale=locale).to_string(),key);
                }
            }
            this.dashboard.loading=true;
            let revision=this.dashboard.revision;
            this.reload_cursor_account(cx);
            assert_eq!(this.dashboard.revision,revision,"stale account action cannot replace the local query");
        });
    }
}
