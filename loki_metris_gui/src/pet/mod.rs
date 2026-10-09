//! 应用级桌宠owner：窗口、展示快照和字段级偏好回执独立于主页面。

pub(crate) mod host;
mod overlay;
mod settings;
mod size_control;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod first_populated_tests;

use crate::{
    monitor::{images::MonitorGallerySnapshot, settings::MonitorSettings},
    usage_service::UsageService,
};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Global, Task,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, base::Root, point, prelude::*, px,
    size,
};
use loki_metris_core::{
    AiProfileDraft, HookError, PetLayout, PetOverlayPage, PetOverlayPosition, PetOverlayToolState,
    PetPreferenceChange, PetSizeRange, PetWindowPreferences, change_pet_preferences,
    first_populated_pet_slot, pet_window_dimensions, project_available_pet_overlay_page,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

/// 唯一全局句柄拥有controller，窗口内容只订阅这个实体。
struct Manager(Entity<Controller>);
impl Global for Manager {}
/// 首图自动定位只消费成功事实；失败限频重试，后续手动分页结束自动阶段。
#[derive(Clone, Copy, PartialEq, Eq)]
enum FirstPopulatedFocus {
    /// 尚未收到可用图片。
    Waiting,
    /// 唯一自动偏好事务尚未收到回执。
    Pending,
    /// 保存失败后等待下一次允许的轮询。
    Retry(std::time::Instant),
    /// 已定位、当前页已有图或用户在尝试后手动选择页面。
    Done,
}
/// 桌宠投影只缓存成功事实，损坏配置的默认值只用于当前展示。
struct Controller {
    service: Arc<UsageService>,
    main: AnyWindowHandle,
    overlay: Option<AnyWindowHandle>,
    settings_window: Option<AnyWindowHandle>,
    overlay_view: Option<Entity<overlay::Overlay>>,
    visible: bool,
    locale: String,
    settings: MonitorSettings,
    drafts: Vec<AiProfileDraft>,
    states: Vec<PetOverlayToolState>,
    images: HashMap<String, crate::monitor::image_decode::MonitorImageSource>,
    observed: (u64, u64, u64, u64),
    loading: bool,
    settings_generation: u64,
    first_populated: FirstPopulatedFocus,
    busy: bool,
    pending: VecDeque<PetPreferenceChange>,
    poll: Option<Task<()>>,
    query: Option<Task<()>>,
    mutation: Option<Task<()>>,
    position_task: Option<Task<()>>,
    applying: bool,
    geometry_restored: bool,
    retry_after: Option<std::time::Instant>,
    last_error: Option<HookError>,
    scheduled: Option<PetPreferenceChange>,
    quitting: bool,
    position_dirty: Option<PetOverlayPosition>,
}
impl Controller {
    /// 公共配置首次失败保留安全展示默认，不发布到中继或writer。
    fn new(service: Arc<UsageService>, main: AnyWindowHandle, locale: &str) -> Self {
        Self {
            service,
            main,
            overlay: None,
            settings_window: None,
            overlay_view: None,
            visible: true,
            locale: locale.into(),
            settings: MonitorSettings::default(),
            drafts: Vec::new(),
            states: Vec::new(),
            images: HashMap::new(),
            observed: (u64::MAX, u64::MAX, u64::MAX, u64::MAX),
            loading: false,
            settings_generation: 0,
            first_populated: FirstPopulatedFocus::Waiting,
            busy: false,
            pending: VecDeque::new(),
            poll: None,
            query: None,
            mutation: None,
            position_task: None,
            applying: false,
            geometry_restored: false,
            retry_after: None,
            last_error: None,
            scheduled: None,
            quitting: false,
            position_dirty: None,
        }
    }
    /// 三类文件快照版本由服务拥有，首图定位须等待与当前版本一致的完整读取。
    fn snapshot_revisions(&self) -> (u64, u64, u64) {
        (
            self.service.agent_settings_revision(),
            self.service.pet_settings_revision(),
            self.service.monitor_catalog_revision(),
        )
    }
    /// 文件快照只在设置或图库代数变化时读取，事件投影每100ms检查内存版本。
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Ok(status) = self.service.monitor_status() {
            if self.observed.3 != status.revision {
                self.states = status
                    .pet_states
                    .into_iter()
                    .filter(|state| loki_metris_core::is_public_monitor_tool(state.tool))
                    .collect();
                self.observed.3 = status.revision;
                cx.notify();
            }
        }
        self.focus_first_populated(cx);
        let keys = self.snapshot_revisions();
        if self.loading
            || self
                .retry_after
                .is_some_and(|retry| cx.background_executor().now() < retry)
            || ((self.observed.0, self.observed.1, self.observed.2) == keys
                && self.retry_after.is_none())
        {
            return;
        }
        self.loading = true;
        let work = (
            self.service.monitor_settings(),
            self.service.list_monitor_profile_drafts(),
            self.service.list_monitor_images(),
        );
        let generation = self.settings_generation;
        self.query = Some(cx.spawn(async move |entity, cx| {
            let settings = match work.0 {
                Ok(work) => crate::usage_service::receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsReadFailed",
                )
                .await
                .ok(),
                Err(_) => None,
            };
            let drafts = match work.1 {
                Ok(work) => crate::usage_service::receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsReadFailed",
                )
                .await
                .ok(),
                Err(_) => None,
            };
            let images = match work.2 {
                Ok(work) => crate::usage_service::receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsReadFailed",
                )
                .await
                .ok(),
                Err(_) => None,
            };
            let _ = entity.update(cx, |this, cx| {
                this.loading = false;
                let catalog_complete = drafts.is_some() && images.is_some();
                let complete = settings.is_some() && catalog_complete;
                this.retry_after =
                    (!complete).then(|| cx.background_executor().now() + Duration::from_secs(2));
                if generation == this.settings_generation && !this.busy {
                    if let Some(settings) = settings {
                        this.settings = settings;
                        match this.apply_geometry(!this.geometry_restored, cx) {
                            Ok(()) => {
                                this.geometry_restored = true;
                                this.observed.0 = keys.0;
                                this.observed.1 = keys.1;
                            }
                            Err(error) => {
                                tracing::warn!(%error,"pet geometry restore failed");
                                this.retry_after =
                                    Some(cx.background_executor().now() + Duration::from_secs(2));
                            }
                        }
                    }
                }
                if let Some(drafts) = drafts {
                    this.drafts = drafts.drafts;
                }
                if let Some(images) = images {
                    this.accept_images(images);
                    if catalog_complete {
                        this.observed.2 = keys.2;
                    }
                }
                this.focus_first_populated(cx);
                cx.notify();
            });
        }));
    }
    /// 只保留可读取原图，缺失资源不计入有图页。
    fn accept_images(&mut self, mut snapshot: MonitorGallerySnapshot) {
        self.images = snapshot
            .gallery
            .images
            .into_iter()
            .filter_map(|image| {
                snapshot.bytes.remove(&image.id).map(|bytes| {
                    (
                        image.id.clone(),
                        crate::monitor::image_decode::MonitorImageSource {
                            id: image.id,
                            bytes,
                            format: image.format,
                        },
                    )
                })
            })
            .collect();
    }
    /// 可用性来自文件宿主，布局、占位和冲突仍由core投影。
    fn page(&self) -> PetOverlayPage {
        let prefs = self.settings.pet_window;
        project_available_pet_overlay_page(
            &self.drafts,
            &self.states,
            prefs.layout,
            usize::from(prefs.focused_slot) / prefs.layout.capacity(),
            |id| self.images.contains_key(id),
        )
    }
    /// 首图只消费完整且版本仍然一致的快照；在途查询、读取失败和手动事务不能排入旧目标。
    fn focus_first_populated(&mut self, cx: &mut Context<Self>) {
        if self.quitting
            || self.loading
            || self.retry_after.is_some()
            || self.busy
            || !self.pending.is_empty()
            || !self.geometry_restored
            || (self.observed.0, self.observed.1, self.observed.2) != self.snapshot_revisions()
        {
            return;
        }
        match self.first_populated {
            FirstPopulatedFocus::Pending | FirstPopulatedFocus::Done => return,
            FirstPopulatedFocus::Retry(at) if cx.background_executor().now() < at => return,
            FirstPopulatedFocus::Waiting | FirstPopulatedFocus::Retry(_) => {}
        }
        let page = self.page();
        if !page.has_any_image {
            return;
        }
        if page.page_has_image {
            self.first_populated = FirstPopulatedFocus::Done;
            return;
        }
        if let Some(slot) = first_populated_pet_slot(&self.drafts, &self.states, |id| {
            self.images.contains_key(id)
        }) {
            self.first_populated = FirstPopulatedFocus::Pending;
            self.enqueue_change(PetPreferenceChange::FocusedSlot(slot), cx);
        }
    }
    /// 旧自动回执不能复活已被手动分页取消的定位；持续写入失败最多每两秒重试一次。
    fn complete_first_focus(&mut self, success: bool, cx: &Context<Self>) {
        if self.first_populated == FirstPopulatedFocus::Pending {
            self.first_populated = if success {
                FirstPopulatedFocus::Done
            } else {
                FirstPopulatedFocus::Retry(cx.background_executor().now() + Duration::from_secs(2))
            };
        }
    }
    /// 当前屏工作区只属于宿主；尺寸约束由core推导。
    fn range(&self, layout: PetLayout, cx: &mut App) -> PetSizeRange {
        range_for(self.overlay, layout, cx)
    }
    /// 同一个窗口动态应用尺寸和属性；范围变化时夹紧位置。
    fn apply_geometry(&mut self, restore: bool, cx: &mut App) -> anyhow::Result<()> {
        let prefs = self.settings.pet_window;
        let stored = self.settings.pet_overlay_position;
        self.applying = true;
        let result = if let Some(handle) = self.overlay {
            handle
                .update(cx, |_, window, _| {
                    let (area, scale, valid_stored) = if restore {
                        host::restoration_area(window, stored, prefs)
                    } else {
                        (
                            host::work_area(window),
                            f64::from(window.scale_factor()),
                            false,
                        )
                    };
                    let range = PetSizeRange::for_work_area(
                        f64::from(f32::from(area.size.width.min(area.size.height))),
                        prefs.layout,
                    );
                    let (width, height) =
                        pet_window_dimensions(prefs.layout, range.constrain(prefs.pet_size));
                    host::apply(window, prefs, false)?;
                    let actual = host::bounds(window);
                    let (x, y) = if restore {
                        stored
                            .filter(|_| valid_stored)
                            .map(|position| {
                                (f64::from(position.x) / scale, f64::from(position.y) / scale)
                            })
                            .unwrap_or((
                                f64::from(f32::from(area.right())) - width - 16.,
                                f64::from(f32::from(area.bottom())) - height - 16.,
                            ))
                    } else {
                        (
                            f64::from(f32::from(actual.origin.x)),
                            f64::from(f32::from(actual.origin.y)),
                        )
                    };
                    let left = f64::from(f32::from(area.origin.x));
                    let top = f64::from(f32::from(area.origin.y));
                    host::set_geometry(
                        window,
                        Bounds::new(
                            point(
                                px(x.clamp(
                                    left,
                                    (f64::from(f32::from(area.right())) - width).max(left),
                                ) as f32),
                                px(y.clamp(
                                    top,
                                    (f64::from(f32::from(area.bottom())) - height).max(top),
                                ) as f32),
                            ),
                            size(px(width as f32), px(height as f32)),
                        ),
                    )
                })
                .and_then(|result| result)
        } else {
            Err(anyhow::anyhow!("pet overlay unavailable"))
        };
        self.applying = false;
        if result.is_ok() {
            if let Some(handle) = self.overlay {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| window.bounds_changed(cx));
                });
            }
        }
        result
    }
    /// 属性或落盘失败时恢复此前原点和尺寸，不能保留夹紧后的新位置。
    fn restore_bounds(&self, bounds: Option<Bounds<gpui_kit::Pixels>>, cx: &mut App) {
        if let (Some(handle), Some(bounds)) = (self.overlay, bounds) {
            let _ = handle.update(cx, |_, window, _| host::set_geometry(window, bounds));
        }
    }

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
        if self.busy {
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
        if next == old {
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
        if native_changed {
            if let Err(error) = self.apply_geometry(false, cx) {
                self.complete_first_focus(false, cx);
                self.settings.pet_window = old;
                let _ = self.apply_geometry(false, cx);
                self.restore_bounds(old_bounds, cx);
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
                        this.observed.1 = this.service.pet_settings_revision();
                    }
                    Err(error) => {
                        this.settings.pet_window = old;
                        if native_changed {
                            let _ = this.apply_geometry(false, cx);
                            this.restore_bounds(old_bounds, cx);
                        }
                        tracing::warn!(code = error.code, "pet preference save failed");
                        this.last_error = Some(error);
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
        if let Some(next) = self.pending.pop_front() {
            self.change(next, cx);
        }
    }
    /// 几何观察只登记200ms防抖；布局变动后的旧尺寸不会再落盘。
    fn moved(&mut self, bounds: Bounds<gpui_kit::Pixels>, scale: f32, cx: &mut Context<Self>) {
        if self.applying || !self.geometry_restored || self.quitting {
            return;
        }
        let position = PetOverlayPosition {
            x: (f32::from(bounds.origin.x) * scale).round() as i32,
            y: (f32::from(bounds.origin.y) * scale).round() as i32,
        };
        self.position_dirty = Some(position);
        let service = self.service.clone();
        self.position_task = Some(cx.spawn(async move |entity, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            if let Ok(work) = service.save_pet_position(position) {
                if crate::usage_service::receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsWriteFailed",
                )
                .await
                .is_ok()
                {
                    let _ = entity.update(cx, |this, _| {
                        if this.position_dirty == Some(position) {
                            this.position_dirty = None;
                        }
                        this.settings.pet_overlay_position = Some(position);
                    });
                }
            }
        }));
    }
    /// 关闭设置销毁窗口；隐藏桌宠不销毁播放状态、不关闭中继。
    fn hide(&mut self, cx: &mut Context<Self>) {
        self.close_settings(cx);
        self.visible = false;
        crate::native::set_pet_visibility(false, cx);
        if let Some(handle) = self.overlay {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| host::set_visible(window, false));
            });
        }
        cx.notify();
    }
    /// 复用当前桌宠，显示时不激活主窗口。
    fn show(&mut self, cx: &mut Context<Self>) {
        self.visible = true;
        crate::native::set_pet_visibility(true, cx);
        let _ = self.apply_geometry(false, cx);
        if let Some(handle) = self.overlay {
            let _ = handle.update(cx, |_, window, _| host::set_visible(window, true));
        }
        self.refresh(cx);
        cx.notify();
    }
    /// 独立设置窗按当前桌宠工作区居中，关闭后下次重新创建。
    fn open_settings(&mut self, cx: &mut Context<Self>) {
        let owner = cx.entity().clone();
        cx.defer(move |cx| open_settings_window(owner, cx));
    }
    /// 设置窗销毁不会取消controller持有的保存回执。
    fn close_settings(&mut self, cx: &mut App) {
        if let Some(handle) = self.settings_window.take() {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            });
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
                size(px(128.), px(128.)),
            ))),
            window_min_size: Some(size(px(64.), px(64.))),
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
            this.overlay = Some(handle.into());
            this.overlay_view = view;
            let _ = this.apply_geometry(true, cx);
            let _ = handle.update(cx, |_, window, _| host::set_visible(window, true));
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
                    PetSizeRange::for_work_area(
                        f64::from(f32::from(area.size.width.min(area.size.height))),
                        layout,
                    )
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

/// 新窗首帧会读取controller，创建必须在它的可变借用之外。
fn open_settings_window(owner: Entity<Controller>, cx: &mut App) {
    let overlay = owner.read(cx).overlay;
    let area = overlay
        .and_then(|handle| {
            handle
                .update(cx, |_, window, _| host::work_area(window))
                .ok()
        })
        .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))));
    let origin = point(
        area.origin.x + (area.size.width - px(320.)) / 2.,
        area.origin.y + (area.size.height - px(470.)) / 2.,
    );
    if let Some(handle) = owner.read(cx).settings_window {
        let _ = handle.update(cx, |_, window, cx| {
            if let Err(error) = host::move_to(
                window,
                f64::from(f32::from(origin.x)),
                f64::from(f32::from(origin.y)),
            ) {
                tracing::warn!(%error,"pet settings recenter failed");
            }
            window.bounds_changed(cx);
            window.activate_window();
        });
        return;
    }
    let controller = owner.clone();
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                origin,
                size(px(320.), px(470.)),
            ))),
            window_min_size: Some(size(px(280.), px(440.))),
            titlebar: None,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| {
            let _ = host::apply(window, PetWindowPreferences::default(), true);
            let _ = host::move_to(
                window,
                f64::from(f32::from(origin.x)),
                f64::from(f32::from(origin.y)),
            );
            let view = cx.new(|cx| settings::Settings::new(controller, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
    if let Ok(handle) = result {
        owner.update(cx, |this, _| this.settings_window = Some(handle.into()));
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
            position: this.position_dirty.take(),
        }
    })
}
