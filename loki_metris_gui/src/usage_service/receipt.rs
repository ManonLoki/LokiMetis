//! GPUI与服务owner之间的回执桥接，不绑定调用窗口的生存期。

use loki_metris_core::HookError;

/// 跨 Tokio owner 回执采用有界轮询；不把 GPUI 假调度器 waker 注册到外部工作线程。
pub(crate) async fn receipt<T>(
    mut work: tokio::sync::oneshot::Receiver<Result<T, HookError>>,
    executor: gpui_kit::BackgroundExecutor,
    code: &'static str,
) -> Result<T, HookError> {
    loop {
        match work.try_recv() {
            Ok(result) => return result,
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                return Err(HookError::new(code));
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                executor.timer(std::time::Duration::from_millis(30)).await
            }
        }
    }
}
