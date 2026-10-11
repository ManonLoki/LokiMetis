//! 数据源页只保存展示快照与草稿；注册表及索引写入属于应用服务。

use gpui_kit::base::TestSupportExt as _;
use crate::app::surface::{StaticCard, StaticMaterial};
mod actions;
mod dialog;
mod table;
mod discovery;
mod button;

use button::{SourceButton, SourceButtonVariant};

use super::Shell;
use crate::usage_service::{UsageQuery,UsageResponse};
use gpui_kit::{AnyElement,Context,Task,div,prelude::*,px,FontWeight,
    component::{ActiveTheme,Disableable,Sizable,Size,button::Button,menu::{DropdownMenu,PopupMenuItem}}};
use loki_metris_core::SourceRootSummary;

/// 局部页面状态随卸载丢弃；服务持有已接受的写任务。
#[derive(Default)]
pub(in crate::app) struct SourcesState {
    pub roots:Option<Vec<SourceRootSummary>>,
    pub refresh_failed:bool,
    pub pending:bool,
    pub feedback:Option<Result<String,&'static str>>,
    pub operation_task:Option<Task<()>>,
    pub candidate_tasks:std::collections::BTreeMap<String,Task<()>>,
    pub post_update_error:Option<&'static str>,
    pub modal:Option<dialog::SourceDialog>,
    pub generation:u64,
    pub discovery:crate::usage_service::DiscoveryView,
    pub workbuddy_discovery:Option<loki_metris_core::RootDiscoveryStatus>,
}
impl SourcesState {
    /// 代数使旧页面回执在离页后重进同 Agent 时仍不可覆盖新快照。
    pub(in crate::app) fn reset(&mut self) {
        let generation=self.generation.wrapping_add(1);*self=Self{generation,..Default::default()};
    }
}

impl Shell {
    /// 查询只读已有根登记；刷新失败保留旧行并冻结变更控件。
    pub(in crate::app) fn load_sources(&mut self,cx:&mut Context<Self>) {
        self.dashboard.revision=self.dashboard.revision.wrapping_add(1);
        self.dashboard.task.take();self.dashboard.error=None;self.dashboard.loading=false;
        let Some(client)=self.usage_client() else{return};
        let Some(service)=&self.dashboard.service else {cx.notify();return};
        let mut receiver=match service.query(UsageQuery::Sources(client)) {
            Ok(receiver)=>receiver,Err(error)=>{self.sources_query_failed(error);cx.notify();return}
        };
        let revision=self.dashboard.revision;let generation=self.dashboard.sources.generation;
        self.dashboard.loading=true;
        self.dashboard.task=Some(cx.spawn(async move|entity,cx| {
            let result=loop {match receiver.try_recv() {
                Ok(result)=>break result,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed)=>break Err("usage_query_failed"),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)=>{},
            }cx.background_executor().timer(std::time::Duration::from_millis(50)).await;};
            let _=entity.update(cx,|this,cx| {
                if this.dashboard.revision!=revision || this.dashboard.sources.generation!=generation || this.usage_client()!=Some(client) {return}
                this.dashboard.loading=false;
                match result {
                    Ok(UsageResponse::Sources(roots))=>{
                        if let Some(status)=&mut this.dashboard.sources.workbuddy_discovery {
                            status.progress.directories_checked=roots.len() as u64;status.progress.file_names_checked=roots.iter().map(|root|root.file_count).sum();
                        }
                        this.dashboard.sources.roots=Some(roots);this.dashboard.sources.refresh_failed=false;
                        // 只有权威来源查询成功才允许恢复当前客户端的候选登记。
                        if let Some(service)=&this.dashboard.service {
                            if let Err(error)=service.resume_discovery(client) {this.dashboard.sources.feedback=Some(Err(error));}
                        }
                    },
                    Err(error)=>this.sources_query_failed(error),_=>this.sources_query_failed("usage_query_failed"),
                }cx.notify();
            });
        }));cx.notify();
    }

    /// 首次查询错误归属全页，已有快照错误归属局部刷新。
    fn sources_query_failed(&mut self,error:&'static str) {
        self.dashboard.loading=false;
        if self.dashboard.sources.roots.is_some() {self.dashboard.sources.refresh_failed=true;}
        else {self.dashboard.error=Some(error);}
    }

    /// 忙态来自接受回执或当前 writer；普通只读刷新不阻断写入。
    fn source_writes_disabled(&self)->bool {
        self.source_client_scanning() || self.dashboard.sources.pending || self.dashboard.sources.refresh_failed || self.dashboard.sources.roots.is_none()
            || self.usage_client().is_some_and(|client|self.dashboard.sources.discovery.indexing.contains(&client))
    }

    /// 扫描 busy 标签属于实际持锁的客户端，未知归属时保守禁用。
    fn source_client_scanning(&self)->bool {
        self.dashboard.scanning&&self.dashboard.scanning_client.is_none_or(|client|Some(client)==self.usage_client())
    }

    /// 所有提示中的客户端身份来自页面选择，不读取目录路径。
    fn sources_client_name(&self)->&'static str {
        match self.dashboard.client {2=>"Claude Code",3=>"Grok",4=>"WorkBuddy",_=>"Codex"}
    }

    /// 三张固定布局卡片直接投影登记列表；WorkBuddy 仅有固定目录重读。
    pub(super) fn usage_sources(&self,cx:&mut Context<Self>)->AnyElement {
        if self.dashboard.loading && self.dashboard.sources.roots.is_none() {return self.usage_loading(cx)}
        let mut page=div().w_full().min_w_0().px_2().flex().flex_col().gap_5();
        if self.dashboard.sources.refresh_failed {page=page.child(self.source_notice(true,self.t("sources.page.refreshError"),cx)
            .child(SourceButton::new("sources-retry",SourceButtonVariant::Default,cx.theme().mode.is_dark()).with_size(Size::XSmall).h(px(30.)).px(px(14.)).rounded(px(12.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).label(self.t("dashboard.retry")).on_click(cx.listener(|this,_,_,cx|this.load_sources(cx)))));}
        if let Some(feedback)=&self.dashboard.sources.feedback {page=page.child(match feedback {
            Ok(key)=>self.source_notice(false,self.t("sources.page.updatedTitle"),cx).child(self.t(key)),
            Err(_)=>self.source_notice(true,self.t("sources.page.updateErrorTitle"),cx)
                .child(self.t("sources.page.updateErrorBody").replace("{{client}}",self.sources_client_name())),
        });}
        if let Some(error)=self.dashboard.sources.post_update_error.or(self.dashboard.sources.discovery.error) {
            page=page.child(self.source_notice(true,self.t(if error=="usage_scan_cancelled" {"dashboard.cancelled"}else{"sources.page.updateErrorTitle"}),cx));
        }
        if self.source_client_scanning() {page=page.child(self.source_notice(false,self.t("sources.background.runningTitle"),cx)
            .child(self.t("sources.background.runningBody").replace("{{client}}",self.sources_client_name())));}
        page=page.child(self.source_table(cx));
        if self.dashboard.client==1 {page=page.child(self.source_primary(cx));}
        page=page.child(self.source_discovery(cx));
        page.into_any_element()
    }

    /// 提示区保留标题和正文层级，不把内部错误码作为产品文案。
    fn source_notice(&self,error:bool,title:String,cx:&Context<Self>)->gpui_kit::Div {
        div().w_full().p_4().rounded_lg().border_1().border_color(if error {cx.theme().danger}else{cx.theme().border})
            .flex().flex_col().gap_2().child(div().font_weight(FontWeight::BOLD).child(title))
    }

    /// 主目录选项只包含已启用登记根；清除沿用 core 原子变更。
    fn source_primary(&self,cx:&mut Context<Self>)->AnyElement {
        let roots=self.dashboard.sources.roots.as_deref().unwrap_or_default();
        let current=roots.iter().find(|root|root.is_primary);
        let selected=current.map(|root|root.id.clone());
        let options:Vec<_>=roots.iter().filter(|root|root.enabled).map(|root|(root.id.clone(),self.source_alias(root,roots))).collect();
        let client=self.usage_client().unwrap();let generation=self.dashboard.sources.generation;let weak=cx.entity().downgrade();
        let disabled=self.source_writes_disabled();
        StaticCard::new("sources-primary-panel", cx.theme().mode.is_dark(), StaticMaterial::Panel).test_support().gap_4().p_5().child(div().flex().items_center().gap_3()
            .child(div().text_size(px(19.2)).font_weight(FontWeight::SEMIBOLD).child(self.t("sources.primary.title")))
            .child(self.quality_badge(self.t("sources.primary.badge"),cx.theme().primary)))
            .child(div().w_full().flex().flex_col().gap_4()
                .child(div().w_full().flex().flex_col().gap_1().child(self.t("sources.primary.label"))
                    .child(Button::new("sources-primary").outline().w_full().h(px(36.)).rounded(px(12.)).disabled(disabled||options.is_empty())
                        .accessibility_label(self.t("sources.primary.label"))
                        .child(div().w_full().flex().justify_between().child(current.map(|root|self.source_alias(root,roots)).unwrap_or_else(||self.t("sources.primary.placeholder"))).child("⌄"))
                        .dropdown_menu(move|mut menu,_,_| {
                            for(id,label)in &options {let id=id.clone();let weak=weak.clone();menu=menu.item(PopupMenuItem::new(label.clone()).checked(selected.as_ref()==Some(&id)).on_click(move|_,_,cx| {
                                let _=weak.update(cx,|this,cx| {if this.dashboard.sources.generation==generation&&this.usage_client()==Some(client) {
                                    this.change_source(crate::usage_service::SourceOperation::SetPrimary{root_id:Some(id.clone())},cx);
                                }});
                            }));}menu
                        })))
                .child(div().w_full().flex().justify_end().child(SourceButton::new("sources-primary-clear",SourceButtonVariant::SubtleBrand,cx.theme().mode.is_dark()).h(px(26.)).px_2().rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled||current.is_none()).label(self.t("sources.primary.clear"))
                    .on_click(cx.listener(|this,_,_,cx|this.change_source(crate::usage_service::SourceOperation::SetPrimary{root_id:None},cx))))))
            .into_any_element()
    }

    /// 同名根附加稳定短 ID，使选择与操作在脱敏后仍可区分。
    fn source_alias(&self,root:&SourceRootSummary,roots:&[SourceRootSummary])->String {
        if roots.iter().filter(|item|item.alias==root.alias).count()>1 {format!("{} · {}",root.alias,root.id.chars().rev().take(8).collect::<String>().chars().rev().collect::<String>())}
        else{root.alias.clone()}
    }
}
