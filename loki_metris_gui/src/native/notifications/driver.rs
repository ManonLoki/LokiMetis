//! 原生适配只处理系统调用、回执与纯值事件，不拥有阈值或重试规则。
use super::Error;
use gpui_kit::BackgroundExecutor;
use loki_metris_core::usage_milestones::{MilestoneOutcome, NotificationPermission};
use std::{sync::mpsc::SyncSender, time::Instant};
/// 有界平台回调，点击必须在 UI 线程恢复主窗口。
pub(super) enum Event {
    Clicked,
    LateFailure(String),
}
#[cfg(target_os = "macos")]
pub(super) use super::macos_delivery::Host;
#[cfg(windows)]
pub(super) use super::windows::Host;
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) struct Host;
#[cfg(not(any(target_os = "macos", windows)))]
impl Host {
    pub fn new(_: SyncSender<Event>) -> Option<Self> {
        None
    }
}
/// 唯一 owner 在任何系统查询前先建立，不请求权限。
pub(super) fn install(events: SyncSender<Event>) -> Option<Host> {
    Host::new(events)
}
/// 定时、激活和设置页只读系统权限。
pub(super) async fn permission(executor: BackgroundExecutor) -> NotificationPermission {
    #[cfg(target_os = "macos")]
    {
        let _ = executor;
        super::macos::permission().await
    }
    #[cfg(windows)]
    {
        executor.spawn(async { super::windows::permission() }).await
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = executor;
        NotificationPermission::Unavailable
    }
}
/// 仅用户开启动作允许申请权限或登记本产品 AUMID。
pub(super) async fn authorize(
    executor: BackgroundExecutor,
    deadline: Instant,
) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    {
        super::macos::authorize(executor, deadline).await
    }
    #[cfg(windows)]
    {
        let _ = deadline;
        executor.spawn(async { super::windows::authorize() }).await
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (executor, deadline);
        Err(Error::Unavailable)
    }
}
/// 系统接受不是实际送达；每次调用由前台统一给予有限等待。
pub(super) async fn send(
    host: &Host,
    executor: BackgroundExecutor,
    id: String,
    title: String,
    body: String,
) -> MilestoneOutcome {
    #[cfg(target_os = "macos")]
    {
        let _ = (host, executor);
        super::macos_delivery::send(&id, &title, &body)
            .await
            .unwrap_or(MilestoneOutcome::Unknown)
    }
    #[cfg(windows)]
    {
        let host = host.clone();
        executor
            .spawn(async move { host.send(&id, &title, &body) })
            .await
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (host, executor, id, title, body);
        MilestoneOutcome::Rejected
    }
}
/// 重启后只用平台保留历史确认相同ID，找不到不自动发送。
pub(super) async fn accepted(executor: BackgroundExecutor, id: String) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = executor;
        super::macos_delivery::accepted(id).await
    }
    #[cfg(windows)]
    {
        executor
            .spawn(async move { super::windows::accepted(&id) })
            .await
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (executor, id);
        false
    }
}

/// 点击可合并，但失败回执溢出必须披露。
pub(super) fn overflow(host: &Host) -> bool {
    #[cfg(windows)]
    {
        host.overflow()
    }
    #[cfg(not(windows))]
    {
        let _ = host;
        false
    }
}
