//! 主机mDNS广播的有界回收；配对取消和页面切换不关闭应用级广播。

use super::{Devices, Duration, Instant};
use loki_metris_core::devices::{DeviceBook, link::HOST_SERVICE_TYPE};
use mdns_sd::{DaemonEvent, DaemonStatus, ServiceDaemon};
use std::time::SystemTime;

/// 墙钟比单调时钟多走超过此值即视为系统睡眠过；唤醒后网卡可能保留原IP，mDNS库察觉不到组播成员已失效。
const SLEEP_DETECTION_GAP: Duration = Duration::from_secs(10);

impl Devices {
    /// 确认旧daemon退出后用全新socket重新加入组播并公告；重建期间旧广播的goodbye让终端开启新一轮重连。
    pub(super) async fn advertise(&self, book: &DeviceBook, port: u16) -> Result<mdns_sd::Receiver<DaemonEvent>, &'static str> {
        cleanup_mdns(&self.host_daemon).await?;
        let daemon = ServiceDaemon::new().map_err(|_| "device_discovery_failed")?;
        *self.host_daemon.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(daemon.clone());
        // 首版监听IPv4；不发布无法接受连接的IPv6或环回端点。
        daemon.disable_interface(vec![mdns_sd::IfKind::IPv6, mdns_sd::IfKind::LoopbackV4]).map_err(|_| "device_discovery_failed")?;
        let instance = self.instance_name(book); let host = format!("loki-{}.local.", book.client_id());
        let properties = [("v", "1"), ("id", book.client_id()), ("name", instance.as_str())];
        let info = mdns_sd::ServiceInfo::new(HOST_SERVICE_TYPE, &instance, &host, "", port, properties.as_slice())
            .map_err(|_| "device_discovery_failed")?.enable_addr_auto();
        let monitor = daemon.monitor().map_err(|_| "device_discovery_failed")?;
        daemon.register(info).map_err(|_| "device_discovery_failed")?;
        Ok(monitor)
    }

    /// 应用退出时确认主机广播终态，超时仍保留句柄供销毁时重试。
    pub(in crate::usage_service) async fn cleanup_host_daemon(&self) -> Result<(), &'static str> {
        cleanup_mdns(&self.host_daemon).await
    }
}

/// macOS/Linux 用墙钟与单调时钟的差值检测睡眠；Windows 的单调时钟包含睡眠，另检查原生累计睡眠量。
pub(super) fn system_slept(previous: (Instant, SystemTime), now: (Instant, SystemTime)) -> bool {
    let monotonic = now.0.saturating_duration_since(previous.0);
    now.1.duration_since(previous.1).is_ok_and(|wall| wall > monotonic + SLEEP_DETECTION_GAP)
}

/// 原生累计睡眠量增加超过阈值时重建广播；读取失败、计数回退或正常调度延迟不误判。
pub(super) fn suspend_advanced(previous: Option<Duration>, now: Option<Duration>) -> bool {
    previous.zip(now).is_some_and(|(previous, now)| now.saturating_sub(previous) > SLEEP_DETECTION_GAP)
}

/// 主机广播有界确认退出；失败保留精确句柄供下次或应用退出重试。
pub(super) async fn cleanup_mdns(slot: &std::sync::Mutex<Option<ServiceDaemon>>) -> Result<(), &'static str> {
    let daemon = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let Some(daemon) = daemon else { return Ok(()); };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let stopped = match daemon.shutdown() {
            Ok(receiver) => matches!(tokio::time::timeout_at(deadline, receiver.recv_async()).await, Ok(Ok(DaemonStatus::Shutdown))),
            Err(mdns_sd::Error::DaemonShutdown) => true,
            Err(_) => false,
        };
        if stopped {
            slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take(); return Ok(());
        }
        if tokio::time::Instant::now() >= deadline { return Err("device_discovery_failed"); }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

impl Drop for Devices {
    /// 异常销毁仍发送关闭请求；正常路径在应用退出时已经确认Shutdown。
    fn drop(&mut self) {
        if let Some(daemon) = self.host_daemon.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).take()
            && daemon.shutdown().is_err() {
            tracing::warn!(event = "device_mdns_shutdown_unconfirmed");
        }
    }
}
