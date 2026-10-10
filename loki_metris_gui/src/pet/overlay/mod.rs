//! 透明桌宠位置画布，交互只发意图；状态机、布局和分页来自core。

mod gesture;
#[cfg(test)]
mod gesture_tests;
#[cfg(test)]
mod image_residency_tests;
#[cfg(test)]
mod shared_asset_tests;

use super::{Controller, host};
use crate::monitor::{
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
    id: String,
    view: Entity<MonitorImageView>,
    _observe: Subscription,
}
/// 原生内容实体跟随窗口可见性，不随主页面导航卸载。
pub(super) struct Overlay {
    owner: Entity<Controller>,
    focus: FocusHandle,
    hovered: bool,
    keyboard_visible: bool,
    players: HashMap<u8, Player>,
    /// 本窗口上一帧的可见素材；最后位置离开时仅退役本窗聚合需求。
    active_assets: HashSet<String>,
    image_owner: ImageOwner,
    last_wheel: Option<Instant>,
    double_click: gesture::DoubleClick,
    _controller: Subscription,
    _visibility: Subscription,
    _bounds: Subscription,
    _activation: Subscription,
    _focus: Subscription,
    _focus_out: Subscription,
}
impl Overlay {
    /// 自有观察处理隐藏后的像素回收，失焦仍继续播放。
    pub fn new(owner: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("LokiMetris Pet");
        let controller = cx.observe(&owner, |_, _, cx| cx.notify());
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
            bounds_owner.update(cx, |owner, cx| {
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
                if !owner.applying && !owner.busy && !prefs.locked && size != prefs.pet_size {
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
        if let Some(source) = source {
            if !self.players.contains_key(&id) {
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
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, event, _, _| {
                this.double_click.begin(id, event);
            }))
            .on_click(cx.listener(move |this, event, _, cx| {
                if this.double_click.complete(id, event) {
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
        if let Some(content) = &slot.tile {
            if self.hovered {
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
        }
        tile.into_any_element()
    }
}
impl Render for Overlay {
    /// 分页栏阻断左键拖动／双击；任意位置右键均沿用参考的桌宠设置入口。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.read(cx);
        let page = owner.page();
        let locked = owner.settings.pet_window.locked;
        let label = owner
            .t("monitor.pet.shellAria")
            .replace("%{page}", &(page.page_index + 1).to_string())
            .replace("%{pages}", &page.page_count.to_string());
        let sources = page
            .slots
            .iter()
            .map(|slot| {
                slot.tile
                    .as_ref()
                    .and_then(|tile| tile.image_key.as_ref())
                    .and_then(|id| owner.images.get(id))
                    .cloned()
            })
            .collect::<Vec<_>>();
        let (today, today_aria) = super::today::labels(owner.today, &owner.locale, f32::from(window.viewport_size().width) < 200.);
        let previous = owner.t("monitor.pet.previousPage");
        let next = owner.t("monitor.pet.nextPage");
        let wanted = if window.is_visible() {
            page.slots.iter().zip(&sources).filter_map(|(slot, source)| {
                source.as_ref().map(|source| (slot.slot_index, source.clone()))
            }).collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        self.sync_residency(&wanted, window, cx);
        let (rows, columns) = page.layout.dimensions();
        let mut grid = div()
            .size_full()
            .grid()
            .grid_cols(columns as u16)
            .grid_rows(rows as u16);
        for (slot, source) in page.slots.iter().zip(sources) {
            grid = grid.child(self.tile(slot, source, !page.page_has_image, window, cx));
        }
        let focused = self.focus.contains_focused(window, cx);
        let controls =
            self.hovered || (self.keyboard_visible && focused && window.is_window_active());
        let vmin = super::geometry::body_vmin(window);
        let a11y_owner = self.owner.clone();
        let a11y_press_owner = self.owner.clone();
        let body = div()
            .id("pet-overlay")
            .test_support()
            .w_full()
            .h(px(super::geometry::body_height(f32::from(window.viewport_size().height))))
            .flex_shrink_0()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus)
            .role(Role::Group)
            .aria_label(label)
            // macOS的AccessKit暂不映射ShowContextMenu；辅助激活提供相同设置入口。
            .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, _, cx| {
                a11y_press_owner.update(cx, |owner, cx| owner.open_settings(cx))
            })
            .on_a11y_action(
                gpui_kit::AccessibleAction::ShowContextMenu,
                move |_, _, cx| a11y_owner.update(cx, |owner, cx| owner.open_settings(cx)),
            )
            .hover_listener_mode(gpui_kit::HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(|this, hovered, _window, cx| {
                this.hovered = *hovered;
                if !hovered {
                    this.double_click.cancel();
                    this.keyboard_visible = false;
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if event.click_count == 1 && !locked {
                        host::start_drag(window, cx);
                    }
                }),
            )
            .on_mouse_up(MouseButton::Left, cx.listener(|_this, _, _window, _cx| {
                _this.double_click.cancel();
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.double_click.cancel();
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.double_click.cancel();
                    cx.stop_propagation();
                    this.owner.update(cx, |owner, cx| owner.open_settings(cx));
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, window, cx| {
                this.keyboard_visible = true;
                cx.notify();
                let recognized = match event.keystroke.key.as_str() {
                    "left" | "right" | "menu" | "contextmenu" => true,
                    "f10" => event.keystroke.modifiers.shift,
                    _ => false,
                };
                if recognized {
                    this.focus.focus(window, cx);
                    // macOS以propagate判定是否进入原生键等价／IME回退，已处理事件必须消费。
                    cx.stop_propagation();
                }
                match event.keystroke.key.as_str() {
                    "left" => this.change(PetPreferenceChange::TurnPage(-1), cx),
                    "right" => this.change(PetPreferenceChange::TurnPage(1), cx),
                    "menu" | "contextmenu" => {
                        this.owner.update(cx, |owner, cx| owner.open_settings(cx))
                    }
                    "f10" if event.keystroke.modifiers.shift => {
                        this.owner.update(cx, |owner, cx| owner.open_settings(cx))
                    }
                    _ => {}
                }
            }))
            .on_scroll_wheel(
                cx.listener(|this, event: &gpui_kit::ScrollWheelEvent, _, cx| {
                    let now = cx.background_executor().now();
                    if this.last_wheel.is_some_and(|last| {
                        now.saturating_duration_since(last) < Duration::from_millis(220)
                    }) {
                        return;
                    }
                    this.last_wheel = Some(now);
                    let up = event.delta.pixel_delta(px(16.)).y > px(0.);
                    this.change(
                        if event.modifiers.control || event.modifiers.platform {
                            PetPreferenceChange::ResizeStep(up)
                        } else {
                            PetPreferenceChange::TurnPage(if up { -1 } else { 1 })
                        },
                        cx,
                    );
                }),
            )
            .child(grid)
            .child(Self::gesture_events(cx))
            .when(controls, |surface| {
                surface.child(
                    div()
                        .id("pet-pager")
                        .test_support()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(2.))
                        .bg(rgba(0x0a0d14d6))
                        .border_t_1()
                        .border_color(rgba(0xffffff1f))
                        .on_mouse_down(MouseButton::Left, cx.listener(|_this, _, _window, cx| {
                            _this.double_click.cancel();
                            cx.stop_propagation();
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            this.double_click.cancel();
                            cx.stop_propagation();
                        }))
                        .child(
                            gpui_kit::base::Button::new("pet-previous")
                                .accessibility_label(previous.clone())
                                .disabled(page.page_count <= 1)
                                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(previous.clone()).build(window, cx))
                                .size(px((vmin * 0.06).clamp(14., 20.)))
                                .text_size(px((vmin * 0.05).clamp(11., 16.)))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|button| button.bg(rgba(0xffffff24)))
                                .child("‹")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.change(PetPreferenceChange::TurnPage(-1), cx)
                                })),
                        )
                        .child(
                            div()
                                .text_size(px((vmin * 0.028).clamp(7., 9.)))
                                .text_color(rgba(0xffffffc2))
                                .child(format!("{}/{}", page.page_index + 1, page.page_count)),
                        )
                        .child(
                            gpui_kit::base::Button::new("pet-next")
                                .accessibility_label(next.clone())
                                .disabled(page.page_count <= 1)
                                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(next.clone()).build(window, cx))
                                .size(px((vmin * 0.06).clamp(14., 20.)))
                                .text_size(px((vmin * 0.05).clamp(11., 16.)))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|button| button.bg(rgba(0xffffff24)))
                                .child("›")
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.change(PetPreferenceChange::TurnPage(1), cx)
                                })),
                        ),
                )
            });
        let tooltip = today_aria.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().h(px(super::geometry::HEADER_HEIGHT as f32)).w_full().flex_shrink_0().pb(px(4.))
                .child(div().id("pet-today-tokens").test_support().size_full().px(px(4.))
                    .flex().items_center().justify_center().overflow_hidden().rounded(px(7.))
                    .bg(rgba(0x0f121ad9)).text_color(rgba(0xfffffff2)).text_size(px(11.))
                    .role(Role::Label).aria_label(today_aria)
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
                    .child(div().truncate().child(today))))
            .child(body)
    }
}
