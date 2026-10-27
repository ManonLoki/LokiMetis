//! 主机mDNS广播的有界回收；配对取消和页面切换不关闭应用级广播。

use super::{Devices, Duration};
use mdns_sd::{DaemonStatus, ServiceDaemon};

impl Devices {
    /// 应用退出时确认主机广播终态，超时仍保留句柄供销毁时重试。
    pub(in crate::usage_service) async fn cleanup_host_daemon(&self) -> Result<(), &'static str> {
        cleanup_mdns(&self.host_daemon).await
    }
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
