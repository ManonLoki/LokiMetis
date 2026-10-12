//! 图库与档案的真实异步操作，查询代数防止旧快照覆盖已提交修改。

use super::{
    Shell,
    state::{CatalogSuccess, Category, Route},
};
use gpui_kit::{
    Context, Window,
    component::input::{InputEvent, TextareaState},
    prelude::*,
};
use loki_metris_core::{AiTool, HookBehavior, HookError, public_monitor_ai_tools};

impl Shell {
    /// 每次壳层投影登记本帧使用位置，未挂载图片的计时器必须暂停。
    pub(in crate::app) fn begin_monitor_image_frame(&self) {
        let epoch = &self.monitor.catalog.frame_epoch;
        epoch.set(epoch.get().wrapping_add(1));
    }
    /// 原生隐藏可能停止绘帧，不能等待下一次 prepaint 才归还像素与图集。
    pub(in crate::app) fn release_monitor_images(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for image in self.monitor.catalog.retired_images.drain(..) {
            super::images::release_image(&image, window, cx);
        }
        for image in &self.monitor.catalog.images {
            for (_, visible) in image.residency.placements.borrow_mut().values_mut() {
                *visible = false;
            }
            super::images::release_image(image, window, cx);
        }
    }
    /// 保持已有 Entity 身份和播放位置，页面离开不遗留动画任务。
    pub(in crate::app) fn finish_monitor_image_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let epoch = self.monitor.catalog.frame_epoch.get();
        for image in self.monitor.catalog.retired_images.drain(..) {
            super::images::release_image(&image, window, cx);
        }
        for image in &self.monitor.catalog.images {
            image
                .residency
                .placements
                .borrow_mut()
                .retain(|_, (seen, _)| *seen == epoch);
            for (seen, view) in image.views.borrow().values() {
                view.update(cx, |view, cx| view.set_mounted(*seen == epoch, window, cx));
            }
            if image.residency.placements.borrow().is_empty() {
                super::images::release_image(image, window, cx);
            }
        }
    }
    /// 显式子页导航加载真实所需数据，已有未提交档案不会被图库刷新覆盖。
    pub(super) fn select_monitor_route(&mut self, route: Route, cx: &mut Context<Self>) {
        if self.monitor.route != route { self.page_scroll.invalidate(); }
        self.monitor.route = route;
        if matches!(route,Route::Management|Route::Images) {
            if !self.monitor.catalog.images_loaded && !self.monitor.catalog.images_loading {
                self.load_monitor_images(cx);
            }
        }
        if route == Route::Management {
            self.load_agent_settings(cx);
            if !self.monitor.catalog.profiles_loaded && !self.monitor.catalog.profiles_loading {
                self.load_monitor_profiles(cx);
            }
        }
        if route==Route::Hooks {
            self.load_agent_settings(cx);
            if !self.monitor.hooks.loaded&&!self.monitor.hooks.loading {self.load_monitor_hook_locations(cx);}
        }
        cx.notify();
    }

    /// 启用集合和已读档案共同决定当前 Agent，不回退到未启用项。
    pub(in crate::app) fn monitor_tool(&self) -> Option<AiTool> {
        let tools = self.monitor_tools();
        self.monitor
            .catalog
            .selected
            .filter(|tool| tools.contains(tool))
            .or_else(|| tools.first().copied())
    }

    /// 显示顺序保持公开 core 目录顺序。
    pub(super) fn monitor_tools(&self) -> Vec<AiTool> {
        let Some(settings) = &self.agent_settings.value else {
            return Vec::new();
        };
        public_monitor_ai_tools()
            .filter(|tool| {
                settings.enabled_ai_tools.contains(tool)
                    && self
                        .monitor
                        .catalog
                        .editors
                        .iter()
                        .any(|editor| editor.draft.tool == *tool)
            })
            .collect()
    }

    /// 文本框只在实际管理页面建立，初始或保存规范化后才重置已提交基线。
    pub(in crate::app) fn sync_monitor_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != crate::app::Page::Monitor || self.monitor.route != Route::Management {
            return;
        }
        let placeholder = self.t("monitor.management.contentPlaceholder");
        for editor in &mut self.monitor.catalog.editors {
            if editor.inputs.is_none() {
                let inputs = std::array::from_fn(|_| {
                    cx.new(|cx| {
                        TextareaState::new(window, cx)
                            .placeholder(placeholder.clone())
                            .auto_grow(2, 2)
                    })
                });
                let tool = editor.draft.tool;
                for (index, input) in inputs.iter().enumerate() {
                    editor.subscriptions.push(cx.subscribe_in(
                        input,
                        window,
                        move |this, input, event, _, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let value = input.read(cx).value().to_string();
                            if let Some(editor) = this.monitor.catalog.editor_mut(tool) {
                                if let Some(hook) = editor.draft.hooks.get_mut(index) {
                                    hook.content = value;
                                }
                            }
                            this.monitor.catalog.changed(tool);
                            cx.notify();
                        },
                    ));
                }
                editor.inputs = Some(inputs);
            }
            // 占位译文不属于草稿；仅变化时更新原实体，避免每帧通知或重置编辑态。
            if let Some(inputs) = &editor.inputs {
                for input in inputs {
                    if input.read(cx).presentation().placeholder().as_str() != placeholder.as_str() {
                        input.update(cx, |input, cx| input.set_placeholder(placeholder.clone(), window, cx));
                    }
                }
            }
            if editor.reset_inputs {
                if let Some(inputs) = &editor.inputs {
                    for (input, hook) in inputs.iter().zip(&editor.draft.hooks) {
                        input.update(cx, |input, cx| {
                            input.set_value(hook.content.clone(), window, cx)
                        });
                    }
                }
                editor.reset_inputs = false;
            }
        }
    }

    /// 档案查询只用于初始化或显式失败重试，不丢弃正常编辑态。
    pub(super) fn load_monitor_profiles(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.monitor.catalog;
        state.profiles_loading = true;
        state.profiles_error = None;
        state.profile_query = state.profile_query.wrapping_add(1);
        let query = state.profile_query;
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.list_monitor_profile_drafts().ok());
        let Some(work) = work else {
            state.profiles_loading = false;
            state.profiles_error = Some(HookError::new("error.monitor.profilesReadFailed"));
            cx.notify();
            return;
        };
        state.profile_task = Some(cx.spawn(async move |entity, cx| {
            let result = receipt(
                work,
                cx.background_executor().clone(),
                "error.monitor.profilesReadFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.catalog;
                if state.profile_query != query {
                    return;
                }
                state.profiles_loading = false;
                match result {
                    Ok(set) => state.accept_profiles(set.drafts),
                    Err(error) => state.profiles_error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 刷新图库保留格式筛选和所有档案草稿。
    pub(super) fn load_monitor_images(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.monitor.catalog;
        if state.uploading || state.deleting.is_some() {
            return;
        }
        state.images_loading = true;
        state.images_error = None;
        state.image_query = state.image_query.wrapping_add(1);
        let query = state.image_query;
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.list_monitor_images().ok());
        let Some(work) = work else {
            state.images_loading = false;
            state.images_error = Some(HookError::new("error.monitor.imagesReadFailed"));
            cx.notify();
            return;
        };
        state.image_task = Some(cx.spawn(async move |entity, cx| {
            let result = receipt(
                work,
                cx.background_executor().clone(),
                "error.monitor.imagesReadFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.catalog;
                if state.image_query != query {
                    return;
                }
                state.images_loading = false;
                match result {
                    Ok(snapshot) => state.accept_images(snapshot),
                    Err(error) => state.images_error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 槽位修改只影响当前草稿，范围来自 core 能力。
    pub(super) fn select_monitor_slot(&mut self, tool: AiTool, slot: u8, cx: &mut Context<Self>) {
        if self.monitor_tool() != Some(tool) {
            return;
        }
        if let Some(editor) = self.monitor.catalog.editor_mut(tool) {
            editor.draft.slot = loki_metris_core::clamp_profile_slot(slot);
        }
        self.monitor.catalog.changed(tool);
        cx.notify();
    }

    /// 图片选择只改草稿；保存档案前不会改变桌宠的权威绑定。
    pub(super) fn select_monitor_image(
        &mut self,
        tool: AiTool,
        behavior: HookBehavior,
        id: String,
        cx: &mut Context<Self>,
    ) {
        if self.monitor_tool() != Some(tool) {
            return;
        }
        self.bind_monitor_image(tool, behavior, id, cx);
    }

    /// 异步上传按捕获的工具身份绑定；用户切换 Agent 不得把回执丢弃。
    fn bind_monitor_image(
        &mut self,
        tool: AiTool,
        behavior: HookBehavior,
        id: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.monitor.catalog.editor_mut(tool) {
            if let Some(hook) = editor
                .draft
                .hooks
                .iter_mut()
                .find(|hook| hook.behavior == behavior)
            {
                hook.image = id;
            }
        }
        self.monitor.catalog.changed(tool);
        if self.monitor_tool() == Some(tool) {
            self.monitor.catalog.picker = None;
        }
        cx.notify();
    }

    /// 保存收到实际落盘回执才显示成功，提交之后的新草稿保留不回退。
    pub(super) fn save_monitor_profile(&mut self, cx: &mut Context<Self>) {
        if self.monitor.catalog.saving {
            return;
        }
        let Some(tool) = self.monitor_tool() else {
            return;
        };
        let Some(editor) = self.monitor.catalog.editor_mut(tool) else {
            return;
        };
        let draft = editor.draft.clone();
        let revision = editor.revision;
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.save_monitor_profile_draft(draft).ok());
        let state = &mut self.monitor.catalog;
        state.feedback = None;
        state.saved_tool = None;
        let Some(work) = work else {
            state.feedback = Some(Err(HookError::new("error.monitor.profilesWriteFailed")));
            cx.notify();
            return;
        };
        state.saving = true;
        state.save_task = Some(cx.spawn(async move |entity, cx| {
            let result = receipt(
                work,
                cx.background_executor().clone(),
                "error.monitor.profilesWriteFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.catalog;
                state.saving = false;
                match result {
                    Ok(draft) => {
                        if let Some(editor) = state.editor_mut(tool) {
                            if editor.revision == revision {
                                editor.draft = draft;
                                editor.reset_inputs = true;
                                state.saved_tool = Some(tool);
                            }
                        }
                    }
                    Err(error) => state.feedback = Some(Err(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 原生选择器取消不写入；多选逐项提交，部分失败仍保留已真实成功的项目。
    pub(super) fn upload_monitor_images(
        &mut self,
        target: Option<(AiTool, HookBehavior)>,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.monitor.catalog;
        if state.uploading || state.deleting.is_some() {
            return;
        }
        let Some(service) = self.dashboard.service.clone() else {
            return;
        };
        state.uploading = true;
        let selected = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: target.is_none(),
            prompt: None,
        });
        state.mutation_task = Some(cx.spawn(async move |entity, cx| {
            let paths = match selected.await {
                Ok(Ok(paths)) => paths.unwrap_or_default(),
                _ => {
                    let _ = entity.update(cx, |this, cx| {
                        this.monitor.catalog.uploading = false;
                        this.monitor.catalog.feedback =
                            Some(Err(HookError::new("error.monitor.imagesReadFailed")));
                        cx.notify();
                    });
                    return;
                }
            };
            let mut count = 0;
            let mut failure = None;
            let mut last = None;
            if !paths.is_empty() {
                let _ = entity.update(cx, |this, _| {
                    let state = &mut this.monitor.catalog;
                    state.feedback = None;
                    state.image_query = state.image_query.wrapping_add(1);
                    state.images_loading = false;
                });
            }
            for path in paths {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let result = match service.import_monitor_image(path) {
                    Ok(work) => {
                        receipt(
                            work,
                            cx.background_executor().clone(),
                            "error.monitor.imagesWriteFailed",
                        )
                        .await
                    }
                    Err(_) => Err(HookError::new("error.monitor.imagesWriteFailed")),
                };
                match result {
                    Ok(imported) => {
                        count += 1;
                        last = Some((imported, filename));
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }
            let _ = entity.update(cx, |this, cx| {
                this.monitor.catalog.uploading = false;
                if let Some((imported, filename)) = last {
                    let id = imported.id;
                    this.monitor.catalog.accept_images(imported.snapshot);
                    if let Some((tool, behavior)) = target {
                        this.bind_monitor_image(tool, behavior, id, cx);
                        this.monitor.catalog.feedback =
                            Some(Ok(CatalogSuccess::UploadedAndSelected { filename }));
                    } else {
                        this.monitor.catalog.feedback =
                            Some(Ok(CatalogSuccess::ImagesUploaded { count }));
                    }
                }
                if let Some(error) = failure {
                    this.monitor.catalog.feedback = Some(Err(error));
                }
                if !this.monitor.catalog.images_loaded && !this.monitor.catalog.images_loading {
                    this.load_monitor_images(cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - catalog_delete.rs：删除结果来自共享引用事务；失败时库与草稿保持原值。；监控操作共享服务回执桥接，不向外部Tokio线程注册GPUI前台waker。
include!("catalog_delete.rs");
