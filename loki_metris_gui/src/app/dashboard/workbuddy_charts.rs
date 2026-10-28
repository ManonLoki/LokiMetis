//! WorkBuddy 保持独立的请求、会话、积分与 Trace 图形，绝不伪装成模型图表。

use super::{Shell,format,plot::{self,Series,Value,Scale}};
use gpui_kit::{AnyElement,Context,FontWeight,div,prelude::*,px,rgb,
    component::{ActiveTheme,button::Button,menu::{DropdownMenu,PopupMenuItem}}};
use loki_metris_core::WorkBuddyDailyBucket;
use gpui_kit::base::TestSupportExt as _;

impl Shell {
    /// 八个摘要和四条独立趋势来自同一 WorkBuddy 查询快照。
    pub(super) fn workbuddy_charts(&self,cx:&mut Context<Self>)->AnyElement {
        let Some(data)=&self.dashboard.chart.workbuddy else {return div().into_any_element()};
        let window=loki_metris_core::LocalUsageWindow::OVERVIEW_WINDOWS[self.dashboard.chart.ranges[4]];
        let Some(selected)=data.windows.iter().find(|value|value.window==window) else {return div().into_any_element()};
        let mut page=div().w_full().flex().flex_col().gap_8();
        let metrics=[("statistics.total",selected.tokens), ("statistics.input",selected.input_tokens),
            ("statistics.cached",selected.cached_input_tokens),("statistics.uncached",selected.uncached_input_tokens),("statistics.output",selected.output_tokens)];
        page=page.child(div().grid().grid_cols(if self.viewport_width>=1200.{4}else{2}).gap_4()
            .children(metrics.into_iter().map(|(key,value)|self.mini_metric(key,format::compact(value),Some(value),cx)))
            .child(self.mini_metric("workbuddy.requests",format::exact(selected.request_count),None,cx))
            .child(self.mini_metric("charts.summary_sessions",format::exact(selected.session_count),None,cx))
            .child(self.mini_metric("workbuddy.credits",selected.credits.map(format::credits).unwrap_or_else(||self.t("dashboard.not_provided")),None,cx)));
        let daily:Vec<_>=data.daily_buckets.iter().filter(|bucket|selected.dates.contains(&bucket.date)).collect();
        let hourly=data.hourly_trends.iter().find(|trend|trend.window==window);
        let labels:Vec<_>=if let Some(trend)=hourly {trend.buckets.iter().map(|bucket|bucket.label.clone()).collect()}
            else {daily.iter().map(|bucket|bucket.date.get(5..).unwrap_or(&bucket.date).to_owned()).collect()};
        if labels.is_empty() {page=page.child(self.spot_panel("workbuddy-no-daily", cx).test_support().child(self.t("charts.no_daily")));}
        else {
            for (index,(key,id,color,scale)) in [
                ("charts.requests","workbuddy-requests",0x7c3aed,Scale::Count),
                ("charts.sessions","workbuddy-sessions",0x2563eb,Scale::Count),
                ("charts.tokens","workbuddy-tokens",0x16a34a,Scale::Tokens),
                ("charts.credits","workbuddy-credits",0xf59e0b,Scale::Credits),
            ].into_iter().enumerate() {
                let values=if let Some(trend)=hourly {trend.buckets.iter().map(|bucket|match index {
                    0=>Some(Value::Count(bucket.request_count)),1=>Some(Value::Count(bucket.session_count)),
                    2=>Some(Value::Count(bucket.tokens)),_=>bucket.credits.map(Value::Decimal),
                }).collect()}else{daily.iter().map(|bucket|match index {
                    0=>Some(Value::Count(bucket.request_count)),1=>Some(Value::Count(bucket.session_count)),
                    2=>Some(Value::Count(bucket.tokens)),_=>bucket.credits.map(Value::Decimal),
                }).collect()};
                let series=vec![Series {label:self.t(match index {0=>"workbuddy.requests",1=>"workbuddy.sessions",2=>"charts.legend_tokens",_=>"workbuddy.credits"}),color:rgb(color).into(),values}];
                page=page.child(self.spot_panel(gpui_kit::SharedString::from(format!("chart-{id}")), cx).test_support().child(self.chart_heading(key,cx)).child(self.plot_view(id,labels.clone(),series,index,scale,cx)));
            }
        }
        page.child(self.workbuddy_distribution(daily,selected,cx)).into_any_element()
    }

    /// 民用日与 Trace 状态互斥；切换分组回到该分组的首个合法指标。
    fn workbuddy_distribution(&self,daily:Vec<&WorkBuddyDailyBucket>,window:&loki_metris_core::WorkBuddyWindowAggregate,cx:&mut Context<Self>)->AnyElement {
        let group=self.dashboard.chart.workbuddy_group;
        let selected=self.dashboard.chart.workbuddy_metric;
        let entity=cx.entity().downgrade();
        let groups=[self.t("charts.civil_day"),self.t("charts.trace_status")];
        let group_select=Button::new("workbuddy-chart-group").outline().w(px(200.)).label(groups[group].clone()).accessibility_label(self.t("statistics.dimension"))
            .dropdown_menu(move|mut menu,_,_|{for(index,label)in groups.iter().enumerate(){let entity=entity.clone();menu=menu.item(PopupMenuItem::new(label.clone()).on_click(move|_,_,cx|{
                let _=entity.update(cx,|this,cx|{this.dashboard.chart.workbuddy_group=index;this.dashboard.chart.workbuddy_metric=0;cx.notify();});
            }));}menu});
        let keys=if group==0 {vec!["statistics.total","statistics.input","statistics.cached","statistics.uncached","statistics.output","workbuddy.requests","workbuddy.sessions","workbuddy.credits"]}else{vec!["workbuddy.traces"]};
        let labels:Vec<_>=keys.into_iter().map(|key|self.t(key)).collect();let entity=cx.entity().downgrade();
        let metric_select=Button::new("workbuddy-chart-metric").outline().w(px(200.)).label(labels[selected].clone()).accessibility_label(self.t("charts.metric"))
            .dropdown_menu(move|mut menu,_,_|{for(index,label)in labels.iter().enumerate(){let entity=entity.clone();menu=menu.item(PopupMenuItem::new(label.clone()).on_click(move|_,_,cx|{
                let _=entity.update(cx,|this,cx|{this.dashboard.chart.workbuddy_metric=index;cx.notify();});
            }));}menu});
        let rows:Vec<(String,Option<Value>,String)>=if group==0 {daily.iter().map(|bucket|{
            let value=match selected {0=>Some(Value::Count(bucket.tokens)),1=>Some(Value::Count(bucket.input_tokens)),2=>Some(Value::Count(bucket.cached_input_tokens)),
                3=>Some(Value::Count(bucket.uncached_input_tokens)),4=>Some(Value::Count(bucket.output_tokens)),5=>Some(Value::Count(bucket.request_count)),
                6=>Some(Value::Count(bucket.session_count)),_=>bucket.credits.map(Value::Decimal)};
            let label=value.map(Value::display).unwrap_or_else(||self.t("dashboard.not_provided"));
            (bucket.date.clone(),value,label)
        }).collect()}else{[window.trace_total_count.saturating_sub(window.trace_error_count).saturating_sub(window.trace_cancelled_count),window.trace_error_count,window.trace_cancelled_count]
            .into_iter().zip(["charts.trace_ok","charts.trace_error","charts.trace_cancelled"]).map(|(v,key)|(self.t(key),Some(Value::Count(v)),format::exact(v))).collect()};
        let total=rows.iter().filter_map(|(_,value,_)|*value).map(Value::number).sum::<f64>();
        let rows=rows.into_iter().map(|(label,value,display)|plot::Bar {label,value,display,detail:None,share:value.map(|value|format!("{}：{}",self.t("charts.window_share"),self.basis_points(if total>0. {Some((value.number()*10000./total).round().clamp(0.,10000.) as u16)}else{Some(0)}))).unwrap_or_else(||self.t("dashboard.not_provided")),remainder:false}).collect::<Vec<_>>();
        let controls=div().flex().items_end().gap_4()
            .child(div().flex().flex_col().gap_2().text_sm().child(self.t("statistics.dimension")).child(group_select))
            .child(div().flex().flex_col().gap_2().text_sm().child(self.t("charts.metric")).child(metric_select));
        let mut panel=self.spot_panel("chart-workbuddy-distribution", cx).test_support().child(div().flex().items_center().justify_between().child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t("charts.distribution"))));
        if rows.is_empty() || (group==1 && window.trace_total_count==0) {panel=panel.child(self.t("charts.no_distribution"));}
        else {panel=panel.child(div().id("workbuddy-distribution-scroll").overflow_x_scroll().child(plot::bars("workbuddy-distribution",rows,plot::Palette::new(cx.theme().mode.is_dark()),if group==0 && selected==7 {Scale::Credits}else{Scale::Count})));}
        div().flex().flex_col().gap_8().child(controls).child(panel).into_any_element()
    }
}
