//! 两张卡的草稿、回执与页面代数仅属于展示层，权威设置由服务提供。

use gpui_kit::{Entity,Subscription,Task,component::input::InputState};
use loki_metris_core::{UsageSettings,UsageSettingField};

/// 成功反馈携带该次实际保存值；失败不泄漏文件路径。
#[derive(Clone,Copy)]
pub(crate) enum Feedback {
    /// 保存成功，附实际保存的值。
    Saved(u16),
    /// 保存失败。
    Failed,
}

/// 输入实体跨渲染稳定；离页或切物理客户端只重置页面草稿。
pub(in crate::app) struct UsageSettingsState {
    /// 两个输入框：下标 0 为扫描间隔，1 为保留天数；首次进入页面时创建。
    pub inputs:Option<[Entity<InputState>;2]>,
    /// 输入变化的订阅。
    pub subscriptions:Vec<Subscription>,
    /// 最近一次读取或保存成功的设置。
    pub saved:Option<UsageSettings>,
    /// 下一帧要写回输入框的值；渲染时取走。
    pub reset_inputs:[Option<String>;2],
    /// 各字段保存在途。
    pub pending:[bool;2],
    /// 各字段最近一次保存的反馈。
    pub feedback:[Option<Feedback>;2],
    /// 各字段的保存任务。
    pub save_tasks:[Option<Task<()>>;2],
    /// 读取任务。
    pub query_task:Option<Task<()>>,
    /// 读取在途。
    pub loading:bool,
    /// 最近一次读取失败。
    pub failed:bool,
    /// 页面代次；重置时递增，旧保存结果被丢弃。
    pub generation:u64,
    /// 读取代次；每次发起读取递增，旧读取结果被丢弃。
    pub query_revision:u64,
    /// 已读取到的服务端设置修订号。
    pub service_revision:u64,
    /// 页面对应的物理客户端下标（1..=3）；切换时重置草稿。
    pub physical_client:usize,
}
impl Default for UsageSettingsState {
    /// 物理客户端默认与首次看板 Codex 相同。
    fn default()->Self {Self{inputs:None,subscriptions:Vec::new(),saved:None,reset_inputs:Default::default(),pending:[false;2],feedback:[None;2],save_tasks:Default::default(),query_task:None,loading:false,failed:false,generation:0,query_revision:0,service_revision:0,physical_client:1}}
}
impl UsageSettingsState {
    /// 同基线刷新保留草稿；只有发生变化的字段重置输入及旧反馈。
    pub(super) fn accept_snapshot(&mut self,settings:UsageSettings) {
        for index in 0..2 {if self.saved.is_none_or(|old|value(old,index)!=value(settings,index)) {
            self.reset_inputs[index]=Some(value(settings,index).to_string());self.feedback[index]=None;
        }}self.saved=Some(settings);self.failed=false;
    }
    /// 完整回执只提交本卡字段，防止另一卡更晚提交的基线被旧回执回退。
    pub(super) fn accept_save(&mut self,index:usize,settings:UsageSettings) {
        let saved_value=value(settings,index);
        if let Some(saved)=self.saved {self.saved=saved.updated(field(index),saved_value).ok();}
        self.reset_inputs[index]=Some(saved_value.to_string());self.feedback[index]=Some(Feedback::Saved(saved_value));
    }
    /// 已接受的后台保存由应用 owner 持有；丢弃页面接收任务不撤销写入。
    pub(in crate::app) fn reset(&mut self) {
        self.generation=self.generation.wrapping_add(1);self.query_revision=self.query_revision.wrapping_add(1);
        self.query_task.take();self.save_tasks=[None,None];self.pending=[false;2];self.feedback=[None;2];
        self.saved=None;self.loading=false;self.failed=false;self.reset_inputs=[None,None];
    }
}

/// 展示字段序号稳定对应两张卡，领域变更仍提交明确字段身份。
pub(super) fn field(index:usize)->UsageSettingField {if index==0 {UsageSettingField::ScanInterval}else{UsageSettingField::RetentionDays}}
/// 只读取已校验领域快照，不在 GUI 中创造新的业务默认值。
pub(super) fn value(settings:UsageSettings,index:usize)->u16 {if index==0 {settings.scan_interval.get()}else{settings.retention_days.get()}}
/// 草稿可为空或越界；只在提交时用 core 值域判定合法。
pub(super) fn parsed(text:&str,index:usize)->Option<u16> {
    if text.is_empty()||!text.bytes().all(|b|b.is_ascii_digit()) {return None}
    let value=text.parse::<u16>().ok()?;UsageSettings::default().updated(field(index),value).ok()?;Some(value)
}
