//! 原生退出阶段拥有一次性收尾；不能依赖 macOS NSApplication terminate 返回 main。

use crate::preferences::Writer;
use gpui_kit::App;
use std::{cell::RefCell, rc::Rc};
use tracing_appender::non_blocking::WorkerGuard;

/// 同一状态在原生退出回调与事件循环返回回退间共享，日志 worker 只刷新一次。
pub struct Shutdown {
    writer: Writer,
    log_guard: Option<WorkerGuard>,
}

impl Shutdown {
    /// 接管主入口的偏好 worker 和日志 guard，所有克隆只共享同一个收尾状态。
    pub fn new(writer: Writer, log_guard: WorkerGuard) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self { writer, log_guard: Some(log_guard) }))
    }

    /// 不可取消退出时先保存最终偏好，再记录停止事件并等待本地日志 worker 刷新。
    pub fn finish(&mut self) {
        let Some(guard) = self.log_guard.take() else { return };
        self.writer.finish();
        tracing::info!(event = "application_stopped");
        drop(guard);
    }
}

/// AppKit 终止进程前会调用此钩子；同步收尾不受 GPUI 短期异步退出超时取消。
pub fn install(cx: &App, shutdown: Rc<RefCell<Shutdown>>) {
    cx.on_app_quit(move |_| {
        shutdown.borrow_mut().finish();
        std::future::ready(())
    }).detach();
}

#[cfg(test)]
#[path = "lifecycle_test.rs"]
mod tests;
