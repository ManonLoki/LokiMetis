//! 应用级桌宠owner：窗口、展示快照和字段级偏好回执独立于主页面。

pub(crate) mod host;
mod overlay;
mod geometry;
mod native_settle;
#[cfg(test)]
mod native_settle_tests;
mod preference_mutation;
mod today;
mod settings;
mod settings_window;
mod size_control;
mod position;
mod window_placement;
mod passthrough;
pub(crate) use passthrough::{open_settings, tray_actions};
#[cfg(test)]
mod passthrough_tests;
#[cfg(test)]
mod feedback_tests;
#[cfg(test)]
mod shutdown_receipt_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod resize_tests;
#[cfg(test)]
mod first_populated_tests;
#[cfg(test)]
mod today_tests;

use loki_metris_service::{
    monitor::{images::MonitorGallerySnapshot, settings::MonitorSettings},
    usage_service::UsageService,
};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Global, Task,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, base::Root, point, prelude::*, px,
    size,
};
use loki_metris_core::{
    AiProfileDraft, HookError, PetLayout, PetOverlayPage, PetOverlayToolState,
    PetPreferenceChange, PetSizeRange, change_pet_preferences,
    first_populated_pet_slot, project_available_pet_overlay_page, TodayUsageDisplay,
};
#[cfg(test)]
use loki_metris_core::PetOverlayPosition;
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
    /// 读写设置、图库与用量快照的应用服务。
    service: Arc<UsageService>,
    /// 主窗句柄，桌宠操作需要恢复主窗时使用。
    main: AnyWindowHandle,
    /// 桌宠浮窗句柄；未创建或已关闭时为 None。
    overlay: Option<AnyWindowHandle>,
    /// 桌宠配置窗句柄；未打开时为 None。
    settings_window: Option<AnyWindowHandle>,
    /// 配置窗显示意图独立于设置文件读取 generation，关闭也使尚未创建的显示失效。
    settings_window_generation: u64,
    /// 配置窗打开流程的在途任务。
    settings_window_task: Option<Task<()>>,
    /// 浮窗内容实体，跟随浮窗生命周期。
    overlay_view: Option<Entity<overlay::Overlay>>,
    /// 用户会话内的桌宠可见意图，不落盘。
    visible: bool,
    /// 当前界面语言。
    locale: String,
    /// 当日用量展示快照；只在变化时通知重绘。
    today: TodayUsageDisplay,
    /// 最近一次成功读取或保存的监控设置；事务进行中可能暂含尚未提交的新偏好。
    settings: MonitorSettings,
    /// 各 AI 工具的形象配置草稿。
    drafts: Vec<AiProfileDraft>,
    /// 公开监控工具的当前桌宠展示状态。
    states: Vec<PetOverlayToolState>,
    /// 按图片 ID 索引的已解码图库素材。
    images: HashMap<String, crate::monitor_media::image_decode::MonitorImageSource>,
    /// 已消费的修订号：(Agent 设置, 桌宠设置, 图库, 监控状态)；与服务端修订号不同才重新读取。
    observed: (u64, u64, u64, u64),
    /// 设置、草稿与图库的读取任务在途。
    loading: bool,
    /// 偏好事务代次；每次修改递增，迟到的读取或保存结果按代次丢弃。
    settings_generation: u64,
    /// 首次自动聚焦到有内容页面的进度。
    first_populated: FirstPopulatedFocus,
    /// 有偏好事务在途；期间新意图进入 `pending`。
    busy: bool,
    /// 事务在途时排队的偏好意图，最多 64 个；尺寸类意图只保留最新一个。
    pending: VecDeque<PetPreferenceChange>,
    /// 每 100ms 检查内存修订号并按需刷新的前台轮询任务。
    poll: Option<Task<()>>,
    /// 在途的设置/草稿/图库读取任务。
    query: Option<Task<()>>,
    /// 在途的偏好事务：原生确认、保存与失败回滚。
    mutation: Option<Task<()>>,
    /// 在途的位置保存任务；同一时刻至多一个。
    position_task: Option<Task<()>>,
    /// 正在同步应用几何；期间 bounds 回调不视为用户拖动。
    applying: bool,
    /// 原生命令尚未确认或正在回滚，期间不保存中间 bounds。
    native_mutating: bool,
    /// 启动后已按保存偏好恢复并确认过浮窗几何；之前不保存位置。
    geometry_restored: bool,
    /// 显示租约同时约束显隐和迁屏，旧 defer 不能覆盖较新的用户意图。
    display_generation: u64,
    /// 尚未完成的显示动作入口事实，等设置读取完成后续用。
    display_request: Option<window_placement::Request>,
    /// 在途的显示/迁屏确认任务。
    display_task: Option<Task<()>>,
    /// 原生迁屏尚未读回时屏蔽中间 bounds 的尺寸推导及位置保存。
    display_applying: bool,
    /// 取消或失败不能把已移动但未确认的原生矩形当作用户拖动保存。
    display_geometry_unconfirmed: bool,
    /// 已确认的迁屏或用户拖动几何，不从被取消的原生迁屏中间帧恢复锚点。
    display_state: Option<loki_metris_core::window_placement::WindowStateV2>,
    /// 至少成功读取或保存过一次设置；之前不按默认值恢复几何。
    settings_ready: bool,
    /// 浮窗鼠标穿透的原生确认状态。
    mouse_state: host::MousePassthroughState,
    /// 读取不完整或迁屏失败后的重试时刻；到期前不再读取。
    retry_after: Option<std::time::Instant>,
    /// 最近一次偏好事务的失败，供设置窗展示。
    last_error: Option<HookError>,
    /// 已接受、尚未交给服务保存的字段意图；退出时与 `pending` 一起补交。
    scheduled: Option<PetPreferenceChange>,
    /// 应用正在退出；置位后所有迟到回调都不再修改状态。
    quitting: bool,
    /// 位置保存的单写者队列。
    position: position::PositionQueue,
}
impl Controller {
    /// 公共配置首次失败保留安全展示默认，不发布到中继或writer。
    fn new(service: Arc<UsageService>, main: AnyWindowHandle, locale: &str) -> Self {
        Self {
            service,
            main,
            overlay: None,
            settings_window: None,
            settings_window_generation: 0,
            settings_window_task: None,
            overlay_view: None,
            visible: true,
            locale: locale.into(),
            today: TodayUsageDisplay::Pending,
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
            native_mutating: false,
            geometry_restored: false,
            display_generation: 0,
            display_request: None,
            display_task: None,
            display_applying: false,
            display_geometry_unconfirmed: false,
            display_state: None,
            settings_ready: false,
            mouse_state: Default::default(),
            retry_after: None,
            last_error: None,
            scheduled: None,
            quitting: false,
            position: Default::default(),
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
        let today = self.service.today_display();
        if self.today != today {
            self.today = today;
            cx.notify();
        }
        if let Ok(status) = self.service.monitor_status()
            && self.observed.3 != status.revision {
                self.states = status
                    .pet_states
                    .into_iter()
                    .filter(|state| loki_metris_core::is_public_monitor_tool(state.tool))
                    .collect();
                self.observed.3 = status.revision;
                cx.notify();
            }
        self.focus_first_populated(cx);
        let keys = self.snapshot_revisions();
        if self.loading
            || self.display_applying
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
                Ok(work) => crate::receipt::hook_receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsReadFailed",
                )
                .await
                .ok(),
                Err(_) => None,
            };
            let drafts = match work.1 {
                Ok(work) => crate::receipt::hook_receipt(
                    work,
                    cx.background_executor().clone(),
                    "error.monitor.settingsReadFailed",
                )
                .await
                .ok(),
                Err(_) => None,
            };
            let images = match work.2 {
                Ok(work) => crate::receipt::hook_receipt(
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
                if generation == this.settings_generation && !this.busy
                    && let Some(settings) = settings {
                        this.settings = settings;
                        this.settings_ready = true;
                        this.accept_loaded_geometry((keys.0, keys.1), cx);
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
                        crate::monitor_media::image_decode::MonitorImageSource {
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
            || self.display_applying
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
                    let range = geometry::range(area, prefs.layout);
                    let (width, height) =
                        geometry::dimensions(prefs.layout, range.constrain(prefs.pet_size));
                    let actual = host::bounds(window);
                    host::apply(window, prefs, false)?;
                    let (x, y) = if restore {
                        stored
                            .filter(|_| valid_stored)
                            .map(|position| {
                                (f64::from(position.x) / scale, f64::from(position.y) / scale - geometry::HEADER_HEIGHT)
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
        let result = result.and_then(|()| self.apply_mouse_passthrough(cx));
        if result.is_ok()
            && let Some(handle) = self.overlay {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| window.bounds_changed(cx));
                });
            }
        result
    }
    /// 属性或落盘失败时恢复此前原点和尺寸，不能保留夹紧后的新位置。
    fn restore_bounds(&self, bounds: Option<Bounds<gpui_kit::Pixels>>, cx: &mut App) {
        if let (Some(handle), Some(bounds)) = (self.overlay, bounds) {
            let _ = handle.update(cx, |_, window, _| host::set_geometry(window, bounds));
        }
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - pet_controller_windows.rs：首图尝试之后接受的手动分页拥有最终选择权，包括仍在途和保存失败的自动请求。；原生主窗及服务就绪后建立默认显示的透明浮窗，不复制业务状态机。；更新现有辅助窗，不重建播放或几何实体。
include!("pet_controller_windows.rs");
