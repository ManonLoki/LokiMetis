//! 原生监控管理的有界后台文件操作；接受的修改独立于页面回执生命周期。

use super::UsageService;
use crate::monitor::{
    images::{self, MonitorGallerySnapshot, MonitorImageImport},
    profiles,
};
use loki_metris_core::{
    AiProfileDraft, AiProfileDraftSet, HookError, is_public_monitor_tool, public_monitor_ai_tools,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};
use tokio::sync::oneshot;

impl UsageService {
    /// 图库和档案提交代数只作刷新通知，不复制磁盘权威状态。
    pub fn monitor_catalog_revision(&self) -> u64 {
        self.context.monitor_revision.load(Ordering::Acquire)
    }

    /// 接受操作前登记 owner 并与退出互斥，关闭页面不能取消已接受写入。
    fn monitor_catalog_work<T: Send + 'static>(
        &self,
        mutation: bool,
        work: impl FnOnce(&Path, &Path) -> Result<T, HookError> + Send + 'static,
    ) -> Result<oneshot::Receiver<Result<T, HookError>>, &'static str> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {
            return Err("usage_service_closed");
        }
        let state = self.context.clone();
        let (tx, rx) = oneshot::channel();
        self.background
            .spawn_blocking("monitor-catalog", move |_| {
                let result = work(&state.directory.join("monitor"), &state.directory);
                if mutation && result.is_ok() {
                    state.monitor_revision.fetch_add(1, Ordering::Release);
                }
                let _ = tx.send(result);
            })?;
        drop(runtime);
        Ok(rx)
    }

    /// 公开目录决定可编辑档案，内部未公开 Agent 保存内容继续保留。
    pub fn list_monitor_profile_drafts(
        &self,
    ) -> Result<oneshot::Receiver<Result<AiProfileDraftSet, HookError>>, &'static str> {
        self.monitor_catalog_work(false, |config, _| {
            let set = profiles::load_profile_drafts(config)?;
            Ok(AiProfileDraftSet {
                drafts: public_monitor_ai_tools()
                    .filter_map(|tool| set.drafts.iter().find(|draft| draft.tool == tool).cloned())
                    .collect(),
            })
        })
    }

    /// 保存只接收公开工具；槽位、行为和图片引用交 core 校验后提交。
    pub fn save_monitor_profile_draft(
        &self,
        draft: AiProfileDraft,
    ) -> Result<oneshot::Receiver<Result<AiProfileDraft, HookError>>, &'static str> {
        self.monitor_catalog_work(true, move |config, data| {
            if !is_public_monitor_tool(draft.tool) {
                return Err(HookError::new("error.hooks.toolUnavailable"));
            }
            profiles::save_profile_draft(config, data, draft)
        })
    }

    /// 单次快照包括真实原图字节，原生层不需要网络 URI 或另一次竞态读取。
    pub fn list_monitor_images(
        &self,
    ) -> Result<oneshot::Receiver<Result<MonitorGallerySnapshot, HookError>>, &'static str> {
        self.monitor_catalog_work(false, |_, data| images::list_monitor_images(data))
    }

    /// 文件选择器只交路径，读取和复制在已登记后台操作内完成。
    pub fn import_monitor_image(
        &self,
        path: PathBuf,
    ) -> Result<oneshot::Receiver<Result<MonitorImageImport, HookError>>, &'static str> {
        self.monitor_catalog_work(true, move |_, data| {
            images::import_monitor_image(data, &path)
        })
    }

    /// 删除只修改应用副本；所有已保存档案的引用均在同一事务内检查。
    pub fn delete_monitor_image(
        &self,
        id: String,
    ) -> Result<oneshot::Receiver<Result<MonitorGallerySnapshot, HookError>>, &'static str> {
        self.monitor_catalog_work(true, move |config, data| {
            images::delete_monitor_image(config, data, &id)
        })
    }

    /// 桌宠只可通过图库 ID 请求资源，不能任意读取用户路径。
    pub fn get_monitor_image_bytes(
        &self,
        id: String,
    ) -> Result<oneshot::Receiver<Result<Arc<[u8]>, HookError>>, &'static str> {
        self.monitor_catalog_work(false, move |_, data| images::read_monitor_image(data, &id))
    }
}

#[cfg(test)]
#[path = "monitor_catalog_tests.rs"]
mod tests;
