//! Hooks宿主薄适配：只有应用启动绑定writer，文件操作与回执均由显式owner持有。

use super::UsageService;
use crate::monitor::store::HookConfigWriter;
use loki_metris_core::{
    AiTool, HookConfigLocation, HookConfigWriteResult, HookError, is_public_monitor_tool,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::oneshot;

/// 路径定位与写入共用同一个注入的主目录，测试不启动生产宿主。
pub(super) struct HookHost {
    pub writer: Arc<HookConfigWriter>,
    pub home: PathBuf,
}

/// 判断可执行文件是否位于重启或卸载后会消失的位置：挂载的磁盘映像卷，或 Gatekeeper 的临时转移目录。
/// Hook 命令会被写进其它工具的配置并长期保留，写入这类路径只会在之后留下失效命令。
pub(super) fn is_ephemeral_executable(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("/Volumes/") || text.contains("/AppTranslocation/")
}

impl UsageService {
    /// 窗口回归明确注入隔离目录和relay，普通测试构造仍不安装Hooks。
    #[cfg(test)]
    pub(crate) fn start_test_hook_writer(
        &self,
        executable: PathBuf,
        home: PathBuf,
    ) -> Result<(), &'static str> {
        self.start_hook_writer_at(executable, home)
    }
    /// 只由生产启动或明确隔离测试调用，普通in_directory构造不会安装Hooks。
    pub(super) fn start_hook_writer_at(
        &self,
        executable: PathBuf,
        home: PathBuf,
    ) -> Result<(), &'static str> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {
            return Err("usage_service_closed");
        }
        if self.context.agents.hooks.get().is_some() {
            return Ok(());
        }
        let writer = Arc::new(HookConfigWriter::start(executable, home.clone()));
        self.context.agents.bind_hooks(HookHost { writer, home })
    }

    /// 返回所有公开工具的真实主配置及附加路径，不做写入或安装状态推断。
    pub fn list_monitor_hook_locations(
        &self,
    ) -> Result<oneshot::Receiver<Result<Vec<HookConfigLocation>, HookError>>, &'static str> {
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
            .spawn_blocking("hook-locations-query", move |_| {
                let _ = tx.send(store.hook_locations_blocking());
            })?;
        Ok(rx)
    }

    /// 空目录恢复默认；接受前完成owner登记，页面关闭不丢弃已经接受的保存。
    pub fn save_hook_config_directory(
        &self,
        tool: AiTool,
        directory: String,
    ) -> Result<oneshot::Receiver<Result<HookConfigLocation, HookError>>, &'static str> {
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
            .spawn_blocking("hook-directory-mutation", move |_| {
                let _ = tx.send(store.save_hook_directory_blocking(tool, directory));
            })?;
        Ok(rx)
    }

    /// 显式写入等待串行worker回执，后台owner拥有Future并传播退出取消。
    pub fn write_monitor_hook_config(
        &self,
        tool: AiTool,
    ) -> Result<oneshot::Receiver<Result<HookConfigWriteResult, HookError>>, &'static str> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if runtime.is_none() {
            return Err("usage_service_closed");
        }
        let host = self
            .context
            .agents
            .hooks
            .get()
            .ok_or("hook_writer_unavailable")?;
        let writer = host.writer.clone();
        let directory = self.context.directory.join("monitor");
        let (tx, rx) = oneshot::channel();
        self.background
            .spawn("hook-config-mutation", move |_| async move {
                let result = if is_public_monitor_tool(tool) {
                    writer.write_config(directory, tool).await
                } else {
                    Err(HookError::new("error.hooks.toolUnavailable"))
                };
                let _ = tx.send(result);
            })?;
        Ok(rx)
    }
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "hooks_startup_tests.rs"]
mod startup_tests;
