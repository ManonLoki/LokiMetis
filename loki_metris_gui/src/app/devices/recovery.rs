//! 选择文件、单独保存恢复码与两步确认只持有交互草稿，账本留在服务owner。

use super::Shell;
use gpui_kit::{AppContext, Context, Entity, FocusHandle, Focusable, Subscription, Window, component::input::{InputEvent, InputState}};
use std::{path::PathBuf, time::Duration};

/// 恢复码仅暂存在当前弹窗，关闭即丢弃；预览不再保留输入码。
pub(super) enum RecoveryKind {
    Import(PathBuf),
    Preview(crate::usage_service::DeviceRecoveryPreview),
    Exported(String),
}

/// 弹窗复用原生焦点陷阱，输入实体只在选择包后创建一次。
pub(super) struct RecoveryDialog {
    pub(super) kind: RecoveryKind,
    pub(super) input: Option<Entity<InputState>>,
    pub(super) focus: FocusHandle,
    previous: Option<FocusHandle>,
    _subscription: Option<Subscription>,
}

impl Shell {
    /// 用户选定新文件后导出；取消选择器不生成恢复包或恢复码。
    pub(super) fn choose_device_recovery_export(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.device_modal_open() || self.devices.recovery_busy || self.devices.snapshot.pending { return; }
        self.devices.recovery_busy = true; self.devices.error = None;
        let picker = cx.prompt_for_new_path(&PathBuf::from("."), Some("LokiMetris-pairing.lmr"));
        self.devices.recovery_task = Some(cx.spawn_in(window, async move |entity, cx| {
            let selected = picker.await;
            let work = entity.update_in(cx, |this, _, cx| {
                let path = match selected {
                    Ok(Ok(Some(path))) => path,
                    Ok(Ok(None)) => { this.devices.recovery_busy = false; cx.notify(); return None; },
                    _ => { this.devices.recovery_busy = false; this.devices.error = Some("device_recovery_file_failed"); cx.notify(); return None; },
                };
                match this.dashboard.service.as_ref().ok_or("device_unavailable").and_then(|service| service.export_device_recovery(path)) {
                    Ok(work) => Some(work),
                    Err(error) => { this.devices.recovery_busy = false; this.devices.error = Some(error); cx.notify(); None },
                }
            }).ok().flatten();
            let Some(work) = work else { return; };
            let result = crate::usage_service::poll_receipt(work, cx.background_executor().clone(), Duration::from_millis(30)).await;
            let _ = entity.update_in(cx, |this, window, cx| {
                this.devices.recovery_busy = false;
                match result.unwrap_or(Err("device_recovery_file_failed")) {
                    Ok(code) => this.open_device_recovery(RecoveryKind::Exported(code), window, cx),
                    Err(error) => this.devices.error = Some(error),
                }
                this.refresh_devices(cx); cx.notify();
            });
        })); cx.notify();
    }

    /// 文件选择只指定读取对象，输入恢复码后仍须先预览再确认。
    pub(super) fn choose_device_recovery_import(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.device_modal_open() || self.devices.recovery_busy || self.devices.snapshot.pending { return; }
        self.devices.recovery_busy = true; self.devices.error = None;
        let picker = cx.prompt_for_paths(gpui_kit::PathPromptOptions { files: true, directories: false, multiple: false,
            prompt: Some(self.t("devices.recovery.import").into()) });
        self.devices.recovery_task = Some(cx.spawn_in(window, async move |entity, cx| {
            let selected = picker.await;
            let _ = entity.update_in(cx, |this, window, cx| {
                this.devices.recovery_busy = false;
                match selected {
                    Ok(Ok(Some(paths))) if paths.len() == 1 => this.open_device_recovery(RecoveryKind::Import(paths[0].clone()), window, cx),
                    Ok(Ok(_)) => {},
                    _ => this.devices.error = Some("device_recovery_file_failed"),
                }
                cx.notify();
            });
        })); cx.notify();
    }

    /// 导入框显示所选文件名，恢复码输入采用原生遮盖，不进入辅助树值或设备投影。
    pub(super) fn open_device_recovery(&mut self, kind: RecoveryKind, window: &mut Window, cx: &mut Context<Self>) {
        let previous = self.devices.recovery.as_ref().and_then(|modal| modal.previous.clone()).or_else(|| window.focused(cx));
        let focus = cx.focus_handle();
        let input = matches!(kind, RecoveryKind::Import(_)).then(|| cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("LMR1-…")
                .validate(|text, _| text.len() <= 64 && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_ \r\n\t".contains(&byte)));
            input.set_masked(true, window, cx); input
        }));
        let subscription = input.as_ref().map(|input| cx.subscribe(input, |_, _, event, cx| { if matches!(event, InputEvent::Change) { cx.notify(); } }));
        if let Some(input) = &input { window.focus(&input.focus_handle(cx), cx); } else { window.focus(&focus, cx); }
        self.devices.recovery = Some(RecoveryDialog { kind, input, focus, previous, _subscription: subscription });
        self.devices.error = None; cx.notify();
    }

    /// 关闭丢弃精确预览与草稿；接受中的文件提交保留到真实回执，避免假取消。
    pub(super) fn close_device_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.devices.recovery_busy { return; }
        if let Some(modal) = self.devices.recovery.take() {
            if let RecoveryKind::Preview(preview) = modal.kind && let Some(service) = &self.dashboard.service { service.discard_device_recovery(&preview.ticket); }
            if let Some(previous) = modal.previous { window.focus(&previous, cx); }
        }
        self.devices.error = None; cx.notify();
    }

    /// 第一次确认只解密预览；只有第二次用户确认才消费票据提交原账本身份。
    pub(super) fn confirm_device_recovery(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.devices.recovery_busy { return; }
        let Some(modal) = &self.devices.recovery else { return; };
        let Some(service) = self.dashboard.service.as_ref() else { self.devices.error = Some("device_unavailable"); cx.notify(); return; };
        match &modal.kind {
            RecoveryKind::Import(path) => {
                let code = modal.input.as_ref().map(|input| input.read(cx).value().to_string()).unwrap_or_default();
                let work = match service.prepare_device_recovery(path.clone(), code) {
                    Ok(work) => work, Err(error) => { self.devices.error = Some(error); cx.notify(); return; },
                };
                self.devices.recovery_busy = true; self.devices.error = None;
                self.devices.recovery_task = Some(cx.spawn_in(window, async move |entity, cx| {
                    let result = crate::usage_service::poll_receipt(work, cx.background_executor().clone(), Duration::from_millis(30)).await;
                    let _ = entity.update_in(cx, |this, window, cx| {
                        this.devices.recovery_busy = false;
                        match result.unwrap_or(Err("device_recovery_file_failed")) {
                            Ok(preview) => this.open_device_recovery(RecoveryKind::Preview(preview), window, cx),
                            Err(error) => this.devices.error = Some(error),
                        }
                        cx.notify();
                    });
                }));
            },
            RecoveryKind::Preview(preview) => {
                let work = match service.commit_device_recovery(preview.ticket.clone()) {
                    Ok(work) => work, Err(error) => { self.devices.error = Some(error); cx.notify(); return; },
                };
                self.devices.recovery_busy = true; self.devices.error = None;
                self.devices.recovery_task = Some(cx.spawn_in(window, async move |entity, cx| {
                    let result = crate::usage_service::poll_receipt(work, cx.background_executor().clone(), Duration::from_millis(30)).await;
                    let _ = entity.update_in(cx, |this, window, cx| {
                        this.devices.recovery_busy = false;
                        match result.unwrap_or(Err("device_recovery_file_failed")) {
                            Ok(()) => this.close_device_recovery(window, cx), Err(error) => this.devices.error = Some(error),
                        }
                        this.refresh_devices(cx); cx.notify();
                    });
                }));
            },
            RecoveryKind::Exported(_) => {},
        }
        cx.notify();
    }
}
