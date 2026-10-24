//! 恢复操作由同一设备owner串行持有；GUI只拿恢复码或无秘密预览及一次性票据。

use super::{Devices, UsageService};
use loki_metris_core::devices::{DeviceBook, recovery::RecoverySummary};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::oneshot;

/// 精确预览票据只授予本次用户确认，不是设备认证凭据。
pub(crate) struct DeviceRecoveryPreview {
    pub ticket: String,
    pub summary: RecoverySummary,
}

impl Devices {
    /// Bonjour与设置页消费同一正式/隔离实例名称。
    pub(super) fn instance_name(&self, book: &DeviceBook) -> String {
        loki_metris_core::devices::recovery::desktop_instance_name(book.client_id(), self.isolated)
    }

    /// 用户已选文件后才在后台导出；恢复码从不进入设备快照或日志。
    fn export_recovery(self: &Arc<Self>, destination: PathBuf) -> Result<oneshot::Receiver<Result<String, &'static str>>, &'static str> {
        self.begin()?; let this = self.clone(); let (tx, rx) = oneshot::channel();
        if let Err(error) = self.owner.spawn_blocking("device-recovery-export", move |_| {
            let result = this.store.export_recovery(&destination);
            this.finish(this.store.load(), None);
            let _ = tx.send(result);
        }) { self.finish(Err(error), None); return Err(error); }
        Ok(rx)
    }

    /// 解密与预检不改变信任；原材料只在服务内等待一次明确确认。
    fn prepare_recovery(self: &Arc<Self>, source: PathBuf, code: String) -> Result<oneshot::Receiver<Result<DeviceRecoveryPreview, &'static str>>, &'static str> {
        self.begin()?; let this = self.clone(); let (tx, rx) = oneshot::channel();
        if let Err(error) = self.owner.spawn_blocking("device-recovery-preview", move |_| {
            let result = this.store.prepare_recovery(&source, &code);
            this.finish(this.store.load(), None);
            let result = result.map(|plan| {
                let ticket = uuid::Uuid::new_v4().to_string();
                let preview = DeviceRecoveryPreview { ticket: ticket.clone(), summary: plan.summary() };
                this.state().recovery = Some((ticket, plan)); preview
            });
            let _ = tx.send(result);
        }) { self.finish(Err(error), None); return Err(error); }
        Ok(rx)
    }

    /// 用户确认精确预览后原子恢复；旧监听和所有租约撤销，再广播恢复后的UUID。
    fn commit_recovery(self: &Arc<Self>, ticket: String) -> Result<oneshot::Receiver<Result<(), &'static str>>, &'static str> {
        self.begin()?;
        let plan = {
            let mut state = self.state();
            if state.recovery.as_ref().is_none_or(|(current, _)| current != &ticket) {
                drop(state); self.finish(Err("device_recovery_conflict"), None); return Err("device_recovery_conflict");
            }
            state.recovery.take().map(|(_, plan)| plan).expect("validated recovery ticket")
        };
        let this = self.clone(); let (tx, rx) = oneshot::channel();
        if let Err(error) = self.owner.spawn_blocking("device-recovery-commit", move |_| {
            let result = this.store.commit_recovery(plan);
            let success = result.is_ok();
            if success {
                let mut state = this.state();
                state.server.offers.clear(); state.server.widget_nonces.clear();
                for (_, session) in std::mem::take(&mut state.server.sessions) { let _ = session.cancel.send(true); }
                state.presence = Default::default();
                if let Some(cancel) = state.server.cancel.as_ref() {
                    let _ = cancel.send(true); state.server.restart = true;
                }
            }
            let reply = result.as_ref().map(|_| ()).map_err(|error| *error);
            this.finish(result, Some("restored"));
            if success { let _ = this.start_server(); }
            let _ = tx.send(reply);
        }) { self.finish(Err(error), None); return Err(error); }
        Ok(rx)
    }
}

impl UsageService {
    /// 导出仅处理调用者通过原生选择器指定的新文件。
    pub(crate) fn export_device_recovery(&self, destination: PathBuf) -> Result<oneshot::Receiver<Result<String, &'static str>>, &'static str> {
        self.context.devices.export_recovery(destination)
    }
    /// 预览仅返回无秘密身份和设备列表，不会授予网络访问或覆盖账本。
    pub(crate) fn prepare_device_recovery(&self, source: PathBuf, code: String) -> Result<oneshot::Receiver<Result<DeviceRecoveryPreview, &'static str>>, &'static str> {
        self.context.devices.prepare_recovery(source, code)
    }
    /// 恢复必须消费用户刚刚核对的服务端票据，不能传公开UUID替代。
    pub(crate) fn commit_device_recovery(&self, ticket: String) -> Result<oneshot::Receiver<Result<(), &'static str>>, &'static str> {
        self.context.devices.commit_recovery(ticket)
    }
    /// 取消只清除相同预览，旧弹窗不能清除下一次预览。
    pub(crate) fn discard_device_recovery(&self, ticket: &str) {
        let mut state = self.context.devices.state();
        if state.recovery.as_ref().is_some_and(|(current, _)| current == ticket) { state.recovery = None; }
    }
}
