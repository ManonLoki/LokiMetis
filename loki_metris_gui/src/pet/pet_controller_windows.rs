// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：首图尝试之后接受的手动分页拥有最终选择权，包括仍在途和保存失败的自动请求。；原生主窗及服务就绪后建立默认显示的透明浮窗，不复制业务状态机。；更新现有辅助窗，不重建播放或几何实体。

impl Controller {
    /// 首图尝试之后接受的手动分页拥有最终选择权，包括仍在途和保存失败的自动请求。
    fn change(&mut self, change: PetPreferenceChange, cx: &mut Context<Self>) {
        if !self.quitting
            && (!self.busy || self.pending.len() < 64)
            && matches!(change, PetPreferenceChange::TurnPage(_) | PetPreferenceChange::FocusedSlot(_))
            && self.first_populated != FirstPopulatedFocus::Waiting
        {
            self.first_populated = FirstPopulatedFocus::Done;
        }
        self.enqueue_change(change, cx);
    }
    /// UI意图有界串行，滑块中间值合并；已接受的服务操作不会随设置窗关闭取消。
    fn enqueue_change(&mut self, change: PetPreferenceChange, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        if self.busy || self.display_applying {
            if matches!(
                change,
                PetPreferenceChange::Size(_) | PetPreferenceChange::NativeSize(_)
            ) {
                self.pending.retain(|old| {
                    !matches!(
                        old,
                        PetPreferenceChange::Size(_) | PetPreferenceChange::NativeSize(_)
                    )
                });
            }
            if self.pending.len() < 64 {
                self.pending.push_back(change)
            }
            return;
        }
        self.busy = true;
        self.scheduled = Some(change);
        let owner = cx.entity().clone();
        cx.defer(move |cx| owner.update(cx, |this, cx| this.begin_change(change, cx)));
    }
    /// 原生窗口操作在触发控件的Window更新结束之后执行。
    fn begin_change(&mut self, change: PetPreferenceChange, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.scheduled = None;
        let old = self.settings.pet_window;
        let layout = if let PetPreferenceChange::Layout(layout) = change {
            layout
        } else {
            old.layout
        };
        let range = self.range(layout, cx);
        let Ok(next) = change_pet_preferences(old, change, range) else {
            self.complete_first_focus(false, cx);
            self.busy = false;
            self.next_pending(cx);
            return;
        };
        let mouse_requested = matches!(change, PetPreferenceChange::MousePassthrough(_));
        // 显式穿透意图还须合并到真实store，避免首次读取的旧值覆盖恢复。
        if next == old && !mouse_requested {
            self.complete_first_focus(true, cx);
            self.busy = false;
            self.next_pending(cx);
            return;
        }
        // 页码只改变内容；重复宿主事务会刷新 bounds 并使原生指针坐标失效。
        let native_changed = old.layout != next.layout
            || old.pet_size != next.pet_size
            || old.always_on_top != next.always_on_top
            || old.locked != next.locked;
        let old_bounds = if native_changed {
            self.overlay
                .and_then(|handle| handle.update(cx, |_, window, _| host::bounds(window)).ok())
        } else {
            None
        };
        self.settings_generation = self.settings_generation.wrapping_add(1);
        self.settings.pet_window = next;
        if native_changed || mouse_requested {
            let applied = if native_changed { self.apply_geometry(false, cx) } else { self.apply_mouse_passthrough(cx) };
            if let Err(error) = applied {
                self.complete_first_focus(false, cx);
                self.settings.pet_window = old;
                let _ = self.rollback_native(native_changed, old_bounds, cx);
                self.last_error = Some(HookError::new("error.monitor.petApplyFailed"));
                tracing::warn!(%error,"pet native preference failed");
                self.busy = false;
                self.next_pending(cx);
                cx.notify();
                return;
            }
        }
        self.last_error = None;
        let work = self.service.change_pet_preferences(change, range);
        self.busy = true;
        self.mutation = Some(cx.spawn(async move |entity, cx| {
            let result = match work {
                Ok(work) => {
                    crate::usage_service::receipt(
                        work,
                        cx.background_executor().clone(),
                        "error.monitor.settingsWriteFailed",
                    )
                    .await
                }
                Err(_) => Err(HookError::new("error.monitor.settingsWriteFailed")),
            };
            let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                this.complete_first_focus(result.is_ok(), cx);
                match result {
                    Ok(settings) => {
                        this.settings = settings;
                        this.settings_ready = true;
                        this.observed.1 = this.service.pet_settings_revision();
                    }
                    Err(error) => {
                        this.settings.pet_window = old;
                        let restored = if native_changed || mouse_requested { this.rollback_native(native_changed, old_bounds, cx) } else { true };
                        tracing::warn!(code = error.code, "pet preference save failed");
                        this.last_error = Some(if restored { error } else { HookError::new("error.monitor.petApplyFailed") });
                    }
                }
                this.next_pending(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
    /// 成功、失败和无效意图都继续处理已经排队的后续字段修改。
    fn next_pending(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.display_applying || self.quitting { return; }
        if let Some(next) = self.pending.pop_front() {
            self.change(next, cx);
        }
    }
    /// 关闭设置销毁窗口；隐藏桌宠不销毁播放状态、不关闭中继。
    fn hide(&mut self, cx: &mut Context<Self>) {
        self.close_settings(cx);
        self.visible = false;
        self.service.today_visible(false);
        crate::native::set_pet_visibility(false, cx);
        self.cancel_display();
        let owner = cx.entity().clone();
        let generation = self.display_generation;
        let handle = self.overlay;
        cx.defer(move |cx| window_placement::hide(owner, generation, handle, cx));
        if !self.busy { self.next_pending(cx); }
        cx.notify();
    }
    /// 复用当前桌宠，显示时不激活主窗口。
    fn show(&mut self, cx: &mut Context<Self>) {
        if self.quitting { return; }
        self.visible = true;
        self.service.today_visible(true);
        crate::native::set_pet_visibility(true, cx);
        self.request_display(cx);
        self.refresh(cx);
        cx.notify();
    }
    /// 独立设置在入口捕获鼠标屏；创建与原生定位由专属显示事务执行。
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        if self.quitting { return; }
        self.settings_window_generation = self.settings_window_generation.wrapping_add(1);
        let request = settings_window::Request::capture(self.settings_window_generation, cx);
        let owner = cx.entity().clone();
        cx.defer(move |cx| settings_window::open(owner, request, cx));
    }
    /// 设置窗销毁不会取消controller持有的保存回执。
    fn close_settings(&mut self, cx: &mut App) {
        self.settings_window_generation = self.settings_window_generation.wrapping_add(1);
        if let Some(handle) = self.settings_window.take() {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
        }
    }
    /// 原生关闭已拥有销毁流程，只撤销显示租约并清除当前句柄。
    fn settings_window_closed(&mut self, handle: AnyWindowHandle) {
        if self.settings_window == Some(handle) {
            self.settings_window_generation = self.settings_window_generation.wrapping_add(1);
            self.settings_window = None;
        }
    }
    /// 返回主窗口时关闭设置，浮窗继续显示。
    fn show_main(&mut self, cx: &mut App) {
        self.close_settings(cx);
        crate::native::show(self.main, cx);
    }
    /// 所有辅助窗口文案随主应用当前语言变化。
    fn t(&self, key: &str) -> String {
        rust_i18n::t!(key, locale = &self.locale).to_string()
    }
}


/// 原生主窗及服务就绪后建立默认显示的透明浮窗，不复制业务状态机。
pub fn install(main: AnyWindowHandle, service: Arc<UsageService>, locale: &str, cx: &mut App) {
    let controller = cx.new(|_| Controller::new(service, main, locale));
    cx.set_global(Manager(controller.clone()));
    let owner = controller.clone();
    let mut view = None;
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(24.), px(24.)),
                size(px(128.), px(128. + geometry::HEADER_HEIGHT as f32)),
            ))),
            window_min_size: Some(size(px(64.), px(64. + geometry::HEADER_HEIGHT as f32))),
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: false,
            show: false,
            is_resizable: true,
            is_minimizable: false,
            app_owns_titlebar_drag: true,
            inactive_frame_interval: None,
            ..Default::default()
        },
        |window, cx| {
            let content = cx.new(|cx| overlay::Overlay::new(owner, window, cx));
            view = Some(content.clone());
            cx.new(|cx| Root::new(content, window, cx).bg(gpui_kit::transparent_black()))
        },
    );
    if let Ok(handle) = result {
        controller.update(cx, |this, cx| {
            this.service.today_visible(true);
            this.overlay = Some(handle.into());
            this.overlay_view = view;
            this.request_display(cx);
            this.refresh(cx);
        });
    }
    controller.update(cx, |this, cx| {
        this.poll = Some(cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if entity.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
            }
        }));
    });
}
/// 更新现有辅助窗，不重建播放或几何实体。
pub fn set_language(locale: &str, cx: &mut App) {
    if let Some(manager) = cx.try_global::<Manager>() {
        let owner = manager.0.clone();
        owner.update(cx, |this, cx| {
            this.locale = locale.into();
            cx.notify();
        });
    }
}

/// 窗口内容读取controller后先释放实体借用，再查询当前屏工作区。
fn range_for(overlay: Option<AnyWindowHandle>, layout: PetLayout, cx: &mut App) -> PetSizeRange {
    overlay
        .and_then(|handle| {
            handle
                .update(cx, |_, window, _| {
                    let area = host::work_area(window);
                    geometry::range(area, layout)
                })
                .ok()
        })
        .unwrap_or_else(|| PetSizeRange::for_work_area(f64::NAN, layout))
}

/// 与原版相同的托盘单项切换，显隐不落盘。
pub fn toggle(cx: &mut App) {
    if let Some(manager) = cx.try_global::<Manager>() {
        let owner = manager.0.clone();
        owner.update(cx, |this, cx| {
            if this.visible {
                this.hide(cx)
            } else {
                this.show(cx)
            }
        });
    }
}

/// 窗口尚存活时同步取出最终意图，之后由服务排空队列并保存。
pub(crate) fn take_shutdown_plan(cx: &mut App) -> crate::usage_service::PetShutdownPlan {
    let Some(manager) = cx.try_global::<Manager>() else {
        return Default::default();
    };
    let owner = manager.0.clone();
    owner.update(cx, |this, cx| {
        this.quitting = true;
        this.cancel_display();
        this.poll.take();
        this.query.take();
        this.position_task.take();
        let mut layout = this.settings.pet_window.layout;
        let intents = this
            .scheduled
            .take()
            .into_iter()
            .chain(this.pending.drain(..))
            .collect::<Vec<_>>();
        let changes = intents
            .into_iter()
            .map(|change| {
                if let PetPreferenceChange::Layout(next) = change {
                    layout = next;
                }
                (change, this.range(layout, cx))
            })
            .collect();
        crate::usage_service::PetShutdownPlan {
            changes,
            position: this.position.take_dirty(),
        }
    })
}
