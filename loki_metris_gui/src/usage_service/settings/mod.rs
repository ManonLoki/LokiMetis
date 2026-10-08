//! 用量设置串行落盘成功后才更新内存；读取和保存不认领索引 writer。

mod store;
mod lifecycle;
#[cfg(test)]
mod tests;

use super::{UsageService,background::BackgroundTaskOwner};
use loki_metris_core::{UsageSettings,UsageSettingField};
use std::{path::PathBuf,sync::Arc};
use tokio::sync::{Mutex,oneshot};

/// 一把异步锁覆盖读取、克隆、单字段修改、实际落盘和内存提交全过程。
pub(super) struct SettingsStore {
    path:PathBuf,
    value:Mutex<Option<UsageSettings>>,
    owner:Arc<BackgroundTaskOwner>,
    revision:std::sync::atomic::AtomicU64,
}

impl SettingsStore {
    /// 文件位于当前产品数据命名空间，构造不执行任何磁盘写入。
    pub(super) fn new(path:PathBuf,owner:Arc<BackgroundTaskOwner>)->Self {Self{path,value:Mutex::new(None),owner,revision:Default::default()}}

    /// 首次有界读取失败保持未加载；后续显式重试可重新读取，不伪造默认成功。
    pub(super) async fn snapshot(&self)->Result<UsageSettings,&'static str> {
        let mut value=self.value.lock().await;
        if let Some(settings)=*value {return Ok(settings)}
        let path=self.path.clone();let(tx,rx)=oneshot::channel();
        self.owner.spawn_blocking("usage-settings-read",move|_|{let _=tx.send(store::load(&path));})?;
        let settings=rx.await.map_err(|_|"usage_settings_unavailable")?.map_err(|_|"usage_settings_unavailable")?;
        *value=Some(settings);Ok(settings)
    }

    /// 两卡并发保存以最新完整快照为基线；失败不改变任何已生效值。
    async fn save(&self,field:UsageSettingField,input:u16)->Result<UsageSettings,&'static str> {
        UsageSettings::default().updated(field,input).map_err(|_|"usage_setting_invalid")?;
        self.snapshot().await?;
        let mut value=self.value.lock().await;
        let settings=value.expect("已读取设置").updated(field,input).map_err(|_|"usage_setting_invalid")?;
        let path=self.path.clone();let(tx,rx)=oneshot::channel();
        self.owner.spawn_blocking("usage-settings-save",move|_|{let _=tx.send(store::save(&path,settings));})?;
        rx.await.map_err(|_|"usage_setting_save_failed")?.map_err(|_|"usage_setting_save_failed")?;
        *value=Some(settings);self.revision.fetch_add(1,std::sync::atomic::Ordering::Release);Ok(settings)
    }
}

impl UsageService {
    /// 设置代数只用于唤醒展示刷新，不替代权威快照或触发扫描。
    pub fn usage_settings_revision(&self)->u64 {self.context.settings.revision.load(std::sync::atomic::Ordering::Acquire)}
    /// 设置读取独立于用量查询邮箱，切客户端不会取消另一张卡的写入。
    pub fn usage_settings(&self)->Result<oneshot::Receiver<Result<UsageSettings,&'static str>>,&'static str> {
        let store=self.context.settings.clone();let(tx,rx)=oneshot::channel();
        self.background.spawn("usage-settings-query",move|_|async move {let _=tx.send(store.snapshot().await);})?;Ok(rx)
    }

    /// 每次保存有独立真实回执；页面卸载后，已接受的应用任务仍完成原子落盘。
    pub fn save_usage_setting(&self,field:UsageSettingField,value:u16)->Result<oneshot::Receiver<Result<UsageSettings,&'static str>>,&'static str> {
        UsageSettings::default().updated(field,value).map_err(|_|"usage_setting_invalid")?;
        let store=self.context.settings.clone();let(tx,rx)=oneshot::channel();
        self.background.spawn("usage-settings-mutation",move|_|async move {let _=tx.send(store.save(field,value).await);})?;Ok(rx)
    }
}
