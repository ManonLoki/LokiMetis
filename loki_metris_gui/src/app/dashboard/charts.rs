//! 图表筛选与曲线选择属于视图会话；桶、分组与数值由 core 提供。

use crate::app::surface::{StaticCard, StaticMaterial};
use super::{Shell,format,plot::{self,Series,Value,Scale}};
use gpui_kit::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px, rgb,
    component::{ActiveTheme, button::{Button,ButtonVariants}, checkbox::Checkbox, menu::{DropdownMenu,PopupMenuItem}}};
use loki_metris_core::{UsageChartPage,UsageChartDimension,UsageMeasure,UsageGroupDisplay,WorkbuddyStatisticsSnapshot};
use std::{rc::Rc,cell::Cell};
use gpui_kit::base::TestSupportExt as _;

/// 各 Agent 的图表筛选和悬浮不改变用量统计页的筛选。
pub(in crate::app) struct ChartState {
    pub ranges:[usize;5], pub dimensions:[usize;5], pub visible:[[bool;6];5], pub metric:[usize;5],
    pub data:Option<UsageChartPage>, pub workbuddy:Option<WorkbuddyStatisticsSnapshot>,
    pub workbuddy_group:usize, pub workbuddy_metric:usize,
    pub hover:[Rc<Cell<Option<usize>>>;4],
}
impl Default for ChartState {
    /// 全部默认 Agent 分布；物理 Agent 默认模型，默认三条 Token 曲线。
    fn default()->Self { Self {ranges:[0;5],dimensions:[0,1,1,1,1],visible:[[true,true,false,false,true,false];5],metric:[0;5],
        data:None,workbuddy:None,workbuddy_group:0,workbuddy_metric:0,hover:std::array::from_fn(|_|Rc::new(Cell::new(None)))} }
}

/// 稳定的六个 Token 字段用于勾选曲线和分布指标。
const TOKEN_KEYS:[&str;6]=["total","input","cached","cache_write","output","reasoning_output"];
/// 颜色跟随参考图形固定语义，不随应用主题变色。
const TOKEN_COLORS:[u32;6]=[0x2563eb,0xf59e0b,0x0ea5e9,0x8b5cf6,0xef4444,0xd946ef];

impl Shell {
    /// 非全部页不能查询 Agent 维度；无推理客户端只在当前查询回退模型。
    pub(super) fn chart_dimension(&self)->UsageChartDimension {
        let index=self.dashboard.chart.dimensions[self.dashboard.client];
        if index==2 && self.dashboard.client>=3 {return UsageChartDimension::Model}
        [UsageChartDimension::Agent,UsageChartDimension::Model,UsageChartDimension::ReasoningEffort,
            UsageChartDimension::Project,UsageChartDimension::Thread,UsageChartDimension::Root][index]
    }

    /// 控件变化只替换当前查询，背景扫描继续独立运行。
    fn chart_controls(&self,cx:&mut Context<Self>)->AnyElement {
        let selected=self.dashboard.chart.ranges[self.dashboard.client];
        let ranges=div().flex().rounded(px(12.)).p_1().children(["today","yesterday","this_week","last_week","this_month","last_month"].into_iter().enumerate().map(|(index,key)|
            Button::new(("chart-window",index)).ghost().when(self.dashboard.client==4,|button|button.flex_1()).h(px(28.)).px_3().label(self.t(&format!("dashboard.{key}")))
                .when(selected==index,|button|button.font_weight(FontWeight::SEMIBOLD))
                .on_click(cx.listener(move|this,_,_,cx|{this.dashboard.chart.ranges[this.dashboard.client]=index;this.reload_usage(false,cx);} ))));
        if self.dashboard.client==4 {return ranges.bg(cx.theme().secondary).into_any_element()}
        let current=self.dashboard.chart.dimensions[self.dashboard.client];
        let current=if current==2 && self.dashboard.client>=3 {1}else{current};
        let labels:Vec<_>=["agent","model","reasoning","project","thread","root"].into_iter().enumerate()
            .filter(|(index,_)|(*index!=0 || self.dashboard.client==0) && (*index!=2 || self.dashboard.client<3))
            .map(|(index,key)|(index,self.t(&format!("statistics.{key}")))).collect();
        let value=labels.iter().find(|(index,_)|*index==current).map(|(_,label)|label.clone()).unwrap_or_default();
        let entity=cx.entity().downgrade();let client=self.dashboard.client;
        let select=Button::new("chart-dimension").outline().w(px(256.)).h(px(32.)).accessibility_label(self.t("statistics.dimension"))
            .child(div().w_full().flex().items_center().gap_2().child(div().w(px(68.)).text_xs().text_color(cx.theme().muted_foreground).child(self.t("statistics.dimension")))
                .child(div().flex_1().child(value)).child("⌄"))
            .dropdown_menu(move|mut menu,_,_|{for(index,label)in &labels {let index=*index;let entity=entity.clone();menu=menu.item(PopupMenuItem::new(label.clone()).checked(current==index).on_click(move|_,_,cx|{
                let _=entity.update(cx,|this,cx|{if this.dashboard.client==client {this.dashboard.chart.dimensions[client]=index;this.reload_usage(false,cx);}});
            }));}menu});
        StaticCard::new("charts-filter-bar", cx.theme().mode.is_dark(), StaticMaterial::FilterBar).test_support().px_3().py(px(10.)).flex_row().items_center().flex_wrap().gap_x_4().gap_y_3().child(ranges).child(select).into_any_element()
    }

    /// 三张原生图分别投影 Token、调用与固定排序的维度分布。
    pub(super) fn usage_charts(&self,cx:&mut Context<Self>)->AnyElement {
        let absent=if self.dashboard.client==4 {self.dashboard.chart.workbuddy.is_none()}else{self.dashboard.chart.data.is_none()};
        if self.dashboard.loading && absent {return self.usage_loading(cx)}
        let mut page=div().w_full().flex().flex_col().gap_8();
        if let Some(data)=&self.dashboard.chart.data {
            if data.index_state!=loki_metris_core::LocalIndexState::Ready {page=page.child(self.local_index_notice(data.index_state,cx));}
            if matches!(data.index_state,loki_metris_core::LocalIndexState::NotScanned|loki_metris_core::LocalIndexState::NeedsRescan) {return page.into_any_element()}
        }
        page=page.child(self.chart_controls(cx));
        if self.dashboard.client==4 {return page.child(self.workbuddy_charts(cx)).into_any_element()}
        let Some(data)=&self.dashboard.chart.data else {return page.into_any_element()};
        let aggregate=&data.fact.value;
        page=page.child(div().grid().grid_cols(if self.viewport_width>=1200.{4}else{2}).gap_4()
            .child(self.mini_metric("statistics.total",format::compact(aggregate.tokens.total_tokens),Some(aggregate.tokens.total_tokens),cx))
            .child(self.mini_metric("statistics.input",format::compact(aggregate.tokens.input_tokens),Some(aggregate.tokens.input_tokens),cx))
            .child(self.mini_metric("statistics.output",format::compact(aggregate.tokens.output_tokens),Some(aggregate.tokens.output_tokens),cx))
            .child(self.mini_metric("statistics.calls",format::exact(aggregate.call_count),None,cx)));
        let labels:Vec<_>=data.buckets.iter().map(|bucket|bucket.label.clone()).collect();
        let series=TOKEN_KEYS.iter().enumerate().filter(|(index,_)|self.dashboard.chart.visible[self.dashboard.client][*index]).map(|(index,key)|Series {
            label:self.t(&format!("statistics.{key}")),color:rgb(TOKEN_COLORS[index]).into(),
            values:data.buckets.iter().map(|bucket|token(&bucket.measure,index).map(Value::Count)).collect(),
        }).collect();
        let checkboxes=div().flex().flex_wrap().gap_4().children(TOKEN_KEYS.into_iter().enumerate().map(|(index,key)|
            Checkbox::new(("chart-series",index)).label(self.t(&format!("statistics.{key}"))).checked(self.dashboard.chart.visible[self.dashboard.client][index]).text_xs()
                .on_click(cx.listener(move|this,checked,_,cx|{
                    if *checked || this.dashboard.chart.visible[this.dashboard.client].iter().filter(|v|**v).count()>1 {
                        this.dashboard.chart.visible[this.dashboard.client][index]=*checked;cx.notify();
                    }
                }))));
        page=page.child(self.spot_panel("chart-token-trend", cx).test_support().child(self.chart_heading("charts.tokens",cx)).child(checkboxes)
            .child(self.plot_view("token-trend",labels.clone(),series,0,Scale::Tokens,cx)));
        let calls=vec![Series {label:self.t("statistics.calls"),color:rgb(0x16a34a).into(),values:data.buckets.iter().map(|bucket|Some(Value::Count(bucket.measure.call_count))).collect()}];
        page=page.child(self.spot_panel("chart-calls-trend", cx).test_support().child(self.chart_heading("charts.calls",cx)).child(self.plot_view("calls-trend",labels,calls,1,Scale::Count,cx)));
        let groups=data.groups.iter().chain(data.remainder.iter()).collect();
        page.child(self.distribution(groups,cx)).into_any_element()
    }

    /// 画布横向溢出保持最小宽度和同一几何比例。
    pub(super) fn plot_view(&self,id:&'static str,labels:Vec<String>,series:Vec<Series>,hover:usize,scale:Scale,cx:&Context<Self>)->AnyElement {
        let label=self.t(match id {"token-trend"=>"charts.tokens","calls-trend"=>"charts.calls","workbuddy-requests"=>"charts.requests","workbuddy-sessions"=>"charts.sessions","workbuddy-tokens"=>"charts.tokens",_=>"charts.credits"});
        div().id(SharedString::from(format!("{id}-scroll"))).overflow_x_scroll().w_full().child(plot::line(id,label,labels,series,self.dashboard.chart.hover[hover].clone(),
            plot::Palette::new(cx.theme().mode.is_dark()),"—".into(),scale)).into_any_element()
    }

    /// 图卡标题和时间粒度提示都使用当前真实查询窗口。
    pub(super) fn chart_heading(&self,key:&str,_cx:&Context<Self>)->AnyElement {
        let key=if self.dashboard.client==4 {format!("{key}_{}",if self.dashboard.chart.ranges[4]<2 {"hourly"}else{"daily"})}else{key.to_owned()};
        div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t(&key)).into_any_element()
    }

    /// 指标切换不重排 core 的前十名，确保查看同组时不会产生跳动。
    fn distribution(&self,groups:Vec<&UsageGroupDisplay>,cx:&mut Context<Self>)->AnyElement {
        let index=self.dashboard.chart.metric[self.dashboard.client];
        let entity=cx.entity().downgrade();
        let labels:Vec<_>=TOKEN_KEYS.into_iter().chain(["calls"]).enumerate().map(|(index,key)|(index,self.t(&format!("statistics.{key}")))).collect();
        let current=labels[index].1.clone();
        let select=Button::new("distribution-metric").outline().w(px(200.)).h(px(32.)).label(current).accessibility_label(self.t("charts.metric"))
            .dropdown_menu(move|mut menu,_,_|{for(index,label)in &labels {let index=*index;let entity=entity.clone();menu=menu.item(PopupMenuItem::new(label.clone()).on_click(move|_,_,cx|{
                let _=entity.update(cx,|this,cx|{this.dashboard.chart.metric[this.dashboard.client]=index;cx.notify();});
            }));}menu});
        let values:Vec<_>=groups.iter().map(|group|if index==6 {Some(group.measure.call_count)}else{token(&group.measure,index)}).collect();
        let rows=groups.into_iter().zip(values).map(|(group,value)|plot::Bar {
            label:match group.disambiguation_index {Some(index)=>format!("{} ({index})",self.display_label(&group.label,group.label_code)),None=>self.display_label(&group.label,group.label_code)},
            value:value.map(Value::Count),
            display:value.map(|v|if index==6 {format::exact(v)}else{format!("{} · {}",format::compact(v),format::exact(v))}).unwrap_or_else(||self.t("dashboard.not_provided")),
            share:format!("{} {}",self.t("charts.token_share"),self.basis_points(group.total_token_share_basis_points)),remainder:group.remainder,
        }).collect::<Vec<_>>();
        let mut panel=self.spot_panel("chart-distribution", cx).test_support().child(div().flex().items_center().justify_between().child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t("charts.distribution"))).child(select));
        if rows.is_empty() {panel=panel.child(self.t("charts.no_distribution"));}
        else {panel=panel.child(div().id("distribution-scroll").overflow_x_scroll().w_full().child(plot::bars("distribution",rows,plot::Palette::new(cx.theme().mode.is_dark()),if index==6 {Scale::Count}else{Scale::Tokens})));}
        panel.into_any_element()
    }
}

/// Token 缺失值保持 Option，既不补零也不跨缺失桶连线。
fn token(value:&UsageMeasure,index:usize)->Option<u64> {
    let t=&value.tokens;match index {0=>Some(t.total_tokens),1=>Some(t.input_tokens),2=>t.cached_input_tokens,3=>t.cache_write_input_tokens,4=>Some(t.output_tokens),_=>t.reasoning_output_tokens}
}
