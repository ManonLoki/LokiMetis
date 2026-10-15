//! 换肤目录与原生控件直接消费同一资源服务，不经WebView或前端IPC。

mod actions;
mod host_action;
mod dialogs;
mod import_dialog;
mod imports;
mod state;
mod view;
mod preview;
mod preview_residency;

use super::{Page, Shell};
use crate::skins::{SkinError, SkinService};
use gpui_kit::{
    AppContext, Context, Window,
    component::input::{InputEvent, InputState},
};
use loki_metris_core::{SkinHostKind, SkinReference, public_ai_capabilities};
pub(in crate::app) use state::Skins;
use state::{ModalKind, Outcome, SkinSuccess};
use std::{
    future::Future,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

impl Shell {
    /// 固定公开目录过滤当前统一Agent集合，未知或损坏设置不能猜测宿主。
    fn skin_hosts(&self) -> Vec<SkinHostKind> {
        let Some(settings) = &self.agent_settings.value else {
            return Vec::new();
        };
        public_ai_capabilities()
            .iter()
            .filter(|cap| {
                cap.monitor_tool
                    .is_some_and(|tool| settings.enabled_ai_tools.contains(&tool))
            })
            .filter_map(|cap| cap.skin_host)
            .collect()
    }
    /// 页面离开使尚未开始的宿主副作用失效；已接受文件事务继续收尾。
    pub(in crate::app) fn leave_skins(&mut self) {
        self.skins.authority.fetch_add(1, Ordering::AcqRel);
        self.skins.modal = None;
        self.skins.input_subscriptions.clear();
        self.skins.selected.clear();
        self.skins.import.take();
    }
    /// 搜索实体只创建一次；语言变化原位更新提示，保留输入法与搜索值。
    pub(in crate::app) fn sync_skin_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Skins {
            return;
        }
        let placeholder = self.t("skins.search.placeholder");
        if let Some(input) = &self.skins.search {
            if input.read(cx).presentation().placeholder().as_str() != placeholder.as_str() {
                input.update(cx, |input, cx| input.set_placeholder(placeholder, window, cx));
            }
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        self.skins.search_subscription = Some(cx.subscribe(&input, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        }));
        self.skins.search = Some(input);
    }
    /// 五秒目录刷新同时核对宿主；失败保留已有目录但关闭所有写入口。
    pub(in crate::app) fn refresh_skins(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.page != Page::Skins {
            return;
        }
        let hosts = self.skin_hosts();
        let host = self
            .skins
            .host
            .filter(|host| hosts.contains(host))
            .or_else(|| hosts.first().copied());
        if host != self.skins.host {
            self.skins.host = host;
            self.skins.generation = self.skins.generation.wrapping_add(1);
            self.skins.query.take();
            self.skins.loading = false;
            self.skins.status = None;
            self.skins.instances.clear();
            self.skins.last_read = None;
            self.skins.authority.fetch_add(1, Ordering::AcqRel);
        }
        if self.skins.busy
            || self.skins.loading
            || host.is_none()
            || (!force
                && self
                    .skins
                    .last_read
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(4)))
        {
            return;
        }
        let Some(service) = &self.dashboard.service else {
            return;
        };
        let host = host.unwrap();
        self.skins.loading = true;
        self.skins.generation = self.skins.generation.wrapping_add(1);
        let generation = self.skins.generation;
        let catalog = service.skin_catalog();
        let host_work = service.skin_host_work(move |service| async move {
            let instances = service.scanned_host_instances(host).await?;
            let status = service.status(host).await;
            Ok((instances, status))
        });
        self.skins.query = Some(cx.spawn(async move |entity, cx| {
            let executor = cx.background_executor().clone();
            let catalog = match catalog {
                Ok(work) => crate::usage_service::skin_receipt(work, executor.clone()).await,
                Err(code) => Err(SkinError::new(code, "无法读取皮肤资源库。")),
            };
            let host_result = match host_work {
                Ok(work) => crate::usage_service::skin_receipt(work, executor).await,
                Err(code) => Err(SkinError::new(code, "无法读取宿主状态。")),
            };
            let _ = entity.update(cx, |this, cx| {
                if this.skins.generation != generation || this.skins.host != Some(host) {
                    return;
                }
                this.skins.loading = false;
                this.skins.last_read = Some(std::time::Instant::now());
                match catalog {
                    Ok(catalog) => {
                        this.adopt_skin_catalog(catalog.entries, cx);
                        this.skins.catalog_error = None;
                    }
                    Err(error) => this.skins.catalog_error = Some(error),
                }
                match host_result {
                    Ok((instances, status)) => {
                        this.skins.instances = instances;
                        this.skins.status = Some(status);
                        this.skins.host_error = None;
                    }
                    Err(error) => {
                        this.skins.status = None;
                        this.skins.host_error = Some(error);
                    }
                }
                if let Some(entries) = &this.skins.entries {
                    this.skins.selected.retain(|key| {
                        entries
                            .iter()
                            .any(|entry| skin_reference(&entry.skin) == *key)
                    });
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 每次新副作用必须依赖成功的统一设置、目录及当前宿主快照。
    fn skin_writable(&self) -> bool {
        !self.skins.busy
            && !self.skins.picking
            && self.agent_settings.value.is_some()
            && self.agent_settings.error.is_none()
            && self.skins.entries.is_some()
            && self.skins.catalog_error.is_none()
            && self.skins.host_error.is_none()
            && self.skins.status.is_some()
            && self
                .skins
                .host
                .is_some_and(|host| self.skin_hosts().contains(&host))
    }
    /// 当前活跃项优先唯一实例报告，比较来源与ID的完整身份。
    fn active_skin(&self) -> Option<SkinReference> {
        if self.skins.instances.len() == 1
            && let Some(reference) = &self.skins.instances[0].active_skin {
                return Some(reference.clone());
            }
        self.skins
            .status
            .as_ref()
            .filter(|status| status.installed)
            .and_then(|status| {
                Some(SkinReference {
                    source: status.source?,
                    id: status.skin_id.clone()?,
                })
            })
    }
    /// 任何已接受写入都有服务owner；界面只等待并显示实际结果。
    fn skin_mutation<F, Fut>(&mut self, work: F, cx: &mut Context<Self>)
    where
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Outcome, SkinError>> + Send + 'static,
    {
        self.skin_mutation_kind(work, false, None, cx);
    }
    /// 宿主操作额外启用CDP退出责任，资源库操作不关闭进程级gate。
    fn skin_host_mutation<F, Fut>(&mut self, work: F, cx: &mut Context<Self>)
    where
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Outcome, SkinError>> + Send + 'static,
    {
        self.skin_mutation_kind(work, true, None, cx);
    }
    /// 更换皮肤绑定真实宿主终态通知，资源操作和中间确认保持各自语义。
    fn skin_apply_mutation<F, Fut>(&mut self, host: loki_metris_core::SkinHostKind, work: F, cx: &mut Context<Self>)
    where F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Outcome, SkinError>> + Send + 'static,
    {
        self.skin_mutation_kind(work, true, Some(host), cx);
    }
    /// 同一串行UI入口按副作用种类登记应用owner。
    fn skin_mutation_kind<F, Fut>(&mut self, work: F, host: bool, notification_host: Option<loki_metris_core::SkinHostKind>, cx: &mut Context<Self>)
    where
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Outcome, SkinError>> + Send + 'static,
    {
        if !self.skin_writable() {
            return;
        }
        let epoch = self.skins.authority.load(Ordering::Acquire);
        let Some(service) = &self.dashboard.service else {
            return;
        };
        let notifications = service.activity_notifications();
        let completed = notifications.clone();
        let work = match if host {
            let authority = self.skins.authority.clone();
            service.skin_host_work_observed(move |service| async move {
                crate::skins::with_host_authority(
                    Arc::new(move || authority.load(Ordering::Acquire) == epoch),
                    async move { work(service).await },
                ).await
            }, move |result| {
                if let (Some(host), Some(success)) = (notification_host, host_action::notification_result(result)) {
                    completed.push(crate::notification_events::Activity::Skin { host, success });
                }
            })
        } else {
            service.skin_work(work)
        } {
            Ok(work) => work,
            Err(code) => {
                if let Some(host) = notification_host {
                    notifications.push(crate::notification_events::Activity::Skin { host, success: false });
                }
                self.skins.feedback = Some(Err(SkinError::new(code, "无法提交皮肤操作。")));
                cx.notify();
                return;
            }
        };
        self.skins.generation = self.skins.generation.wrapping_add(1);
        self.skins.query.take();
        self.skins.loading = false;
        self.skins.busy = true;
        self.skins.feedback = None;
        self.skins.mutation = Some(cx.spawn(async move |entity, cx| {
            let result =
                crate::usage_service::skin_receipt(work, cx.background_executor().clone()).await;
            let _ = entity.update(cx, |this, cx| {
                this.skins.busy = false;
                match result {
                    Ok(outcome) => {
                        if !host || this.skins.authority.load(Ordering::Acquire) == epoch {
                            this.accept_skin_outcome(outcome, cx);
                        }
                    }
                    Err(error) => this.skins.feedback = Some(Err(error)),
                }
                this.refresh_skins(true, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 通知只引用成功事务的实际资源名称与数量。
    fn accept_skin_outcome(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        let success = match outcome {
            Outcome::Created(skin) => SkinSuccess::Created { name: skin.name },
            Outcome::Converted(result) => SkinSuccess::Converted { name: result.theme.name },
            Outcome::Deleted(result) => {
                self.skins.selected.clear();
                if let Some(error) = result.failed.first() {
                    self.skins.feedback =
                        Some(Err(SkinError::new(error.code, error.message.clone())));
                    return;
                }
                SkinSuccess::Deleted { count: result.deleted.len() }
            }
            Outcome::Prompt(prompt) => {
                self.skins.prompt = Some(prompt);
                self.skins.copied = false;
                cx.notify();
                return;
            }
            Outcome::Opened => return,
            Outcome::Exported => SkinSuccess::Exported,
            Outcome::Stopped => SkinSuccess::Stopped,
            Outcome::Applied(crate::skins::InstallSkinResult::Installed { .. }, intent) => {
                SkinSuccess::Applied {
                    name: self
                        .skins
                        .entries
                        .as_ref()
                        .and_then(|entries| {
                            entries
                                .iter()
                                .find(|entry| skin_reference(&entry.skin) == intent.skin)
                        })
                        .map(|entry| entry.skin.name.clone())
                        .unwrap_or(intent.skin.id),
                }
            }
            Outcome::Applied(
                crate::skins::InstallSkinResult::NeedsConfirmation { check },
                intent,
            ) => {
                self.defer_skin_dialog(ModalKind::Appearance(check, intent), cx);
                return;
            }
            Outcome::NeedsRestart(intent, mode) => {
                self.defer_skin_dialog(ModalKind::Restart(intent, mode), cx);
                return;
            }
        };
        if matches!(
            self.skins.modal.as_ref().map(|modal| &modal.kind),
            Some(ModalKind::Create(_) | ModalKind::Convert(_) | ModalKind::Delete(_))
        ) {
            self.defer_close_skin_dialog(cx);
        }
        self.skins.feedback = Some(Ok(success));
    }
}

/// 联合身份贯穿卡片、批量选择与文件操作。
fn skin_reference(skin: &crate::skins::SkinDescriptor) -> SkinReference {
    SkinReference {
        source: skin.source,
        id: skin.id.clone(),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod preview_tests;
#[cfg(test)]
mod dialog_tests;
