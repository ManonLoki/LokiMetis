//! 用量设置串行落盘成功后才更新内存；读取和保存不认领索引 writer。

mod store;
mod lifecycle;
#[cfg(test)]
mod tests;

use super::{UsageService,background::BackgroundTaskOwner};
use loki_metris_core::{UsageSettings,UsageSettingField};
use std::{path::PathBuf,sync::{Arc,Mutex}};
use tokio::sync::oneshot;

/// 一把同步锁在阻塞岛内覆盖读取、克隆、单字段修改、实际落盘和内存提交全过程。
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
    pub(super) async fn snapshot(self:&Arc<Self>)->Result<UsageSettings,&'static str> {
        if let Ok(value)=self.value.try_lock()&& let Some(settings)=*value {return Ok(settings)}
        let store=self.clone();let(tx,rx)=oneshot::channel();
        self.owner.spawn_blocking("usage-settings-read",move|_|{let _=tx.send(store.snapshot_blocking());})?;
        rx.await.map_err(|_|"usage_settings_unavailable")?
    }

    /// 同步存储锁仅在真实阻塞任务内持有，不在异步 worker 中执行磁盘 I/O。
    fn snapshot_blocking(&self)->Result<UsageSettings,&'static str> {
        let mut value=self.value.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(settings)=*value {return Ok(settings)}
        let settings=store::load(&self.path).map_err(|_|"usage_settings_unavailable")?;
        *value=Some(settings);Ok(settings)
    }

    /// 两卡并发保存以最新完整快照为基线；失败不改变任何已生效值。
    fn save_blocking(&self,field:UsageSettingField,input:u16)->Result<UsageSettings,&'static str> {
        UsageSettings::default().updated(field,input).map_err(|_|"usage_setting_invalid")?;
        self.snapshot_blocking()?;
        let mut value=self.value.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let settings=value.expect("已读取设置").updated(field,input).map_err(|_|"usage_setting_invalid")?;
        store::save(&self.path,settings).map_err(|_|"usage_setting_save_failed")?;
        *value=Some(settings);self.revision.fetch_add(1,std::sync::atomic::Ordering::Release);Ok(settings)
    }
}

impl UsageService {
    /// 设置代数只用于唤醒展示刷新，不替代权威快照或触发扫描。
    pub fn usage_settings_revision(&self)->u64 {self.context.settings.revision.load(std::sync::atomic::Ordering::Acquire)}
    /// 设置读取独立于用量查询邮箱，切客户端不会取消另一张卡的写入。
    pub fn usage_settings(&self)->Result<oneshot::Receiver<Result<UsageSettings,&'static str>>,&'static str> {
        let store=self.context.settings.clone();let(tx,rx)=oneshot::channel();
        self.background.spawn_blocking("usage-settings-query",move|_|{let _=tx.send(store.snapshot_blocking());})?;Ok(rx)
    }

    /// 每次保存有独立真实回执；页面卸载后，已接受的应用任务仍完成原子落盘。
    pub fn save_usage_setting(&self,field:UsageSettingField,value:u16)->Result<oneshot::Receiver<Result<UsageSettings,&'static str>>,&'static str> {
        UsageSettings::default().updated(field,value).map_err(|_|"usage_setting_invalid")?;
        // 关闭不能在任务预约与 owner 采用之间插入，避免拒绝后仍提交的歧义。
        let runtime=self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {return Err("usage_service_closed")}
        let store=self.context.settings.clone();let(tx,rx)=oneshot::channel();
        self.background.spawn_blocking("usage-settings-mutation",move|_|{let _=tx.send(store.save_blocking(field,value));})?;drop(runtime);Ok(rx)
    }
}
