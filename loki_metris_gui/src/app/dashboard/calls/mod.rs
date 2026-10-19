//! 调用页直接消费 core 固定页大小与不透明游标，不对已加载行二次排序。

mod column_widths;

use column_widths::CallWidthCache;
use crate::app::surface::{BrandTableRow, StaticCard, StaticMaterial};
use super::{Shell,format};
use gpui_kit::base::TestSupportExt as _;
use crate::usage_service::{UsageQuery,UsageResponse};
use gpui_kit::{AnyElement,Context,FontWeight,SharedString,div,prelude::*,px,
    component::{ActiveTheme,Disableable,button::{Button,ButtonVariants},menu::{DropdownMenu,PopupMenuItem},
        table::{Table,TableHeader,TableHead,TableBody,TableRow,TableCell}}};
use loki_metris_core::{UsageCallsQuery,UsageCallsPage,UsageCallFilters,UsageFilterOption,UsageCallItem,
    UsageCallSortField as Field,UsageCallSortDirection as Direction,LocalIndexState,SourceClientKind};

/// 会话筛选按 Agent 独立；结果链只保留当前 Agent 的可信页面。
pub(in crate::app) struct CallsState {
    pub queries:[UsageCallsQuery;5],
    pub page:Option<UsageCallsPage>,
    pub loading_next:bool,
    pub replacement_failed:bool,
    pub next_failed:bool,
    /// 每次整页替换递增，使列宽缓存不会沿用上一结果链的测量。
    pub replacement:u64,
    /// 渲染路径只读借用 Shell，列宽缓存经内部可变性增量更新。
    widths:std::cell::RefCell<CallWidthCache>,
}
impl Default for CallsState {
    /// 首次按时间降序，每个筛选均为空，不提供自定义页大小。
    fn default()->Self {Self {queries:std::array::from_fn(|_|UsageCallsQuery::default()),page:None,
        loading_next:false,replacement_failed:false,next_failed:false,replacement:0,widths:Default::default()}}
}

impl Shell {
    /// 首屏替换与追加共用请求代数；过期响应不能污染另一查询链。
    pub(super) fn load_calls(&mut self,next:bool,cx:&mut Context<Self>) {
        if next && (self.dashboard.loading || self.dashboard.calls.loading_next) {return}
        let client=self.dashboard.client;
        let mut query=self.dashboard.calls.queries[client].clone();
        if client==3 {query.filters.reasoning_effort=None;}
        if next {query.cursor=self.dashboard.calls.page.as_ref().and_then(|page|page.next_cursor.clone());if query.cursor.is_none(){return}}
        else {query.cursor=None;}
        self.dashboard.revision=self.dashboard.revision.wrapping_add(1);
        self.dashboard.task.take();self.dashboard.error=None;
        self.dashboard.calls.replacement_failed=false;self.dashboard.calls.next_failed=false;
        self.dashboard.loading=false;self.dashboard.calls.loading_next=false;
        let Some(service)=&self.dashboard.service else {cx.notify();return};
        let receiver=match service.query(UsageQuery::Calls(self.usage_client(),query)) {
            Ok(receiver)=>receiver,Err(error)=>{
                if next {self.dashboard.calls.next_failed=true;}else if self.dashboard.calls.page.is_some(){self.dashboard.calls.replacement_failed=true;}
                else {self.dashboard.error=Some(error);}cx.notify();return
            }
        };
        let revision=self.dashboard.revision;
        self.dashboard.loading = !next;self.dashboard.calls.loading_next=next;
        self.dashboard.task=Some(cx.spawn(async move|entity,cx| {
            let result=crate::usage_service::poll_receipt(receiver,cx.background_executor().clone(),std::time::Duration::from_millis(50)).await.unwrap_or(Err("usage_query_failed"));
            let _=entity.update(cx,|this,cx| {
                if this.dashboard.revision!=revision || this.dashboard.client!=client {return}
                this.dashboard.loading=false;this.dashboard.calls.loading_next=false;
                match result {
                    Ok(UsageResponse::Calls(mut page))=>{
                        if next {if let Some(current)=&mut this.dashboard.calls.page {
                            current.items.append(&mut page.items);current.total_count=page.total_count;current.next_cursor=page.next_cursor;
                        }}else{this.dashboard.calls.page=Some(page);this.dashboard.calls.replacement=this.dashboard.calls.replacement.wrapping_add(1);}
                    }
                    _=>{if next {this.dashboard.calls.next_failed=true;}
                        else if this.dashboard.calls.page.is_some() {this.dashboard.calls.replacement_failed=true;}
                        else {this.dashboard.error=Some("usage_query_failed");}}
                }
                cx.notify();
            });
        }));cx.notify();
    }

    /// 固定筛选字段通过后端枚举建立选项；清除只清筛选，不重置排序。
    pub(super) fn usage_calls(&self,window:&gpui_kit::Window,cx:&mut Context<Self>)->AnyElement {
        if self.dashboard.loading && self.dashboard.calls.page.is_none() {return self.usage_loading(cx)}
        let Some(page)=&self.dashboard.calls.page else {return div().into_any_element()};
        let mut column=div().w_full().flex().flex_col().gap_8();
        if page.index_state!=LocalIndexState::Ready {column=column.child(self.local_index_notice(page.index_state,cx));}
        if matches!(page.index_state,LocalIndexState::NotScanned|LocalIndexState::NeedsRescan) {return column.into_any_element()}
        let filters=StaticCard::new("calls-filters", cx.theme().mode.is_dark(), StaticMaterial::Panel).test_support().gap_4().p_5().child(div().flex().items_center().justify_between()
            .child(div().text_size(px(16.)).font_weight(FontWeight::BOLD).child(self.t("calls.filters")))
            .child(Button::new("calls-clear").ghost().label(self.t("calls.clear")).on_click(cx.listener(|this,_,_,cx|{
                this.dashboard.calls.queries[this.dashboard.client].filters=UsageCallFilters::default();this.load_calls(false,cx);
            }))))
            .child(div().grid().grid_cols(if self.viewport_width>=1408. {5}else{2}).gap_4()
                .child(self.call_filter(0,"statistics.model",&page.available_filters.models,cx))
                .when(self.dashboard.client!=3,|grid|grid.child(self.call_filter(1,"statistics.reasoning",&page.available_filters.reasoning_efforts,cx)))
                .child(self.call_filter(2,"statistics.project",&page.available_filters.projects,cx))
                .child(self.call_filter(3,"statistics.thread",&page.available_filters.threads,cx))
                .child(self.call_filter(4,"statistics.root",&page.available_filters.roots,cx)));
        column=column.child(filters);
        if self.dashboard.calls.replacement_failed {column=column.child(self.calls_failure(false,cx));}
        column=column.child(self.calls_table(page,window,cx));
        if self.dashboard.calls.next_failed {column=column.child(self.calls_failure(true,cx));}
        column.into_any_element()
    }

    /// 下拉显示安全标签，提交稳定 ID；空值始终对应“全部”。
    fn call_filter(&self,index:usize,key:&str,values:&[UsageFilterOption],cx:&Context<Self>)->AnyElement {
        let filters=&self.dashboard.calls.queries[self.dashboard.client].filters;
        let current=match index {0=>&filters.model,1=>&filters.reasoning_effort,2=>&filters.project,3=>&filters.thread,_=>&filters.root};
        let mut options=vec![(None,self.t("dashboard.all"))];
        options.extend(values.iter().map(|value|(Some(value.id.clone()),match value.disambiguation_index {
            Some(i)=>format!("{} ({i})",self.display_label(&value.label,value.label_code)),None=>self.display_label(&value.label,value.label_code)})));
        let label=options.iter().find(|(id,_)|id==current).map(|(_,label)|label.clone()).unwrap_or_else(||self.t("dashboard.all"));
        let current=current.clone();let client=self.dashboard.client;let entity=cx.entity().downgrade();
        div().flex().flex_col().gap_1().text_sm().child(self.t(key))
            .child(Button::new(("calls-filter",index)).outline().w_full().h(px(36.)).accessibility_label(self.t(key))
                .child(div().flex().w_full().justify_between().child(label).child("⌄"))
                .dropdown_menu(move|mut menu,_,_| {
                    for(id,label)in &options {let id=id.clone();let entity=entity.clone();menu=menu.item(PopupMenuItem::new(label.clone()).checked(id==current).on_click(move|_,_,cx| {
                        let _=entity.update(cx,|this,cx|{if this.dashboard.client!=client{return}
                            let filters=&mut this.dashboard.calls.queries[client].filters;
                            match index {0=>filters.model=id.clone(),1=>filters.reasoning_effort=id.clone(),2=>filters.project=id.clone(),3=>filters.thread=id.clone(),_=>filters.root=id.clone()}
                            this.load_calls(false,cx);
                        });
                    }));}menu
                })).into_any_element()
    }

    /// 表头只发送全局排序意图，新列先降序，同列降序/升序循环。
    fn call_sort_head(&self,widths:&[f32;13],key:&str,field:Field,index:usize,cx:&Context<Self>)->TableHead {
        let query=&self.dashboard.calls.queries[self.dashboard.client];
        let active=query.sort_field==field;let ascending=active&&query.sort_direction==Direction::Asc;
        let arrow=if active {if ascending {"↑"}else{"↓"}}else{"↕"};
        let column=match index {0=>1,1=>4,2=>5,3=>6,4=>7,5=>8,6=>10,7=>11,_=>12};
        self.call_head(widths,column).child(Button::new(("calls-sort",index)).ghost().w_full().min_w(px(80.)).px_0().h_auto()
            .accessibility_label(format!("{} {}",self.t(key),arrow))
            .child(div().w_full().flex().items_center().gap_2().font_weight(FontWeight::BOLD)
                .when(index>=3,|row|row.justify_end()).when(index<3,|row|row.justify_between()).child(self.t(key)).child(arrow))
            .on_click(cx.listener(move|this,_,_,cx|{
                let query=&mut this.dashboard.calls.queries[this.dashboard.client];
                query.sort_direction=if query.sort_field==field&&query.sort_direction==Direction::Desc {Direction::Asc}else{Direction::Desc};
                query.sort_field=field;this.load_calls(false,cx);
            })))
    }

    /// 显式宽度覆盖 Kit 的等分 flex-basis，保持所有行的列边界一致。
    fn call_head(&self,widths:&[f32;13],column:usize)->TableHead {
        TableHead::new().w(px(widths[column])).min_w(px(widths[column])).flex_none().px(px(10.)).py_4()
    }

    /// 固定列顺序与游标 footer；替换中保留控件和表头，隐藏旧结果行。
    fn calls_table(&self,page:&UsageCallsPage,window:&gpui_kit::Window,cx:&Context<Self>)->AnyElement {
        let all=self.dashboard.client==0;let refreshing=self.dashboard.loading;let failed=self.dashboard.calls.replacement_failed;
        let widths=self.call_widths(page,window,cx);
        let mut header=TableRow::new();
        if all {header=header.child(self.call_head(&widths,0).child("Agent"));}
        header=header.child(self.call_sort_head(&widths,"calls.time",Field::OccurredAt,0,cx))
            .child(self.call_head(&widths,2).child(self.t("statistics.project")))
            .child(self.call_head(&widths,3).child(self.t("statistics.thread")))
            .child(self.call_sort_head(&widths,"statistics.model",Field::Model,1,cx))
            .child(self.call_sort_head(&widths,"statistics.reasoning",Field::ReasoningEffort,2,cx));
        for(index,key,field)in [(3,"calls.input",Field::InputTokens),(4,"statistics.total",Field::TotalTokens),
            (5,"statistics.cached",Field::CachedInputTokens),(6,"statistics.uncached",Field::UncachedInputTokens),
            (7,"statistics.output",Field::OutputTokens),(8,"statistics.reasoning_output",Field::ReasoningOutputTokens)] {
            if index==6 {header=header.child(self.call_head(&widths,9).text_right().child(self.t("statistics.cache_write")));}
            header=header.child(self.call_sort_head(&widths,key,field,index,cx));
        }
        let mut body=TableBody::new();
        if refreshing||failed {body=body.child(BrandTableRow::new("calls-updating-row",cx.theme().mode.is_dark()).child(TableCell::new().col_span(if all {13}else{12}).px(px(10.)).py_4()
            .text_color(if failed {cx.theme().danger}else{cx.theme().muted_foreground}).child(self.t(if failed {"calls.first_failed"}else{"calls.updating"}))));}
        else {body=body.children(page.items.iter().enumerate().map(|(index,item)|self.call_row(item,index,all,&widths,cx)));}
        let table=Table::new().w_full().text_size(px(14.)).font_weight(FontWeight::NORMAL).text_color(cx.theme().foreground)
            .child(TableHeader::new().font_weight(FontWeight::BOLD).child(header)).child(body);
        let count=if refreshing {self.t("calls.reading_first")}else if failed {self.t("calls.first_failed")}else{
            self.t("calls.loaded").replace("{loaded}",&format::exact(page.items.len() as u64)).replace("{total}",&format::exact(page.total_count))};
        let footer=div().p_4().border_t_1().border_color(cx.theme().border).flex().items_center().justify_between().text_sm().text_color(cx.theme().muted_foreground)
            .child(div().id("calls-count").test_support().role(gpui_kit::Role::Label).aria_label(count.clone()).child(count))
            .when(refreshing||failed,|row|row.child(self.t(if failed {"calls.retry_above"}else{"calls.controls_available"})))
            .when(!refreshing&&!failed,|row|{if page.next_cursor.is_some() {
                row.child(Button::new("calls-load-more").outline().label(self.t("calls.load_more")).loading(self.dashboard.calls.loading_next).disabled(self.dashboard.calls.loading_next)
                    .on_click(cx.listener(|this,_,_,cx|this.load_calls(true,cx))))
            }else{row.child(self.t("calls.all_loaded"))}});
        StaticCard::new("calls-results", cx.theme().mode.is_dark(), StaticMaterial::Table).test_support().p_0().gap_0().child(div().id("calls-table-scroll").tab_index(0).role(gpui_kit::Role::Group).aria_label(self.t("calls.table"))
            .overflow_x_scroll().w_full().min_w_0().child(div().w(px(widths[if all {0}else{1}..].iter().sum())).flex_none().child(table)))
            .when(!refreshing&&!failed&&page.items.is_empty(),|panel|panel.child(div().p_5().text_color(cx.theme().muted_foreground)
                .child(self.t(if page.index_state==LocalIndexState::ReadyNoCalls {"calls.empty_scanned"}else{"calls.empty_filtered"}))))
            .child(footer).into_any_element()
    }

    /// 页面替换失败隐藏旧值；后页失败保留当前已加载链。
    fn calls_failure(&self,next:bool,cx:&Context<Self>)->AnyElement {
        div().p_4().rounded_lg().bg(cx.theme().danger.opacity(0.1)).text_color(cx.theme().danger).flex().items_center().justify_between()
            .child(div().flex().flex_col().gap_2().child(div().font_weight(FontWeight::BOLD).child(self.t(if next {"calls.next_failed"}else{"calls.refresh_failed"})))
                .child(self.t(if next {"calls.next_retained"}else{"calls.refresh_retained"})))
            .child(Button::new("calls-retry").outline().label(self.t(if next {"calls.retry_next"}else{"calls.retry_refresh"}))
                .on_click(cx.listener(move|this,_,_,cx|this.load_calls(next,cx))))
            .into_any_element()
    }

    /// 每条事实的标签与 Token 均按 core 口径展示；Grok 推理保持缺失。
    fn call_row(&self,item:&UsageCallItem,index:usize,all:bool,widths:&[f32;13],cx:&Context<Self>)->BrandTableRow {
        let text_cell=|field:usize,value:String| {
            let title=value.clone();
            TableCell::new().w(px(widths[field])).min_w(px(widths[field])).flex_none().px(px(10.)).py_4().child(div()
                .id(SharedString::from(format!("call-{index}-{field}"))).test_support().w_full().min_w_0().truncate().role(gpui_kit::Role::Label).aria_label(value.clone())
                .tooltip(move|window,cx|gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window,cx)).child(value))
        };
        let mut row=BrandTableRow::new(SharedString::from(format!("calls-row-{:?}-{}",item.client,item.id)),cx.theme().mode.is_dark());
        if all {row=row.child(text_cell(0,match item.client {SourceClientKind::Codex=>"Codex",SourceClientKind::ClaudeCode=>"Claude Code",SourceClientKind::GrokBuildCli=>"Grok",SourceClientKind::WorkBuddy=>"WorkBuddy"}.into()));}
        row=row.child(text_cell(1,format::observed(item.occurred_at_epoch_ms,self.locale)))
            .child(text_cell(2,self.display_label(&item.project_label,item.project_label_code)))
            .child(text_cell(3,self.display_label(&item.thread_label,item.thread_label_code)))
            .child(text_cell(4,self.display_label(&item.model_label,item.model_label_code)))
            .child(text_cell(5,if item.client==SourceClientKind::GrokBuildCli {self.t("dashboard.not_provided")}else{self.display_label(&item.reasoning_effort_label,item.reasoning_effort_label_code)}));
        let tokens=&item.fact.value;
        for (offset,value) in [Some(tokens.input_tokens),Some(tokens.total_tokens),tokens.cached_input_tokens,tokens.cache_write_input_tokens,
            item.uncached_input_tokens,Some(tokens.output_tokens),tokens.reasoning_output_tokens].into_iter().enumerate() {
            let column=offset+6;
            let exact=value.map(format::exact).unwrap_or_else(||self.t("dashboard.not_provided"));
            let content=div().id(SharedString::from(format!("call-{index}-{column}"))).test_support().role(gpui_kit::Role::Label).aria_label(exact.clone())
                .flex().flex_col().items_end().text_right().font_family("Geist Mono")
                .when_some(value,|cell,value|cell.child(div().text_size(px(16.)).line_height(gpui_kit::relative(1.55)).font_weight(FontWeight::BOLD).child(format::compact(value)))
                    .child(div().text_xs().line_height(gpui_kit::relative(1.4)).text_color(cx.theme().muted_foreground).child(exact.clone())))
                .when(value.is_none(),|cell|cell.font_family(cx.theme().font_family.clone()).text_color(cx.theme().muted_foreground).child(exact));
            row=row.child(TableCell::new().w(px(widths[column])).min_w(px(widths[column])).flex_none().px(px(10.)).py_4().text_right().child(content));
        }
        row
    }
}
