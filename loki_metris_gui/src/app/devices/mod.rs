//! 设备页只持有展示快照和临时配对草稿，真实配对与保存属于应用服务。

mod dialog;
mod view;
mod paired_card;
mod artwork;
#[cfg(test)]
mod tests;

use super::Shell;
use crate::usage_service::DeviceView;
use gpui_kit::Context;

/// 设备页会话跨页面保留列表，配对码草稿离开模态即丢弃。
#[derive(Default)]
pub(super) struct DevicesView {
    snapshot: DeviceView,
    error: Option<&'static str>,
    modal: Option<dialog::DeviceDialog>,
}

impl Shell {
    /// 模态只接受真实客户端请求；普通mDNS广告不触发配对。
    fn device_pairing_error(&self, id: &str) -> Option<&'static str> {
        match self.devices.snapshot.requests.iter().find(|request| request.device.id == id) {
            None => Some("device_request_expired"),
            Some(request) if !request.device.pairing_available => Some("device_pairing_unavailable"),
            _ => None,
        }
    }
    /// 模态状态由设备模块拥有，不向壳层泄漏私有草稿类型。
    pub(in crate::app) fn device_modal_open(&self) -> bool { self.devices.modal.is_some() }
    /// 进入页面只读取保存列表并确保服务运行，不再发起旧终端浏览。
    pub(in crate::app) fn enter_devices(&mut self, cx: &mut Context<Self>) {
        self.devices.error = None;
        let Some(service) = self.dashboard.service.as_ref() else {
            self.devices.error = Some("device_unavailable"); cx.notify(); return;
        };
        if !service.devices().pending {
            if let Err(error) = service.load_devices() { self.devices.error = Some(error); }
        }
        if let Err(error) = service.start_device_service() { self.devices.error = Some(error); }
        self.refresh_devices(cx);
    }

    /// 复用主窗已有计时器读纯内存投影，不在render中访问磁盘或网络。
    pub(in crate::app) fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        if let Some(service) = self.dashboard.service.as_ref() {
            let snapshot = service.devices();
            if snapshot != self.devices.snapshot {
                let incoming = snapshot.requests.iter().any(|request| !self.devices.snapshot.requests.contains(request));
                self.devices.snapshot = snapshot;
                if incoming && !self.window_visible {
                    let handle = self.native_window;
                    cx.defer(move |cx| crate::native::show(handle, cx));
                }
                cx.notify();
            }
        }
    }

    /// 请求全局串行展示，其他模态优先；过期或断开的请求丢弃草稿并恢复焦点。
    pub(in crate::app) fn sync_device_requests(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.devices.modal.as_ref().is_some_and(|modal| modal.submitted)
            && !self.devices.snapshot.pending && self.devices.snapshot.completed == Some("paired") {
            self.close_device_dialog(window, cx);
        }
        if self.devices.modal.as_ref().is_some_and(|modal| !modal.submitted && modal.request.as_ref().is_some_and(|request|
            !self.devices.snapshot.requests.iter().any(|current| current.ticket == request.ticket && current.device.id == request.device.id))) {
            self.close_device_dialog(window, cx);
        }
        if self.devices.modal.is_some() || self.devices.snapshot.pending || !self.devices.snapshot.loaded
            || self.release_notes_modal.is_some() || self.skin_modal_open() || self.source_modal_open() { return; }
        if let Some(request) = self.devices.snapshot.requests.first().cloned() {
            self.open_device_dialog(request.device.id, request.device.name, false, window, cx);
        }
    }

    /// 页面错误只显示固定本地文案，绝不拼接任意网络响应或路径。
    fn device_error_text(&self, error: &'static str) -> String {
        let suffix = match error {
            "device_code_invalid" => "codeInvalid",
            "device_pairing_failed" => "pairingFailed",
            "device_pairing_unavailable" => "pairingUnavailable",
            "device_timeout" => "timeout",
            "device_connection_failed" => "connectionFailed",
            "device_discovery_failed" => "discoveryFailed",
            "device_storage_corrupt" => "storageCorrupt",
            "device_storage_permissions" => "storagePermissions",
            "device_storage_failed" => "storageFailed",
            "device_not_discovered" => "notDiscovered",
            "device_request_expired" => "requestExpired",
            "device_already_saved" => "alreadySaved",
            "device_limit" => "limit",
            "device_cancelled" => "cancelled",
            "device_busy" => "busy",
            "device_protocol_invalid" => "protocolInvalid",
            _ => "unavailable",
        };
        self.t(&format!("devices.errors.{suffix}"))
    }
}
