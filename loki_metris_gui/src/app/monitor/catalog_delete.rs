// 本文件是 catalog.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：删除结果来自共享引用事务；失败时库与草稿保持原值。；监控操作共享服务回执桥接，不向外部Tokio线程注册GPUI前台waker。

impl Shell {
    /// 删除结果来自共享引用事务；失败时库与草稿保持原值。
    pub(super) fn delete_monitor_image(&mut self, id: String, cx: &mut Context<Self>) {
        let state = &mut self.monitor.catalog;
        if state.deleting.is_some() || state.uploading {
            return;
        }
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.delete_monitor_image(id.clone()).ok());
        let Some(work) = work else {
            state.feedback = Some(Err(HookError::new("error.monitor.imagesWriteFailed")));
            cx.notify();
            return;
        };
        state.deleting = Some(id);
        state.feedback = None;
        state.image_query = state.image_query.wrapping_add(1);
        state.images_loading = false;
        state.mutation_task = Some(cx.spawn(async move |entity, cx| {
            let result = receipt(
                work,
                cx.background_executor().clone(),
                "error.monitor.imagesWriteFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.catalog;
                state.deleting = None;
                match result {
                    Ok(snapshot) => state.accept_images(snapshot),
                    Err(error) => state.feedback = Some(Err(error)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 切换 Agent 卸载当前内嵌选择面板，仍保留所有未保存档案。
    pub(super) fn select_monitor_tool(&mut self, tool: AiTool, cx: &mut Context<Self>) {
        if !self.monitor_tools().contains(&tool) {
            return;
        }
        if self.monitor_tool() != Some(tool) { self.page_scroll.invalidate(); }
        let state = &mut self.monitor.catalog;
        state.selected = Some(tool);
        state.picker = None;
        state.picker_category = Category::All;
        state.feedback = None;
        state.saved_tool = None;
        cx.notify();
    }
}


/// 监控操作共享服务回执桥接，不向外部Tokio线程注册GPUI前台waker。
pub(super) use crate::usage_service::receipt;
