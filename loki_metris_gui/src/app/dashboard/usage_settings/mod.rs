//! 全局用量设置页使用独立查询与逐字段真实保存，不依赖索引繁忙状态。

mod state;
mod view;
pub(in crate::app) use state::UsageSettingsState;
use state::{Feedback,field,value,parsed};
use super::{Shell,Route};
use gpui_kit::{prelude::*,Context,Window,component::input::{InputState,InputEvent}};

impl Shell {
    /// 输入实体只创建一次；set_value 只用于已变化的权威基线，避免覆盖 IME 和草稿。
    pub(in crate::app) fn sync_usage_setting_inputs(&mut self,window:&mut Window,cx:&mut Context<Self>) {
        if self.page!=crate::app::Page::Dashboard||self.dashboard.route!=Route::Settings {return}
        if self.dashboard.usage_settings.inputs.is_none() {
            let inputs=std::array::from_fn(|_|cx.new(|cx|InputState::new(window,cx).validate(|text,_|text.bytes().all(|b|b.is_ascii_digit()))));
            for (index,input) in inputs.iter().enumerate() {
                let subscription=cx.subscribe_in(input,window,move|this,_,event,_,cx| {
                    match event {
                        InputEvent::Change=>{this.dashboard.usage_settings.feedback[index]=None;cx.notify();},
                        InputEvent::PressEnter{..}=>this.save_usage_setting(index,cx),
                        _=>{},
                    }
                });self.dashboard.usage_settings.subscriptions.push(subscription);
            }
            self.dashboard.usage_settings.inputs=Some(inputs);
        }
        let state=&mut self.dashboard.usage_settings;
        if let Some(inputs)=&state.inputs {for (index,input) in inputs.iter().enumerate() {
            if let Some(value)=state.reset_inputs[index].take() {input.update(cx,|input,cx|input.set_value(value,window,cx));}
        }}
    }

    /// 查询失败显示整页重试，旧快照不被伪装成当前可编辑设置。
    pub(super) fn load_usage_settings(&mut self,cx:&mut Context<Self>) {
        let state=&mut self.dashboard.usage_settings;state.query_revision=state.query_revision.wrapping_add(1);state.query_task.take();
        state.loading=true;state.failed=false;
        let Some(service)=&self.dashboard.service else {state.loading=false;cx.notify();return};
        let service_revision=service.usage_settings_revision();let generation=state.generation;let revision=state.query_revision;
        let work=match service.usage_settings() {Ok(work)=>work,Err(_)=>{state.loading=false;state.failed=true;cx.notify();return}};
        state.query_task=Some(cx.spawn(async move|entity,cx| {
            let result=crate::receipt::poll_receipt(work,cx.background_executor().clone(),std::time::Duration::from_millis(30)).await.unwrap_or(Err("usage_settings_unavailable"));
            let _=entity.update(cx,|this,cx| {
                let state=&mut this.dashboard.usage_settings;
                if state.generation!=generation||state.query_revision!=revision {return}
                state.loading=false;state.service_revision=service_revision;
                match result {
                    Ok(settings)=>state.accept_snapshot(settings),Err(_)=>state.failed=true,
                }cx.notify();
            });
        }));cx.notify();
    }

    /// 保存捕获本次合法草稿；两个回执只更新各自字段，避免响应顺序覆盖另一张卡。
    fn save_usage_setting(&mut self,index:usize,cx:&mut Context<Self>) {
        let state=&mut self.dashboard.usage_settings;
        if state.loading||state.failed||state.pending[index] {return}
        let Some(saved)=state.saved else{return};let Some(inputs)=&state.inputs else{return};
        let Some(input)=parsed(&inputs[index].read(cx).value(),index) else{return};
        if input==value(saved,index) {return}
        let Some(service)=&self.dashboard.service else{return};
        let work=match service.save_usage_setting(field(index),input) {Ok(work)=>work,Err(_)=>{state.feedback[index]=Some(Feedback::Failed);cx.notify();return}};
        state.query_task.take();state.query_revision=state.query_revision.wrapping_add(1);
        state.pending[index]=true;state.feedback[index]=None;let generation=state.generation;
        state.save_tasks[index]=Some(cx.spawn(async move|entity,cx| {
            let result=crate::receipt::poll_receipt(work,cx.background_executor().clone(),std::time::Duration::from_millis(30)).await.unwrap_or(Err("usage_setting_save_failed"));
            let _=entity.update(cx,|this,cx| {
                let state=&mut this.dashboard.usage_settings;if state.generation!=generation {return}
                state.pending[index]=false;
                match result {
                    Ok(settings)=>state.accept_save(index,settings),Err(_)=>state.feedback[index]=Some(Feedback::Failed),
                }cx.notify();
            });
        }));cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::state::{parsed,UsageSettingsState};
    use loki_metris_core::{UsageSettings,UsageSettingField};
    /// 后台同值刷新不能覆盖尚未提交的输入，变化基线仅重置对应卡。
    #[test]
    fn refresh_keeps_same_baseline_draft_and_resets_only_changed_field() {
        let mut state=UsageSettingsState::default();state.accept_snapshot(UsageSettings::default());state.reset_inputs=[None,None];
        state.accept_snapshot(UsageSettings::default());assert_eq!(state.reset_inputs,[None,None]);
        state.accept_snapshot(UsageSettings::default().updated(UsageSettingField::RetentionDays,120).unwrap());
        assert_eq!(state.reset_inputs,[None,Some("120".into())]);
    }
    /// 回执乱序到达不能把另一张卡较新的值回退到旧完整快照。
    #[test]
    fn save_receipts_merge_the_submitted_field_only() {
        let mut state=UsageSettingsState{saved:Some(UsageSettings::default()),..Default::default()};
        let earlier=UsageSettings::default().updated(UsageSettingField::ScanInterval,7).unwrap();
        let later=earlier.updated(UsageSettingField::RetentionDays,120).unwrap();
        state.accept_save(1,later);state.accept_save(0,earlier);
        assert_eq!(state.saved,Some(later));
    }
    /// 非法草稿保持可见但不能提交；两字段各自使用 core 的真实边界。
    #[test]
    fn draft_validation_uses_distinct_core_bounds() {
        for text in ["","0","-1","1.5","1441","65536"] {assert_eq!(parsed(text,0),None);}
        assert_eq!(parsed("1440",0),Some(1440));assert_eq!(parsed("3650",1),Some(3650));assert_eq!(parsed("3651",1),None);
    }
}
