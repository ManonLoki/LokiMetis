//! mDNS浏览只解析兼容服务；权限、组播和退出失败均显式反映到页面。

use super::{Devices, Duration, Instant};
use loki_metris_core::devices::{DEVICE_SERVICE_TYPE, DiscoveredDevice, MAX_DISCOVERED_DEVICES};
use mdns_sd::{DaemonEvent, DaemonStatus, ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent};
use std::{net::{SocketAddr, SocketAddrV6}, sync::Arc};
use tokio::sync::watch;

impl Devices {
    /// 每次浏览12秒；重复点击不创建额外daemon，所有退出路径均请求并确认关闭。
    pub(super) fn discover(self: &Arc<Self>) -> Result<(), &'static str> {
        self.discover_for(None)
    }

    /// 自动重启必须在预约新轮的锁内复核取消代次，不能持旧布尔值越过离页动作。
    pub(super) fn discover_for(self: &Arc<Self>, expected_epoch: Option<u64>) -> Result<(), &'static str> {
        let (cancel, mut cancelled) = watch::channel(false);
        {
            let mut state = self.state();
            if expected_epoch.is_some_and(|epoch| epoch != state.scan_epoch) { return Ok(()); }
            if state.view.scanning {
                if state.scan_cancel.as_ref().is_some_and(|cancel| *cancel.borrow()) { state.restart_scan = true; }
                return Ok(());
            }
            state.view.scanning = true; state.view.discovery_error = None; state.restart_scan = false;
            state.candidates.clear(); state.scan_cancel = Some(cancel);
        }
        let this = self.clone();
        if let Err(error) = self.owner.spawn("device-mdns", move |mut shutdown| async move {
            let result = async {
                // 上次异常关停仍保留句柄，确认退出前绝不创建第二个daemon。
                this.cleanup_daemon().await?;
                if shutdown.is_cancelled() || *cancelled.borrow() { return Ok(()); }
                let daemon = ServiceDaemon::new().map_err(|_| "device_discovery_failed")?;
                *this.daemon.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(daemon.clone());
                let result = async {
                    let monitor = daemon.monitor().map_err(|_| "device_discovery_failed")?;
                    let events = daemon.browse(DEVICE_SERVICE_TYPE).map_err(|_| "device_discovery_failed")?;
                    let deadline = tokio::time::sleep(Duration::from_secs(12)); tokio::pin!(deadline);
                    loop {
                        tokio::select! {
                            biased;
                            _ = shutdown.cancelled() => break,
                            _ = cancelled.changed() => break,
                            _ = &mut deadline => break,
                            event = monitor.recv_async() => {
                                match event { Ok(DaemonEvent::Error(_)) | Err(_) => return Err("device_discovery_failed"), _ => {} }
                            },
                            event = events.recv_async() => {
                                match event {
                                    Ok(ServiceEvent::ServiceResolved(info)) => this.resolved(&info),
                                    Ok(ServiceEvent::ServiceRemoved(_, fullname)) => { this.state().candidates.remove(&fullname); },
                                    Err(_) => return Err("device_discovery_failed"),
                                    _ => {},
                                }
                            },
                        }
                    } Ok(())
                }.await;
                let _ = daemon.stop_browse(DEVICE_SERVICE_TYPE);
                this.cleanup_daemon().await?;
                result
            }.await;
            let restart = this.finish_discovery(result);
            if let Some(epoch) = restart && !shutdown.is_cancelled() { let _ = this.discover_for(Some(epoch)); }
        }) {
            self.finish_discovery(Err(error)); return Err(error);
        } Ok(())
    }

    /// 旧轮收尾只处理发现错误，快速重入的请求在确认旧daemon终态后串行启动。
    pub(super) fn finish_discovery(&self, result: Result<(), &'static str>) -> Option<u64> {
        let mut state = self.state(); state.view.scanning = false; state.scan_cancel = None;
        if let Err(error) = result { state.view.discovery_error = Some(error); state.candidates.clear(); }
        std::mem::take(&mut state.restart_scan).then_some(state.scan_epoch)
    }

    /// 命令拥塞时有界重试；超时不释放句柄，后续发现或正式退出继续承担回收责任。
    pub(in crate::usage_service) async fn cleanup_daemon(&self) -> Result<(), &'static str> {
        let daemon = self.daemon.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let Some(daemon) = daemon else { return Ok(()); };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let stopped = match daemon.shutdown() {
                Ok(receiver) => matches!(tokio::time::timeout_at(deadline, receiver.recv_async()).await, Ok(Ok(DaemonStatus::Shutdown))),
                Err(mdns_sd::Error::DaemonShutdown) => true,
                Err(_) => false,
            };
            if stopped {
                self.daemon.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take(); return Ok(());
            }
            if tokio::time::Instant::now() >= deadline { return Err("device_discovery_failed"); }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// 校验后才替换同一服务实例的广告，本地30秒租约限制浏览结束后的可用期。
    fn resolved(&self, info: &ResolvedService) {
        let Some(device) = candidate(info) else { return; };
        let mut state = self.state();
        if state.scan_cancel.as_ref().is_none_or(|cancel| *cancel.borrow()) { return; }
        if state.candidates.len() >= MAX_DISCOVERED_DEVICES && !state.candidates.contains_key(info.get_fullname()) { return; }
        state.candidates.insert(info.get_fullname().into(), (device, Instant::now() + Duration::from_secs(30)));
    }
}

impl Drop for Devices {
    /// 异常销毁仍发送关闭请求；正常路径在应用退出时已经确认Shutdown。
    fn drop(&mut self) {
        if let Some(daemon) = self.daemon.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner).take()
            && daemon.shutdown().is_err() {
                tracing::warn!(event = "device_mdns_shutdown_unconfirmed");
            }
    }
}

/// 广告只采用固定TXT字段与已解析IP；不跟随广告中的URL、主机名或重定向。
pub(super) fn candidate(info: &ResolvedService) -> Option<DiscoveredDevice> {
    if info.ty_domain != DEVICE_SERVICE_TYPE || info.get_property_val_str("v") != Some("1") { return None; }
    let id = info.get_property_val_str("id")?.to_owned();
    let name = info.get_property_val_str("name")?.to_owned();
    let endpoints = info.get_addresses().iter().filter_map(|ip| match ip {
        ScopedIp::V4(value) => Some(SocketAddr::new((*value.addr()).into(), info.get_port())),
        ScopedIp::V6(value) => Some(SocketAddr::V6(SocketAddrV6::new(*value.addr(), info.get_port(), 0, value.scope_id().index))),
        _ => None,
    }).collect();
    DiscoveredDevice::new(id, name, endpoints).ok().map(|device| device.with_pairing_protocol(info.get_property_val_str("pair")))
}
