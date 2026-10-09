//! 原生退出阶段拥有一次性收尾；不能依赖 macOS NSApplication terminate 返回 main。

use crate::preferences::Writer;
use gpui_kit::App;
use std::{cell::RefCell, rc::Rc};
use tracing_appender::non_blocking::WorkerGuard;

/// 同一状态在原生退出回调与事件循环返回回退间共享，日志 worker 只刷新一次。
pub struct Shutdown {
    writer: Writer,
    notifications:Option<crate::native::notifications::Service>,
    log_guard: Option<WorkerGuard>,
    usage: Option<std::sync::Arc<crate::usage_service::UsageService>>,
}

impl Shutdown {
    /// 接管主入口的偏好 worker 和日志 guard，所有克隆只共享同一个收尾状态。
    pub fn new(writer: Writer, log_guard: WorkerGuard) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { writer, notifications:None, log_guard: Some(log_guard), usage: None }))
    }

    /// 通知授权必须在统一收尾点失效，不能依赖独立退出钩子的执行顺序。
    pub fn own_notifications(&mut self,service:crate::native::notifications::Service) {self.notifications=Some(service);}

    /// 应用退出钩子拥有业务运行时，窗口隐藏不会停止扫描或销毁服务。
    pub fn own_usage(&mut self, usage: std::sync::Arc<crate::usage_service::UsageService>) {
        self.usage = Some(usage);
    }

    /// 不可取消退出时先保存最终偏好，再记录停止事件并等待本地日志 worker 刷新。
    pub fn finish(&mut self) {self.finish_with_pet(crate::usage_service::PetShutdownPlan::default());}

    /// 唯一退出钩子先捕获桌宠意图，避免依赖短期异步退出预算或钩子顺序。
    fn finish_with_pet(&mut self,plan:crate::usage_service::PetShutdownPlan) {
        let Some(guard) = self.log_guard.take() else { return };
        if let Some(notifications)=self.notifications.take() {notifications.stop();}
        if let Some(usage) = self.usage.take() { usage.finish_with_pet(plan); }
        self.writer.finish();
        tracing::info!(event = "application_stopped");
        drop(guard);
    }
}

/// AppKit 终止进程前会调用此钩子；同步收尾不受 GPUI 短期异步退出超时取消。
pub fn install(cx: &App, shutdown: Rc<RefCell<Shutdown>>) {
    cx.on_app_quit(move |cx| {
        let plan=crate::pet::take_shutdown_plan(cx);
        shutdown.borrow_mut().finish_with_pet(plan);
        std::future::ready(())
    }).detach();
}

#[cfg(test)]
#[path = "lifecycle_test.rs"]
mod tests;
