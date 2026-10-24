//! 透明桌宠位置画布，交互只发意图；状态机、布局和分页来自core。

mod gesture;
#[cfg(test)]
mod gesture_tests;
#[cfg(test)]
mod image_residency_tests;
#[cfg(test)]
mod shared_asset_tests;

use super::{Controller, host};
use crate::monitor_media::{
    image_decode::MonitorImageSource,
    image_playback::MonitorImageView,
    image_residency::{self, ImageOwner},
};
use gpui_kit::{
    Context, Entity, FocusHandle, FontWeight, IntoElement, MouseButton, ObjectFit, Render, Role,
    Subscription, Window, base::TestSupportExt as _, div, prelude::*, px, rgba,
};
use loki_metris_core::{PetOverlaySlot, PetPreferenceChange, pet_size_from_resize};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

/// 每个绝对位置持有独立时钟，同图片原始字节通过Asset共享。
struct Player {
    /// 当前播放的图片 ID。
    id: String,
    /// 图片播放视图。
    view: Entity<MonitorImageView>,
    /// 订阅播放视图变化以重绘浮窗。
    _observe: Subscription,
}
/// 原生内容实体跟随窗口可见性，不随主页面导航卸载。
pub(super) struct Overlay {
    /// 所属桌宠控制器。
    owner: Entity<Controller>,
    /// 浮窗键盘焦点。
    focus: FocusHandle,
    /// 指针位于浮窗内。
    hovered: bool,
    /// 键盘操作后显示焦点指示；指针操作或失焦时隐藏。
    keyboard_visible: bool,
    /// 按格位索引的图片播放器，每格独立计时。
    players: HashMap<u8, Player>,
    /// 本窗口上一帧的可见素材；最后位置离开时仅退役本窗聚合需求。
    active_assets: HashSet<String>,
    /// 本浮窗的图片驻留归属。
    image_owner: ImageOwner,
    /// 最近一次被接受的滚轮翻页时刻；220ms 内的后续滚轮被忽略。
    last_wheel: Option<Instant>,
    /// 双击手势识别状态。
    double_click: gesture::DoubleClick,
    /// 订阅控制器变化以重绘。
    _controller: Subscription,
    /// 订阅窗口可见性变化。
    _visibility: Subscription,
    /// 订阅窗口 bounds 变化以识别用户拖动和拖边。
    _bounds: Subscription,
    /// 订阅窗口激活变化。
    _activation: Subscription,
    /// 订阅获得焦点。
    _focus: Subscription,
    /// 订阅失去焦点，用于隐藏键盘焦点指示。
    _focus_out: Subscription,
}
impl Overlay {
    /// 自有观察处理隐藏后的像素回收，失焦仍继续播放。
    pub fn new(owner: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("LokiMetris Pet");
        let controller = cx.observe(&owner, |this, owner, cx| {
            if owner.read(cx).mouse_state.confirmed() == Some(true) {
                this.double_click.cancel();
                this.hovered = false;
                this.keyboard_visible = false;
            }
            cx.notify();
        });
        let visibility = cx.observe_window_visibility(window, |this, _, window, cx| {
            if !window.is_visible() {
                this.double_click.cancel();
                this.release_all(window, cx);
                this.hovered = false;
                this.keyboard_visible = false;
            }
            cx.notify();
        });
        let bounds_owner = owner.clone();
        let bounds = cx.observe_window_bounds(window, move |_, window, cx| {
            let bounds = host::bounds(window);
            let scale = window.scale_factor();
            let trusted = bounds_owner.read(cx);
            let actual = if !trusted.applying && !trusted.native_mutating && !trusted.display_geometry_unconfirmed && trusted.geometry_restored {
                super::window_placement::observed_state(window, cx)
            } else { None };
            bounds_owner.update(cx, |owner, cx| {
                if let Some(actual) = actual { owner.display_state = Some(actual); }
                let prefs = owner.settings.pet_window;
                let area = host::work_area(window);
                let range = super::geometry::range(area, prefs.layout);
                let size = pet_size_from_resize(
                    prefs.layout,
                    f64::from(f32::from(bounds.size.width)),
                    f64::from(super::geometry::body_height(f32::from(bounds.size.height))),
                    prefs.pet_size,
                    range,
                );
                let geometry_matches = super::geometry::matches_dimensions(prefs.layout, size, bounds.size, scale);
                // 比较当前屏幕的有效尺寸，运行态夹紧不能改写历史偏好；比例偏差仍须纠正。
                if !owner.applying && !owner.display_applying && !owner.display_geometry_unconfirmed
                    && !owner.busy && !prefs.locked && (size != range.constrain(prefs.pet_size) || !geometry_matches) {
                    owner.change(PetPreferenceChange::NativeSize(size), cx);
                }
                owner.moved(bounds, scale, cx);
            });
            cx.notify();
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.double_click.cancel();
                this.hovered = false;
                this.keyboard_visible = false;
            }
            cx.notify();
        });
        let close_owner = owner.clone();
        window.on_window_should_close(cx, move |_, cx| {
            close_owner.update(cx, |owner, cx| owner.hide(cx));
            false
        });
        let focus = cx.focus_handle();
        let focus_observe = cx.on_focus_in(&focus, window, |this, _window, cx| {
            this.keyboard_visible = true;
            cx.notify();
        });
        let focus_out = cx.on_focus_out(&focus, window, |this, _, _, _| {
            this.double_click.cancel();
        });
        Self {
            owner,
            focus,
            hovered: false,
            keyboard_visible: false,
            players: HashMap::new(),
            active_assets: HashSet::new(),
            image_owner: ImageOwner::overlay(window, cx),
            last_wheel: None,
            double_click: gesture::DoubleClick::default(),
            _controller: controller,
            _visibility: visibility,
            _bounds: bounds,
            _activation: activation,
            _focus: focus_observe,
            _focus_out: focus_out,
        }
    }
    /// 位置离屏停止像素与计时；App 最后 owner 才退役共享缓存及所有窗口图集。
    fn release_player(player: &Player, window: &mut Window, cx: &mut Context<Self>) {
        player.view.update(cx, |view, cx| {
            view.set_mounted(false, window, cx);
        });
    }
    /// 先登记新页的全 App 需求，再停止退役位置；同 ID 跨页与跨窗继续共用原资源。
    fn sync_residency(&mut self, wanted: &HashMap<u8, MonitorImageSource>, window: &mut Window,
        cx: &mut Context<Self>) {
        let active_assets = wanted.values().map(|source| source.id.clone()).collect::<HashSet<_>>();
        let retired = self.active_assets.difference(&active_assets).cloned().collect::<HashSet<_>>();
        for source in wanted.values() { image_residency::acquire(source, &self.image_owner, cx); }
        for (slot, player) in &self.players {
            if wanted.get(slot).map(|source| &source.id) != Some(&player.id) {
                Self::release_player(player, window, cx);
            }
        }
        for id in retired {
            image_residency::release(&id, &self.image_owner, Some(window), cx);
        }
        self.active_assets = active_assets;
    }
    /// 系统隐藏立即退役本窗全部素材，时间线保留且重复隐藏不会重复驱逐 cache。
    fn release_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_residency(&HashMap::new(), window, cx);
    }
    /// 事件和控件共用字段意图，锁定只由core决定缩放是否允许。
    fn change(&mut self, change: PetPreferenceChange, cx: &mut Context<Self>) {
        self.owner.update(cx, |owner, cx| owner.change(change, cx));
    }
    /// 已开启后迟到的鼠标事件也不能恢复hover、拖动或双击候选。
    fn accepts_mouse(&self, cx: &gpui_kit::App) -> bool {
        self.owner.read(cx).mouse_state.confirmed() != Some(true)
    }
    /// Canvas仅在paint注册全窗取消观察，不创建Hitbox或拦截输入，覆盖离窗后返回。
    fn gesture_events(cx: &Context<Self>) -> gpui_kit::AnyElement {
        let down = cx.entity().downgrade();
        let motion = down.clone();
        let exit = down.clone();
        gpui_kit::canvas(|_, _, _| (), move |_, (), window, _| {
            window.on_mouse_event(move |_: &gpui_kit::MouseDownEvent, phase, _, cx| {
                if phase == gpui_kit::DispatchPhase::Capture {
                    let _ = down.update(cx, |this, _| this.double_click.cancel());
                }
            });
            window.on_mouse_event(move |event: &gpui_kit::MouseMoveEvent, phase, _, cx| {
                if phase == gpui_kit::DispatchPhase::Capture {
                    let _ = motion.update(cx, |this, _| this.double_click.motion(event.position));
                }
            });
            window.on_mouse_event(move |_: &gpui_kit::MouseExitEvent, phase, _, cx| {
                if phase == gpui_kit::DispatchPhase::Capture {
                    let _ = exit.update(cx, |this, _| this.double_click.cancel());
                }
            });
        }).absolute().inset_0().size_full().into_any_element()
    }
    /// 图片实体不跨窗口复用；同一绝对位置跨页保留自己的播放时间线。
    fn tile(
        &mut self,
        slot: &PetOverlaySlot,
        source: Option<MonitorImageSource>,
        empty_page: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let id = slot.slot_index;
        let wanted = source.as_ref().map(|source| source.id.as_str());
        if self
            .players
            .get(&id)
            .is_some_and(|player| Some(player.id.as_str()) != wanted)
        {
            self.players.remove(&id);
        }
        if let Some(source) = source
            && !self.players.contains_key(&id) {
                let view = cx.new(|cx| {
                    MonitorImageView::new(source.clone(), ObjectFit::Contain, window, cx)
                });
                let observe = cx.observe(&view, |_, _, cx| cx.notify());
                self.players.insert(
                    id,
                    Player {
                        id: source.id.clone(),
                        view,
                        _observe: observe,
                    },
                );
            }
        let decoded = self
            .players
            .get(&id)
            .is_some_and(|player| player.view.read(cx).is_decoded());
        let show_empty = !decoded
            && (self.hovered
                || empty_page
                || slot
                    .tile
                    .as_ref()
                    .and_then(|tile| tile.image_key.as_ref())
                    .is_some());
        let vmin = super::geometry::body_vmin(window);
        let radius = f32::from(self.owner.read(cx).settings.pet_window.pet_size) * 0.28;
        let mut tile = div()
            .id(("pet-position", usize::from(id)))
            .test_support()
            .relative()
            .size_full()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event, _, cx| {
                if !this.accepts_mouse(cx) { return; }
                this.double_click.begin(id, event);
            }))
            .on_click(cx.listener(move |this, event, _, cx| {
                if this.accepts_mouse(cx) && this.double_click.complete(id, event) {
                    this.owner.update(cx, |owner, cx| owner.show_main(cx));
                }
            }))
            .when(!decoded, |tile| {
                tile.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgba(0xffffff9e))
                        .bg(rgba(0x0f121a40))
                        .opacity(if self.hovered {
                            1.
                        } else if show_empty {
                            0.22
                        } else {
                            0.
                        })
                        .child(
                            div()
                                .absolute()
                                .inset(px(1.))
                                .rounded(px(radius))
                                .border_1()
                                .border_dashed()
                                .border_color(rgba(0xffffff2e)),
                        )
                        .child(
                            div()
                                .text_size(px((vmin * 0.05).clamp(13., 22.)))
                                .font_weight(FontWeight(750.))
                                .child(format!("{:02}", id + 1)),
                        ),
                )
            });
        if let Some(player) = self.players.get(&id) {
            player
                .view
                .update(cx, |view, cx| view.set_mounted(true, window, cx));
            tile = tile.child(
                div()
                    .id(("pet-labels", usize::from(id)))
                    .absolute()
                    .size_full()
                    .child(player.view.clone()),
            );
        }
        if let Some(content) = &slot.tile
            && self.hovered {
                tile = tile.child(
                    div()
                        .id(("pet-labels", usize::from(id)))
                        .absolute()
                        .size_full()
                        .p(px((vmin * 0.012).clamp(2., 6.)))
                        .flex()
                        .flex_col()
                        .justify_between()
                        .text_color(rgba(0xffffffff))
                        .line_height(gpui_kit::relative(1.2))
                        .role(Role::Label)
                        .aria_label(format!("{}，{}", content.name, content.content))
                        .child(
                            div()
                                .text_size(px((vmin * 0.045).clamp(10., 14.)))
                                .font_weight(FontWeight::BOLD)
                                .truncate()
                                .child(content.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px((vmin * 0.038).clamp(9., 12.)))
                                .truncate()
                                .child(content.content.clone()),
                        ),
                );
            }
        tile.into_any_element()
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - overlay_view.rs：分页栏阻断左键拖动／双击；任意位置右键均沿用参考的桌宠设置入口。
include!("overlay_view.rs");
