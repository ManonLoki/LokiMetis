//! 来源发现控件投影独立状态机；手动选择器等待任务不随页面卸载丢弃。

use crate::app::surface::{StaticCard, StaticMaterial};
use super::{Shell,SourceButton,SourceButtonVariant};
use crate::usage_service::ManualAddResult;
use gpui_kit::{AnyElement,Context,FontWeight,SharedString,div,prelude::*,px,
    base::TestSupportExt as _,component::{ActiveTheme,Disableable}};
use loki_metris_core::{RootDiscoveryLifecycle as State,RootDiscoveryScope as Scope,RootDiscoveryStatus};

impl Shell {
    /// 候选重试等待真实登记回执，页面卸载只丢弃提示，不取消服务写任务。
    fn retry_source_candidate(&mut self,id:String,cx:&mut Context<Self>) {
        let Some(service)=&self.dashboard.service else{return};
        let receiver=match service.retry_candidate(id.clone()) {Ok(receiver)=>receiver,Err(error)=>{self.dashboard.sources.feedback=Some(Err(error));cx.notify();return}};
        let generation=self.dashboard.sources.generation;let client=self.usage_client();
        self.dashboard.sources.feedback=None;self.dashboard.sources.post_update_error=None;
        let task=cx.spawn(async move|entity,cx| {
            let result=crate::usage_service::poll_receipt(receiver,cx.background_executor().clone(),std::time::Duration::from_millis(50)).await.unwrap_or(Err("source_update_failed"));
            let _=entity.update(cx,|this,cx| {
                if this.dashboard.sources.generation!=generation||this.usage_client()!=client {return}
                this.dashboard.sources.feedback=Some(result.map(|()|"sources.messages.sourceRegistered".into()));
                this.load_sources(cx);cx.notify();
            });
        });self.dashboard.sources.candidate_tasks.insert(id,task);cx.notify();
    }

    /// 启动动作捕获当前客户端；失败提示仍由当前页面拥有。
    fn start_source_discovery(&mut self,scope:Scope,cx:&mut Context<Self>) {
        self.dashboard.sources.feedback=None;
        if self.dashboard.client==4 {
            let mut status=RootDiscoveryStatus{lifecycle:State::Complete,scope,..Default::default()};
            status.progress.volumes_completed=1;status.progress.volumes_total=1;
            self.dashboard.sources.workbuddy_discovery=Some(status);self.load_sources(cx);return
        }
        if let (Some(service),Some(client))=(&self.dashboard.service,self.usage_client()) {
            if let Err(error)=service.start_discovery(client,scope) {self.dashboard.sources.feedback=Some(Err(error));}
            self.dashboard.sources.discovery=service.discovery_view();
        }cx.notify();
    }

    /// 预约先于原生选择器；用户取消没有错误、写入或索引副作用。
    fn pick_source_directory(&mut self,cx:&mut Context<Self>) {
        if self.dashboard.manual_pending {return}
        let (Some(service),Some(client))=(self.dashboard.service.clone(),self.usage_client()) else{return};
        let lease=match service.begin_manual_add(client) {Ok(lease)=>lease,Err(error)=>{self.dashboard.sources.feedback=Some(Err(error));cx.notify();return}};
        let generation=self.dashboard.sources.generation;
        self.dashboard.manual_pending=true;self.dashboard.sources.feedback=None;
        let selected=cx.prompt_for_paths(gpui_kit::PathPromptOptions{files:false,directories:true,multiple:false,prompt:None});
        self.dashboard.manual_task=Some(cx.spawn(async move|entity,cx| {
            let result=match selected.await {
                Ok(Ok(paths))=>{
                    let path=paths.and_then(|paths|paths.into_iter().next());
                    match service.accept_manual_directory(client,lease,path) {
                        Ok(receiver)=>crate::usage_service::poll_receipt(receiver,cx.background_executor().clone(),std::time::Duration::from_millis(50)).await.unwrap_or(Err("source_update_failed")),
                        Err(error)=>Err(error),
                    }
                },
                _=>Err("source_directory_unavailable"),
            };
            let _=entity.update(cx,|this,cx| {
                this.dashboard.manual_pending=false;
                if this.dashboard.sources.generation!=generation||this.usage_client()!=Some(client) {cx.notify();return}
                this.dashboard.sources.feedback=match result {
                    Ok(ManualAddResult::Cancelled)=>None,
                    Ok(ManualAddResult::Registered{outcome,refresh})=>{
                        this.dashboard.sources.post_update_error=refresh.err();
                        Some(Ok(format!("sources.messages.{}",if outcome.changed {"sourceRegistered"}else{"sourceAlreadyRegistered"})))
                    },
                    Ok(ManualAddResult::DeepSearchStarted)=>Some(Ok("sources.messages.sourceManualDeepSearchStarted".into())),
                    Err(error)=>Some(Err(error)),
                };
                this.load_sources(cx);cx.notify();
            });
        }));cx.notify();
    }

    /// 来源发现只呈现结构化计数，绝不显示虚构百分比。
    pub(super) fn source_discovery(&self,cx:&mut Context<Self>)->AnyElement {
        let view=&self.dashboard.sources.discovery;
        let fallback=RootDiscoveryStatus::default();
        let status=if self.dashboard.client==4 {self.dashboard.sources.workbuddy_discovery.as_ref().unwrap_or(&fallback)}else{&view.status};
        let state=match status.lifecycle {State::Idle=>"idle",State::Running=>"running",State::Complete=>"complete",State::Partial=>"partial",State::Cancelled=>"cancelled",State::Failed=>"failed"};
        let scope=match status.scope {Scope::UserPriority=>"userPriority",Scope::FullLocalVolumes=>"fullLocalVolumes",Scope::ManualSubtree=>"manualSubtree"};
        let running=status.lifecycle==State::Running;
        let disabled=self.source_writes_disabled();let start_disabled=disabled||running||self.dashboard.manual_pending;
        let state_label=self.t(&format!("sources.discovery.state.{state}"));
        let mut card=StaticCard::new("sources-discovery-panel", cx.theme().mode.is_dark(), StaticMaterial::Panel).test_support().gap_4().p_5().child(div().w_full().flex().flex_wrap().items_center().justify_between().gap_4()
            .child(div().flex().flex_col().gap_1().child(div().flex().items_center().gap_3()
                .child(div().text_size(px(19.2)).font_weight(FontWeight::SEMIBOLD).child(self.t("sources.discovery.title")))
                .child(div().id("sources-discovery-state").test_support().role(gpui_kit::Role::Label).aria_label(state_label.clone())
                    .child(self.quality_badge(state_label.clone(),if status.lifecycle==State::Partial {gpui_kit::rgb(0xf08c00).into()}else{cx.theme().muted_foreground}))))
                .child(div().text_color(cx.theme().muted_foreground).text_size(px(12.)).child(self.t(&format!("sources.discovery.scope.{scope}")))))
            .child(div().flex().flex_wrap().gap_4()
                .child(SourceButton::new("sources-discover-quick",SourceButtonVariant::FilledBrand,cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(start_disabled).label(self.t("sources.discovery.startUser"))
                    .on_click(cx.listener(|this,_,_,cx|this.start_source_discovery(Scope::UserPriority,cx))))
                .child(SourceButton::new("sources-discover-full",SourceButtonVariant::LightBrand,cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(start_disabled).label(self.t("sources.discovery.startFull"))
                    .on_click(cx.listener(|this,_,_,cx|this.start_source_discovery(Scope::FullLocalVolumes,cx))))
                .when(self.dashboard.client!=4,|row|row.child(SourceButton::new("sources-add",SourceButtonVariant::Default,cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(start_disabled)
                    .loading(self.dashboard.manual_pending).label(self.t("sources.page.addRoot"))
                    .on_click(cx.listener(|this,_,_,cx|this.pick_source_directory(cx)))))
                .child(SourceButton::new("sources-discovery-cancel",SourceButtonVariant::LightRed,cx.theme().mode.is_dark()).h(px(36.)).px(px(18.)).rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled||!running||self.dashboard.client==4)
                    .label(self.t("sources.discovery.cancel"))
                    .on_click(cx.listener(|this,_,_,cx| {
                        if let Some(service)=&this.dashboard.service
                            && let Err(error)=service.cancel_discovery(){this.dashboard.sources.feedback=Some(Err(error));}cx.notify();
                    })))));
        let candidates:Vec<_>=view.candidates.iter().filter(|candidate|Some(candidate.client)==self.usage_client()&&view.failed_ids.contains(&candidate.id)).collect();
        let progress=status.progress;
        let counts=[self.t("sources.discovery.volumes").replace("{{completed}}",&progress.volumes_completed.to_string()).replace("{{total}}",&progress.volumes_total.to_string()),
            self.t("sources.discovery.directories").replace("{{count}}",&progress.directories_checked.to_string()),
            self.t("sources.discovery.fileNames").replace("{{count}}",&progress.file_names_checked.to_string()),
            self.t("sources.discovery.candidates").replace("{{count}}",&candidates.len().to_string())];
        card=card.child(div().flex().flex_wrap().gap_8().children(counts.into_iter().enumerate().map(|(index,label)| {
            div().id(("sources-discovery-count",index)).test_support().role(gpui_kit::Role::Label).aria_label(label.clone()).child(label)
        })));
        if status.lifecycle==State::Failed {card=card.child(self.source_notice(true,self.t("sources.discovery.failedTitle"),cx)
            .child(self.t("sources.discovery.failedBody").replace("{{code}}",status.error_code.as_deref().unwrap_or("source_discovery_failed"))));}
        else if status.lifecycle==State::Partial {card=card.child(self.source_notice(false,self.t("sources.discovery.partialTitle"),cx));}
        for candidate in candidates {
            let id=candidate.id.clone();
            card=card.child(div().w_full().flex().items_center().justify_between().gap_3().border_t_1().border_color(cx.theme().border).pt_3()
                .child(div().flex_1().min_w_0().child(candidate.absolute_path.clone()))
                .child(SourceButton::new(SharedString::from(format!("candidate-add-{id}")),SourceButtonVariant::FilledBrand,cx.theme().mode.is_dark()).h(px(26.)).px_2().rounded(px(28.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled||view.retrying_ids.contains(&id))
                    .loading(view.retrying_ids.contains(&id)).label(self.t("sources.discovery.add"))
                    .on_click(cx.listener(move|this,_,_,cx|this.retry_source_candidate(id.clone(),cx)))));
        }
        card.into_any_element()
    }
}
