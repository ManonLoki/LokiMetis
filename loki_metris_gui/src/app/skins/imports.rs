//! ZIP选择、预检进度与确认提交，token的生命周期由应用级租约负责。

use super::{ModalKind, Shell, SkinError, SkinSuccess, state::ImportDraft};
use gpui_kit::{Context, Window};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

impl Shell {
    /// 在原生选择器前预约入口，连续点击不会创建重叠选择器。
    pub(super) fn choose_skin_import(&mut self, cx: &mut Context<Self>) {
        if !self.skin_writable() || self.skins.modal.is_some() {
            return;
        }
        self.skins.picking = true;
        let authority = self.skins.authority.clone();
        let epoch = authority.load(Ordering::Acquire);
        let picker = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.t("skins.action.import").into()),
        });
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result = picker.await;
            let _ = entity.update(cx, |this, cx| {
                this.skins.picking = false;
                if authority.load(Ordering::Acquire) != epoch {
                    cx.notify();
                    return;
                }
                this.restore_skin_picker_focus(cx);
                match result {
                    Ok(Ok(Some(paths))) if !paths.is_empty() => this.prepare_skin_import(paths, cx),
                    Ok(Ok(_)) => {}
                    _ => {
                        this.skins.feedback = Some(Err(SkinError::new(
                            "skin.import_failed",
                            "无法打开皮肤文件选择器。",
                        )))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 拖放与文件选择共用一个预检入口，后台逐项报告真实完成数量。
    pub(super) fn prepare_skin_import(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.skin_writable() || self.skins.modal.is_some() || self.skins.import.is_some() {
            return;
        }
        let Some(service) = &self.dashboard.service else {
            return;
        };
        let (lease, mut work) = match service.prepare_skin_import(paths) {
            Ok(value) => value,
            Err(code) => {
                self.skins.feedback = Some(Err(SkinError::new(code, "无法开始皮肤预检。")));
                cx.notify();
                return;
            }
        };
        let session = lease.session.clone();
        self.skins.import = Some(ImportDraft {
            lease,
            batch: None,
            selected: Vec::new(),
            trusted: false,
        });
        self.skins.busy = true;
        self.skins.feedback = None;
        self.skins.generation = self.skins.generation.wrapping_add(1);
        self.skins.query.take();
        self.skins.loading = false;
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result = loop {
                match work.try_recv() {
                    Ok(result) => break result,
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                        break Err(SkinError::new(
                            "skin.operation_unavailable",
                            "导入预检回执不可用。",
                        ));
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        let _ = entity.update(cx, |_, cx| cx.notify());
                        cx.background_executor()
                            .timer(Duration::from_millis(30))
                            .await;
                    }
                }
            };
            let _ = entity.update(cx, |this, cx| {
                this.skins.busy = false;
                let current = this
                    .skins
                    .import
                    .as_mut()
                    .filter(|draft| Arc::ptr_eq(&draft.lease.session, &session));
                if let Some(draft) = current {
                    match result {
                        Ok(batch) => {
                            draft.selected = batch
                                .items
                                .iter()
                                .map(|item| item.item_id.clone())
                                .collect();
                            draft.batch = Some(batch);
                            this.defer_skin_dialog(ModalKind::Import, cx);
                        }
                        Err(error) => {
                            this.skins.import.take();
                            this.skins.feedback = Some(Err(error));
                        }
                    }
                }
                this.refresh_skins(true, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 只改变所选Legacy集合时重置信任，选择纯主题不会丢已有确认。
    pub(super) fn select_skin_import(
        &mut self,
        item_id: &str,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if self.skins.busy {
            return;
        }
        let Some(draft) = &mut self.skins.import else {
            return;
        };
        let legacy = draft.batch.as_ref().is_some_and(|batch| {
            batch.items.iter().any(|item| {
                item.item_id == item_id
                    && item.skin.package_type == loki_metris_core::SkinPackageType::LegacySkin
            })
        });
        draft.selected.retain(|id| id != item_id);
        if selected {
            draft.selected.push(item_id.to_owned());
        }
        if legacy {
            draft.trusted = false;
        }
        cx.notify();
    }
    /// 取消失败保留原确认窗与租约，用户能够直接重试。
    pub(super) fn cancel_skin_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.skins.busy {
            return;
        }
        let Some(draft) = &self.skins.import else {
            self.close_skin_dialog_now(window, cx);
            return;
        };
        let session = draft.lease.session.clone();
        let Some(service) = &self.dashboard.service else {
            return;
        };
        let work = match service.skin_work(move |service| async move { session.cancel(&service) }) {
            Ok(work) => work,
            Err(code) => {
                self.skins.feedback = Some(Err(SkinError::new(code, "无法取消皮肤导入。")));
                cx.notify();
                return;
            }
        };
        self.skins.busy = true;
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result =
                crate::usage_service::skin_receipt(work, cx.background_executor().clone()).await;
            let _ = entity.update(cx, |this, cx| {
                this.skins.busy = false;
                match result {
                    Ok(()) => {
                        this.skins.import.take();
                        this.defer_close_skin_dialog(cx);
                    }
                    Err(error) => this.skins.feedback = Some(Err(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 提交只使用当前批次、真实选择与本次信任勾选；成功终态释放租约。
    pub(super) fn commit_skin_import(&mut self, cx: &mut Context<Self>) {
        if !self.skin_writable() {
            return;
        }
        let Some(draft) = &self.skins.import else {
            return;
        };
        let Some(batch) = &draft.batch else {
            return;
        };
        let trust_required = batch.items.iter().any(|item| {
            draft.selected.contains(&item.item_id)
                && item.skin.package_type == loki_metris_core::SkinPackageType::LegacySkin
        });
        if draft.selected.is_empty() || (trust_required && !draft.trusted) {
            return;
        }
        let session = draft.lease.session.clone();
        let token = match session.reserve_commit() {
            Ok(token) => token,
            Err(error) => {
                self.skins.feedback = Some(Err(error));
                cx.notify();
                return;
            }
        };
        let Some(service) = &self.dashboard.service else {
            return;
        };
        let work = match service.commit_skin_import(
            session,
            token,
            draft.selected.clone(),
            trust_required && draft.trusted,
        ) {
            Ok(work) => work,
            Err(code) => {
                self.skins.feedback = Some(Err(SkinError::new(code, "无法提交皮肤导入。")));
                cx.notify();
                return;
            }
        };
        self.skins.busy = true;
        self.skins.feedback = None;
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result =
                crate::usage_service::skin_receipt(work, cx.background_executor().clone()).await;
            let _ = entity.update(cx, |this, cx| {
                this.skins.busy = false;
                match result {
                    Ok(result) => {
                        this.skins.import.take();
                        this.defer_close_skin_dialog(cx);
                        this.skins.feedback = Some(match result.failed.first() {
                            Some(error) => Err(SkinError::new(error.code, error.message.clone())),
                            None => Ok(SkinSuccess::Imported { count: result.installed.len() }),
                        });
                    }
                    Err(error) => this.skins.feedback = Some(Err(error)),
                }
                this.refresh_skins(true, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
}
