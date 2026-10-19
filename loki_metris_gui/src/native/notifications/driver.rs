//! 原生适配只处理系统调用、回执与纯值事件，不拥有阈值或重试规则。
use super::Error;
use gpui_kit::BackgroundExecutor;
use loki_metris_core::usage_milestones::{MilestoneOutcome, NotificationPermission};
use std::{sync::mpsc::SyncSender, time::Instant};
/// 每次显式授权独占取消位；外层超时或退出丢弃 future 时阻止迟到的系统写入。
#[cfg(windows)]
#[derive(Clone)]
pub(super) struct Authorization {
    /// 授权必须完成的截止时刻；同步宿主边界前后都会检查。
    deadline:Instant,
    /// 与 AuthorizationGuard 共享的取消位；等待的 future 被丢弃时置位。
    cancelled:std::sync::Arc<std::sync::atomic::AtomicBool>,
}
/// 只由等待该授权的 future 持有，不能移入无法中断的同步 WinRT worker。
#[cfg(windows)]
pub(super) struct AuthorizationGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
#[cfg(windows)]
impl Authorization {
    /// 创建独立操作控制，重试不复用已取消的前一代标志。
    pub(super) fn new(deadline:Instant)->(Self,AuthorizationGuard) {
        let cancelled=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        (Self{deadline,cancelled:cancelled.clone()},AuthorizationGuard(cancelled))
    }
    /// 同步宿主边界前后检查，已经开始的系统清理不受此检查阻断。
    pub(super) fn check(&self)->Result<(),Error> {
        if Instant::now()>=self.deadline {return Err(Error::Timeout)}
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {return Err(Error::Unavailable)}
        Ok(())
    }
}
#[cfg(windows)]
impl Drop for AuthorizationGuard {
    /// 外层 future 完成、超时或取消均撤销之后尚未开始的系统动作。
    fn drop(&mut self) {self.0.store(true,std::sync::atomic::Ordering::Release);}
}
/// 有界平台回调，点击必须在 UI 线程恢复主窗口。
pub(super) enum Event {
    /// macOS delegate 回调产生；Windows 点击经宿主的 `pending_click` 标志轮询，不走事件通道。
    #[cfg_attr(any(windows, target_os = "linux"), allow(dead_code))]
    Clicked,
    /// 只有 Windows 的晚到失败回调会产生；其它平台不会构造，但事件处理对所有平台统一。
    #[cfg_attr(not(windows), allow(dead_code))]
    LateFailure(String),
}
#[cfg(target_os = "macos")]
pub(super) use super::macos_delivery::Host;
#[cfg(windows)]
pub(super) use super::windows::Host;
#[cfg(target_os = "linux")]
pub(super) use super::linux::Host;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
/// 不支持原生通知平台的占位宿主，构造总是失败。
pub(super) struct Host;
#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
impl Host {
    /// 当前平台没有原生通知实现，直接返回 None，上层据此把通知功能显示为不可用。
    pub fn new(_: SyncSender<Event>) -> Option<Self> {
        None
    }
}
/// 唯一 owner 在任何系统查询前先建立，不请求权限。
pub(super) fn install(events: SyncSender<Event>, executor: BackgroundExecutor) -> Option<Host> {
    #[cfg(target_os = "linux")]
    { Host::new(events, executor) }
    #[cfg(not(target_os = "linux"))]
    {
    let _ = executor;
    Host::new(events)
    }
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
    #[cfg(target_os = "linux")]
    {
        executor.spawn(super::linux::permission()).await
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
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
        let (control,_guard)=Authorization::new(deadline);
        executor.spawn(async move { super::windows::authorize(&control) }).await
    }
    #[cfg(target_os = "linux")]
    {
        let _ = deadline;
        if executor.spawn(super::linux::permission()).await == NotificationPermission::Allowed {
            Ok(())
        } else { Err(Error::Unavailable) }
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (executor, deadline);
        Err(Error::Unavailable)
    }
}
/// 同步创建平台提交以冻结本次撤销意图；系统接受不是实际送达，回执仍由前台有限等待。
pub(super) fn send(
    host: &Host,
    executor: BackgroundExecutor,
    id: String,
    title: String,
    body: String,
) -> impl std::future::Future<Output = MilestoneOutcome> + 'static + use<> {
    #[cfg(target_os = "macos")]
    {
        let _ = (host, executor);
        async move { super::macos_delivery::send(&id, &title, &body)
            .await
            .unwrap_or(MilestoneOutcome::Unknown) }
    }
    #[cfg(windows)]
    {
        let host = host.clone();
        async move { executor
            .spawn(async move { host.send(&id, &title, &body) })
            .await }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = executor;
        host.send(id, title, body)
    }
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (host, executor, id, title, body);
        async { MilestoneOutcome::Rejected }
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

/// Windows 点击不与失败回执争抢队列容量；其他平台保留其原生点击事件。
pub(super) fn take_click(host: &Host) -> bool {
    #[cfg(any(windows, target_os = "linux"))]
    { host.take_click() }
    #[cfg(not(any(windows, target_os = "linux")))]
    { let _ = host; false }
}
