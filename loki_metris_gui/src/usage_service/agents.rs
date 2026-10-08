//! 统一 Agent 选择只保存于监控配置；看板、扫描和未来桌宠均消费同一权威快照。

use super::{UsageService, background::BackgroundTaskOwner};
use crate::monitor::settings::{MonitorSettings, load_monitor_settings, update_monitor_settings};
use loki_metris_core::{AiTool, HookError, SourceClientKind, dashboard_selection_for_ai_tools};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;

/// 内存仅缓存成功读取或提交的完整监控配置；没有第二份启用项落盘副本。
pub(super) struct AgentSettingsStore {
    directory: PathBuf,
    value: Mutex<Option<MonitorSettings>>,
    owner: Arc<BackgroundTaskOwner>,
    revision: AtomicU64,
    pub(super) listener: std::sync::OnceLock<Arc<crate::monitor::listener::HookListenerControl>>,
    pub(super) hooks: std::sync::OnceLock<super::hooks::HookHost>,
}

impl AgentSettingsStore {
    /// 目录由当前产品显式注入，构造不读原 App 配置或写 Hooks。
    pub(super) fn new(directory: PathBuf, owner: Arc<BackgroundTaskOwner>) -> Self {
        Self {
            directory,
            value: Mutex::new(None),
            owner,
            revision: AtomicU64::new(0),
            listener: Default::default(),
            hooks: Default::default(),
        }
    }

    /// 绑定与缓存读取/保存共享互斥，晚启动也不会错过已确认的启用集合。
    pub(super) fn bind_listener(
        &self,
        listener: Arc<crate::monitor::listener::HookListenerControl>,
    ) -> Result<(), &'static str> {
        let value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.listener
            .set(listener)
            .map_err(|_| "monitor_already_started")?;
        if let Some(settings) = &*value {
            self.listener
                .get()
                .unwrap()
                .replace_enabled_tools(&settings.enabled_ai_tools);
        }
        Ok(())
    }

    /// 绑定与已确认快照共用锁，普通查询命中缓存时不会重复触发修复。
    pub(super) fn bind_hooks(&self, host: super::hooks::HookHost) -> Result<(), &'static str> {
        let value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.hooks
            .set(host)
            .map_err(|_| "hook_writer_already_started")?;
        if let Some(settings) = &*value {
            self.hooks
                .get()
                .unwrap()
                .writer
                .request_enabled(settings.clone());
        }
        Ok(())
    }

    /// 缓存提交与两个宿主的快照发布保持同一顺序，失败写入不会发布。
    fn publish(&self, value: &mut Option<MonitorSettings>, settings: MonitorSettings) {
        *value = Some(settings.clone());
        if let Some(listener) = self.listener.get() {
            listener.replace_enabled_tools(&settings.enabled_ai_tools);
        }
        if let Some(host) = self.hooks.get() {
            host.writer.request_enabled(settings);
        }
    }

    /// 生产定位使用系统用户目录，隔离宿主则使用明确注入的目录。
    fn hook_home(&self) -> Result<PathBuf, HookError> {
        self.hooks
            .get()
            .map(|host| host.home.clone())
            .or_else(crate::backend::local_index::current_user_home)
            .ok_or_else(|| HookError::new("error.hooks.homeDirectoryUnavailable"))
    }

    /// 首次读取失败不缓存默认值，后续显式重试仍可恢复真实配置。
    pub(super) async fn snapshot(self: &Arc<Self>) -> Result<MonitorSettings, HookError> {
        if let Ok(value) = self.value.try_lock() {
            if let Some(settings) = &*value {
                return Ok(settings.clone());
            }
        }
        let store = self.clone();
        let (tx, rx) = oneshot::channel();
        self.owner
            .spawn_blocking("agent-settings-read", move |_| {
                let _ = tx.send(store.snapshot_blocking());
            })
            .map_err(|_| HookError::new("error.monitor.settingsReadFailed"))?;
        rx.await
            .map_err(|_| HookError::new("error.monitor.settingsReadFailed"))?
    }

    /// 同步锁只在已登记的阻塞岛内覆盖实际读取与内存提交。
    pub(super) fn snapshot_blocking(&self) -> Result<MonitorSettings, HookError> {
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(settings) = &*value {
            return Ok(settings.clone());
        }
        let settings = load_monitor_settings(&self.directory)?;
        self.publish(&mut value, settings.clone());
        Ok(settings)
    }

    /// 每次读改写只替换启用集合，目录覆盖、桌宠位置和窗口偏好继续保留。
    fn save_blocking(&self, tools: Vec<AiTool>) -> Result<MonitorSettings, HookError> {
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let settings = update_monitor_settings(&self.directory, |settings| {
            settings.enabled_ai_tools = tools
        })?;
        self.publish(&mut value, settings.clone());
        self.revision.fetch_add(1, Ordering::Release);
        Ok(settings)
    }

    /// 路径查询在已登记阻塞岛内执行，覆盖值与当前启用集合共享同一缓存。
    pub(super) fn hook_locations_blocking(
        &self,
    ) -> Result<Vec<loki_metris_core::HookConfigLocation>, HookError> {
        let settings = self.snapshot_blocking()?;
        Ok(crate::monitor::store::list_hook_config_locations(
            &settings,
            &self.hook_home()?,
        ))
    }

    /// 保存目录只替换一个字段，完整快照仍提交给监听器与自动写入宿主。
    pub(super) fn save_hook_directory_blocking(
        &self,
        tool: AiTool,
        directory: String,
    ) -> Result<loki_metris_core::HookConfigLocation, HookError> {
        if !loki_metris_core::is_public_monitor_tool(tool) {
            return Err(HookError::new("error.hooks.toolUnavailable"));
        }
        let directory = crate::monitor::store::validate_hook_config_directory(&directory)?;
        let home = self.hook_home()?;
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let settings = update_monitor_settings(&self.directory, |settings| {
            settings.hook_directories.set(tool, directory)
        })?;
        let location = crate::monitor::store::list_hook_config_locations(&settings, &home)
            .into_iter()
            .find(|location| location.tool == tool)
            .ok_or_else(|| HookError::new("error.hooks.locationNotFound"))?;
        self.publish(&mut value, settings);
        self.revision.fetch_add(1, Ordering::Release);
        Ok(location)
    }

    /// 公开能力目录在 core 中决定映射；WorkBuddy 单独走只读模型。
    pub(super) async fn dashboard_selection(
        self: &Arc<Self>,
    ) -> Result<(Vec<SourceClientKind>, bool), &'static str> {
        self.snapshot()
            .await
            .map(|settings| dashboard_selection_for_ai_tools(&settings.enabled_ai_tools))
            .map_err(|_| "usage_agent_settings_unavailable")
    }
}

impl UsageService {
    /// 选择代数只通知展示刷新，不触发扫描或外部 Agent 配置写入。
    pub fn agent_settings_revision(&self) -> u64 {
        self.context.agents.revision.load(Ordering::Acquire)
    }

    /// 设置查询独立于页面查询邮箱；关闭页面不会取消已接受的配置操作。
    pub fn monitor_settings(
        &self,
    ) -> Result<oneshot::Receiver<Result<MonitorSettings, HookError>>, &'static str> {
        let store = self.context.agents.clone();
        let (tx, rx) = oneshot::channel();
        self.background
            .spawn_blocking("agent-settings-query", move |_| {
                let _ = tx.send(store.snapshot_blocking());
            })?;
        Ok(rx)
    }

    /// 保存成功返回规范化的权威快照，失败保持此前展示选择和实际配置。
    pub fn save_enabled_ai_tools(
        &self,
        tools: Vec<AiTool>,
    ) -> Result<oneshot::Receiver<Result<MonitorSettings, HookError>>, &'static str> {
        // 与 finish 的 runtime 接管互斥，阻塞任务必须在接受回执前完成 owner 登记。
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {
            return Err("usage_service_closed");
        }
        let store = self.context.agents.clone();
        let (tx, rx) = oneshot::channel();
        self.background
            .spawn_blocking("agent-settings-mutation", move |_| {
                let _ = tx.send(store.save_blocking(tools));
            })?;
        drop(runtime);
        Ok(rx)
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
