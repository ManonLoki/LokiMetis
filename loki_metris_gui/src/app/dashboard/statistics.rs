//! 原生统计页：筛选保留会话状态，表格直接投影 core 日桶和前十分组。

use gpui_kit::base::TestSupportExt as _;
use crate::app::surface::{BrandTableRow, StaticCard, StaticMaterial};
use super::{Shell, format};
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgb,
    component::{ActiveTheme, button::{Button, ButtonVariants}, menu::{DropdownMenu, PopupMenuItem},
    table::{Table, TableHeader, TableHead, TableBody, TableRow, TableCell}}};
use loki_metris_core::{UsageDimension, UsageStatisticsPage, WorkBuddyModelUsageWindow, Completeness, Confidence};

/// 页面路由独立于所选 Agent；具体筛选按 Agent 隔离。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum Route {
    /// 概览。
    Overview,
    /// 用量统计。
    Statistics,
    /// 图表。
    Charts,
    /// 调用记录（仅“全部”）。
    Calls,
    /// 数据源。
    Sources,
    /// 用量设置。
    Settings,
}

/// 统计结果只是查询快照，时间和分组选择属于进程会话。
pub(in crate::app) struct DetailState {
    /// 各客户端统计页选中的窗口下标。
    pub ranges: [usize; 5],
    /// 各客户端统计页选中的分组维度下标。
    pub dimensions: [usize; 5],
    /// 当前统计结果；切换客户端时清空。
    pub statistics: Option<UsageStatisticsPage>,
    /// WorkBuddy 逐模型用量；仅 WorkBuddy 使用。
    pub models: Option<WorkBuddyModelUsageWindow>,
}
impl Default for DetailState {
    /// 用量默认本周/模型，与概览的今日保持独立。
    fn default() -> Self { Self { ranges: [2; 5], dimensions: [0; 5], statistics: None, models: None } }
}

impl Shell {
    /// Grok/WorkBuddy 不提供推理维度，仅当前查询回退模型。
    pub(super) fn statistics_dimension(&self) -> UsageDimension {
        let index = self.dashboard.detail.dimensions[self.dashboard.client];
        if index == 1 && self.dashboard.client >= 3 { return UsageDimension::Model }
        [UsageDimension::Model, UsageDimension::ReasoningEffort, UsageDimension::Project, UsageDimension::Thread, UsageDimension::Root][index]
    }

    /// 用量、图表后续共享同一筛选材质，但各自状态仍独立。
    fn statistics_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.dashboard.detail.ranges[self.dashboard.client];
        let ranges = div().flex().rounded(px(12.)).bg(cx.theme().secondary).p_1()
            .children(super::NATURAL_WINDOW_KEYS.into_iter().enumerate().map(|(index,key)| {
                Button::new(("statistics-window",index)).ghost().h(px(28.)).px_3()
                    .label(self.t(&format!("dashboard.{key}"))).accessibility_label(self.t(&format!("dashboard.{key}")))
                    .when(selected == index, |button| button.font_weight(FontWeight::SEMIBOLD))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.dashboard.detail.ranges[this.dashboard.client] = index;
                        this.reload_usage(false, cx);
                    }))
            }));
        let selected = self.dashboard.detail.dimensions[self.dashboard.client];
        let effective = if selected == 1 && self.dashboard.client >= 3 { 0 } else { selected };
        let labels: Vec<_> = ["model", "reasoning", "project", "thread", "root"].into_iter().enumerate()
            .filter(|(index,_)| *index != 1 || self.dashboard.client < 3)
            .map(|(index,key)| (index,self.t(&format!("statistics.{key}")))).collect();
        let current = labels.iter().find(|(index,_)| *index == effective).map(|(_,label)| label.clone()).unwrap_or_default();
        let shell = cx.entity().downgrade();
        let client = self.dashboard.client;
        let select = Button::new("statistics-dimension").outline().h(px(32.)).w(px(256.))
            .accessibility_label(self.t("statistics.dimension"))
            .child(div().w_full().flex().items_center().gap_2()
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(self.t("statistics.dimension")))
                .child(div().flex_1().child(current)).child("⌄"))
            .dropdown_menu(move |mut menu, _, _| {
                for (index,label) in &labels {
                    let shell=shell.clone(); let index=*index;
                    menu=menu.item(PopupMenuItem::new(label.clone()).checked(index==effective).on_click(move |_,_,cx| {
                        let _=shell.update(cx,|this,cx| { if this.dashboard.client == client {
                            this.dashboard.detail.dimensions[client]=index;this.reload_usage(false,cx);
                        }});
                    }));
                }
                menu
            });
        let mut bar = StaticCard::new("statistics-filter-bar", cx.theme().mode.is_dark(), StaticMaterial::FilterBar).test_support().py(px(10.)).px_3().flex_row().flex_wrap().items_center().gap_x_4().gap_y_3()
            .child(ranges).child(select);
        if let Some(data) = &self.dashboard.detail.statistics {
            bar = bar.child(div().flex_1()).child(div().flex().items_center().gap_2()
                .child(self.quality_badge(self.t(match data.fact.completeness {Completeness::Complete=>"statistics.complete",Completeness::Partial=>"dashboard.partial",Completeness::Unknown=>"statistics.unknown_coverage"}),
                    rgb(if data.fact.completeness == Completeness::Complete { 0x12b886 } else { 0xfab005 }).into()))
                .child(self.quality_badge(self.t(match data.fact.confidence {Confidence::Exact=>"dashboard.local_fact",Confidence::Derived=>"dashboard.derived",Confidence::Suspected=>"dashboard.suspected"}),rgb(if data.fact.confidence == Confidence::Exact {0x12b886}else{0xfab005}).into()))
                .child(div().text_xs().text_color(cx.theme().muted_foreground).child(if self.dashboard.loading {
                    self.t("statistics.updating")
                } else { format!("{} {}",self.t("statistics.observed"),format::observed(data.observed_at_epoch_ms,self.locale)) })));
        }
        bar.into_any_element()
    }

    /// 时间和分组变化保留上一快照，成功后整体替换，不在前台自行统计。
    pub(super) fn usage_statistics(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.dashboard.loading && self.dashboard.detail.statistics.is_none() { return self.usage_loading(cx) }
        let mut page=div().w_full().flex().flex_col().gap_5();
        if let Some(data)=&self.dashboard.detail.statistics&& data.index_state!=loki_metris_core::LocalIndexState::Ready {page=page.child(self.local_index_notice(data.index_state,cx));}
        page=page.child(self.statistics_controls(cx));
        let Some(data)=&self.dashboard.detail.statistics else { return page.into_any_element() };
        let value=&data.fact.value;
        page=page.child(div().w_full().grid().grid_cols(if self.viewport_width>=1200. {4}else{2}).gap_4()
            .child(self.mini_metric("statistics.total",format::compact(value.tokens.total_tokens),Some(value.tokens.total_tokens),cx))
            .child(self.mini_metric("statistics.unique_calls",format::exact(value.call_count),None,cx))
            .child(self.mini_metric("statistics.unique_threads",format::exact(value.thread_count),None,cx))
            .child(self.mini_metric("dashboard.cache_ratio",if value.tokens.cached_input_tokens.is_none() {self.t("dashboard.not_provided")}else{self.basis_points(value.cache_read_basis_points)},None,cx)));
        let header=TableHeader::new().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(cx.theme().foreground).child(TableRow::new().children(["date","total","input","cached","output","calls"].into_iter().enumerate().map(|(index,key)|
            TableHead::new().px(px(10.)).py_3().when(index==0,|head|head.w(gpui_kit::relative(0.22)).flex_shrink_0()).when(index>0,|head|head.text_right()).child(self.t(&format!("statistics.{key}"))))));
        let body=TableBody::new().children(data.daily_buckets.iter().enumerate().map(|(index,bucket)| {
            let tokens=&bucket.measure.tokens;
            BrandTableRow::new(SharedString::from(format!("statistics-daily-row-{}",bucket.local_date)),cx.theme().mode.is_dark())
                .bottom_corner_radius(if index + 1 == data.daily_buckets.len() { px(19.) } else { px(0.) })
                .child(TableCell::new().w(gpui_kit::relative(0.22)).flex_shrink_0().px(px(10.)).py_3().child(div().flex().items_center().gap_2().text_size(px(16.))
                    .child(format::calendar(&bucket.local_date,self.locale))
                    .when(bucket.in_progress, |row| row.child(self.quality_badge(self.t("statistics.in_progress"),rgb(0xfab005).into())))))
                .child(self.table_token(Some(tokens.total_tokens),cx))
                .child(self.table_token(Some(tokens.input_tokens),cx))
                .child(self.table_token(tokens.cached_input_tokens,cx))
                .child(self.table_token(Some(tokens.output_tokens),cx))
                .child(TableCell::new().px(px(10.)).py_3().text_right().child(format::exact(bucket.measure.call_count)))
        }));
        page=page.child(self.table_panel("statistics.daily",Table::new().w_full().text_size(px(16.)).font_weight(FontWeight::NORMAL).text_color(cx.theme().foreground).child(header).child(body).into_any_element(),760.,cx));
        let dimension_key=match self.statistics_dimension() { UsageDimension::Model=>"model",UsageDimension::ReasoningEffort=>"reasoning",UsageDimension::Project=>"project",UsageDimension::Thread=>"thread",UsageDimension::Root=>"root" };
        let header=TableHeader::new().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(cx.theme().foreground).child(TableRow::new().children([
            self.t(&format!("statistics.{dimension_key}")),self.t("statistics.total"),self.t("statistics.share"),self.t("statistics.calls"),
            self.t("dashboard.cache_ratio"),self.t("statistics.quality")].into_iter().enumerate().map(|(index,label)| TableHead::new().px(px(10.)).py_3().when(index==0,|head|head.w(gpui_kit::relative(0.22)).flex_shrink_0()).when((1..5).contains(&index),|head|head.text_right()).child(label))));
        let groups=data.groups.iter().chain(data.remainder.iter());
        let group_count=data.groups.len()+usize::from(data.remainder.is_some());
        let body=TableBody::new().children(groups.enumerate().map(|(index,group)| {
            BrandTableRow::new(SharedString::from(format!("statistics-group-row-{}",group.id)),cx.theme().mode.is_dark())
                .bottom_corner_radius(if index + 1 == group_count { px(19.) } else { px(0.) })
                .child(TableCell::new().w(gpui_kit::relative(0.22)).flex_shrink_0().px(px(10.)).py_3().child(div().text_size(px(16.)).when(group.remainder, |row| row.font_weight(FontWeight::BOLD))
                    .child(match group.disambiguation_index {Some(index)=>format!("{} ({index})",self.display_label(&group.label,group.label_code)),None=>self.display_label(&group.label,group.label_code)})))
                .child(self.table_token(Some(group.measure.tokens.total_tokens),cx))
                .child(TableCell::new().px(px(10.)).py_3().text_right().child(self.basis_points(group.total_token_share_basis_points)))
                .child(TableCell::new().px(px(10.)).py_3().text_right().child(format::exact(group.measure.call_count)))
                .child(TableCell::new().px(px(10.)).py_3().text_right().child(if group.measure.tokens.cached_input_tokens.is_none() {self.t("dashboard.not_provided")}else{self.basis_points(group.measure.cache_read_basis_points)}))
                .child(TableCell::new().px(px(10.)).py_3().child(self.t(match group.measure.confidence {Confidence::Exact=>"dashboard.local_fact",Confidence::Derived=>"dashboard.derived",Confidence::Suspected=>"dashboard.suspected"})))
        }));
        page=page.child(self.table_panel("statistics.groups",Table::new().w_full().text_size(px(16.)).font_weight(FontWeight::NORMAL).text_color(cx.theme().foreground).child(header).child(body).into_any_element(),760.,cx));
        if let Some(models)=&self.dashboard.detail.models {
            let header=TableHeader::new().text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(cx.theme().foreground).child(TableRow::new().children(["actual_model","total","input","cached","uncached","output","requests","top_level","subagent","credits"].into_iter().enumerate().map(|(index,key)| TableHead::new().px(px(10.)).py_3().when(index==0,|head|head.w(gpui_kit::relative(0.22)).flex_shrink_0()).when(index>0,|head|head.text_right()).child(self.t(&format!("statistics.{key}"))))));
            let body=TableBody::new().children(models.groups.iter().enumerate().map(|(index,group)| BrandTableRow::new(SharedString::from(format!("workbuddy-model-row-{:?}",group.model)),cx.theme().mode.is_dark())
                .bottom_corner_radius(if index + 1 == models.groups.len() { px(19.) } else { px(0.) })
                .child(TableCell::new().w(gpui_kit::relative(0.22)).flex_shrink_0().px(px(10.)).py_3().font_weight(FontWeight::BOLD).child(group.model.clone().unwrap_or_else(||self.t("statistics.unassigned"))))
                .child(self.table_token(Some(group.total_tokens),cx)).child(self.table_token(Some(group.input_tokens),cx))
                .child(self.table_token(Some(group.cached_input_tokens),cx)).child(self.table_token(Some(group.uncached_input_tokens),cx)).child(self.table_token(Some(group.output_tokens),cx))
                .children([group.call_count,group.top_level_call_count,group.subagent_call_count].map(|count|TableCell::new().px(px(10.)).py_3().text_right().child(format::exact(count))))
                .child(TableCell::new().px(px(10.)).py_3().text_right().child(group.credits.map(format::credits).unwrap_or_else(||self.t("dashboard.not_provided"))))))
                .when(models.groups.is_empty(),|body|body.child(BrandTableRow::new("workbuddy-model-empty-row",cx.theme().mode.is_dark()).bottom_corner_radius(px(19.)).child(TableCell::new().col_span(10).px_5().py_4().text_color(cx.theme().muted_foreground).child(self.t("statistics.no_models")))));
            let table=Table::new().w_full().text_size(px(16.)).font_weight(FontWeight::NORMAL).text_color(cx.theme().foreground).child(header).child(body);
            page=page.child(StaticCard::new("workbuddy-model-table-panel", cx.theme().mode.is_dark(), StaticMaterial::Table).test_support().p_0().gap_0().mt_3()
                .child(div().p_5().flex().items_center().justify_between().child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t("statistics.actual_models")))
                    .child(self.quality_badge(self.t("statistics.model_basis"),cx.theme().primary)))
                .child(div().id("workbuddy-model-table").overflow_x_scroll().w_full().child(div().min_w(px(1320.)).child(table))));
        }
        page.into_any_element()
    }

    /// 表格 Token 采用同一整数的紧凑值与精确值，缺失不会补零。
    fn table_token(&self,value:Option<u64>,cx:&Context<Self>) -> TableCell {
        let Some(value)=value else {return TableCell::new().px(px(10.)).py_3().text_right().font_weight(FontWeight::NORMAL).text_color(cx.theme().muted_foreground).child(self.t("dashboard.not_provided"))};
        TableCell::new().px(px(10.)).py_3().text_right().child(div().flex().flex_col().items_end().text_right().font_family("Geist Mono")
            .child(div().text_size(px(16.)).line_height(gpui_kit::relative(1.55)).font_weight(FontWeight::BOLD).child(format::compact(value)))
            .child(div().text_xs().line_height(gpui_kit::relative(1.4)).font_weight(FontWeight::NORMAL).text_color(cx.theme().muted_foreground).child(format::exact(value))))
    }

    /// 固定宽表在小窗口仅横向滚动，保持参考列顺序和读数对齐。
    fn table_panel(&self,title:&str,table:AnyElement,width:f32,cx:&Context<Self>) -> AnyElement {
        StaticCard::new(SharedString::from(format!("{title}-panel")), cx.theme().mode.is_dark(), StaticMaterial::Table).test_support().p_0().gap_0().child(div().p_5().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t(title)))
            .child(div().id(SharedString::from(title.to_owned())).overflow_x_scroll().w_full().child(div().min_w(px(width)).child(table)))
            .into_any_element()
    }
}
