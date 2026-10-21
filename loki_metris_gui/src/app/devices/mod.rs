//! 设备页只持有展示快照和临时配对草稿，真实配对与保存属于应用服务。

mod dialog;
mod view;
mod paired_card;
mod discovered;
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
    /// 模态只按当前发现快照展示可操作性，最终发起资格仍由服务重新校验。
    fn device_pairing_error(&self, id: &str) -> Option<&'static str> {
        match self.devices.snapshot.discovered.iter().find(|device| device.id == id) {
            None => Some("device_not_discovered"),
            Some(device) if !device.pairing_available => Some("device_pairing_unavailable"),
            _ => None,
        }
    }
    /// 模态状态由设备模块拥有，不向壳层泄漏私有草稿类型。
    pub(in crate::app) fn device_modal_open(&self) -> bool { self.devices.modal.is_some() }
    /// 进入页面才读取账本和发起一次发现；启动或其它页面不联网。
    pub(in crate::app) fn enter_devices(&mut self, cx: &mut Context<Self>) {
        self.devices.error = None;
        let Some(service) = self.dashboard.service.as_ref() else {
            self.devices.error = Some("device_unavailable"); cx.notify(); return;
        };
        if !service.devices().pending {
            if let Err(error) = service.load_devices() { self.devices.error = Some(error); }
        }
        if let Err(error) = service.discover_devices() { self.devices.error = Some(error); }
        self.refresh_devices(cx);
    }

    /// 复用主窗已有计时器读纯内存投影，不在render中访问磁盘或网络。
    pub(in crate::app) fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        if let Some(service) = self.dashboard.service.as_ref() {
            let snapshot = service.devices();
            if snapshot != self.devices.snapshot { self.devices.snapshot = snapshot; cx.notify(); }
        }
    }

    /// 离页撤销未完成握手与短时发现，已接受的原子保存由服务继续收尾。
    pub(in crate::app) fn leave_devices(&mut self) {
        self.devices.modal = None;
        if let Some(service) = self.dashboard.service.as_ref() { service.cancel_device_activity(); }
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
