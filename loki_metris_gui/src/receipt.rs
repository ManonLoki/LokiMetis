//! GPUI 与服务层 owner 之间的回执桥接，不绑定调用窗口的生存期。
//!
//! 服务层只交付 Tokio oneshot 回执；用 GPUI 计时器轮询属于界面层，因此位于 GUI crate。

use std::time::Duration;

use loki_metris_core::HookError;
use loki_metris_service::skins::SkinError;
use tokio::sync::oneshot::{self, error::TryRecvError};

/// Hook 回执的轮询间隔；与页面交互延迟同量级，不在空闲时产生唤醒。
const HOOK_RECEIPT_POLL: Duration = Duration::from_millis(30);

/// 以固定间隔有界轮询一个跨 owner 的一次性回执，发送端丢弃时返回 `None`。
///
/// 回执由外部 Tokio owner 完成；这里只用 GPUI 自己的计时器轮询，
/// 不把 GPUI（尤其是测试假调度器）的 waker 注册到外部工作线程。
/// 轮询只在请求在途期间存在，回执到达或任务被丢弃即停止，不形成空闲唤醒。
pub(crate) async fn poll_receipt<T>(
    mut work: oneshot::Receiver<T>,
    executor: gpui_kit::BackgroundExecutor,
    interval: Duration,
) -> Option<T> {
    loop {
        match work.try_recv() {
            Ok(value) => return Some(value),
            Err(TryRecvError::Closed) => return None,
            Err(TryRecvError::Empty) => executor.timer(interval).await,
        }
    }
}

/// 等待 Hook 服务回执；owner 提前丢弃发送端时映射为调用方给定的稳定错误码。
pub(crate) async fn hook_receipt<T>(
    work: oneshot::Receiver<Result<T, HookError>>,
    executor: gpui_kit::BackgroundExecutor,
    code: &'static str,
) -> Result<T, HookError> {
    poll_receipt(work, executor, HOOK_RECEIPT_POLL)
        .await
        .unwrap_or_else(|| Err(HookError::new(code)))
}

/// 等待换肤服务回执；owner 提前丢弃发送端时返回稳定的不可用错误。
pub(crate) async fn skin_receipt<T>(
    work: oneshot::Receiver<Result<T, SkinError>>,
    executor: gpui_kit::BackgroundExecutor,
) -> Result<T, SkinError> {
    poll_receipt(work, executor, Duration::from_millis(30))
        .await
        .unwrap_or_else(|| {
            Err(SkinError::new(
                "skin.operation_unavailable",
                "换肤操作回执不可用。",
            ))
        })
}
