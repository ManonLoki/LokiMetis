//! 每个显式操作保存自身回执，查询刷新不会吞掉已发生的变更结果。

use super::Shell;
use crate::usage_service::{SourceOperation,SourceOperationResult};
use gpui_kit::Context;
use loki_metris_core::SourceRootMutationKind as Kind;

impl Shell {
    /// 服务立即检查共享写锁；被接受的操作由服务拥有，页面仅等待显示回执。
    pub(super) fn change_source(&mut self,operation:SourceOperation,cx:&mut Context<Self>) {
        let Some(client)=self.usage_client() else{return};
        self.dashboard.sources.feedback=None;
        let Some(service)=&self.dashboard.service else{return};
        let receiver=match service.mutate_source(client,operation) {
            Ok(receiver)=>receiver,Err(error)=>{self.dashboard.sources.feedback=Some(Err(error));cx.notify();return}
        };
        let generation=self.dashboard.sources.generation;
        self.dashboard.sources.pending=true;
        self.dashboard.sources.operation_task=Some(cx.spawn(async move|entity,cx| {
            let result=crate::usage_service::poll_receipt(receiver,cx.background_executor().clone(),std::time::Duration::from_millis(50)).await.unwrap_or(Err("source_update_failed"));
            let _=entity.update(cx,|this,cx| {
                if this.dashboard.sources.generation!=generation || this.usage_client()!=Some(client) {return}
                this.dashboard.sources.pending=false;
                this.dashboard.sources.feedback=Some(result.map(|result|match result {
                    SourceOperationResult::Reindexed=>"sources.messages.reindexed".into(),
                    SourceOperationResult::Mutation(outcome)=>format!("sources.messages.{}",match outcome.kind {
                        Kind::SourceRootRegistered=>"sourceRegistered",Kind::SourceRootAlreadyRegistered=>"sourceAlreadyRegistered",
                        Kind::SourceRootEnabled=>"sourceEnabled",Kind::SourceRootDisabled=>"sourceDisabled",
                        Kind::SourceRootRenamed=>"sourceRenamed",Kind::SourceRootRemoved=>"sourceRemoved",
                        Kind::PrimarySourceRootChanged=>"primaryChanged",Kind::PrimarySourceRootAlreadySelected=>"primaryAlreadySelected",
                        Kind::PrimarySourceRootCleared=>"primaryCleared",Kind::PrimarySourceRootNotSet=>"primaryNotSet",
                    }),
                }));
                this.load_sources(cx);cx.notify();
            });
        }));cx.notify();
    }
}
