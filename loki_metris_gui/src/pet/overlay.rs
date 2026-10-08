//! 透明桌宠位置画布，交互只发意图；状态机、布局和分页来自core。

use super::{Controller, host};
use crate::monitor::{
    image_decode::{MonitorImageDecoder, MonitorImageSource},
    image_playback::MonitorImageView,
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
    source: MonitorImageSource,
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
    last_wheel: Option<Instant>,
    _controller: Subscription,
    _visibility: Subscription,
    _bounds: Subscription,
    _activation: Subscription,
    _focus: Subscription,
}
impl Overlay {
    /// 自有观察处理隐藏后的像素回收，失焦仍继续播放。
    pub fn new(owner: Entity<Controller>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("LokiMetris Pet");
        let controller = cx.observe(&owner, |_, _, cx| cx.notify());
        let visibility = cx.observe_window_visibility(window, |this, _, window, cx| {
            if !window.is_visible() {
                this.release_all(window, cx);
                this.hovered = false;
                this.keyboard_visible = false;
            }
            cx.notify();
        });
        let bounds_owner = owner.clone();
        let bounds = cx.observe_window_bounds(window, move |this, window, cx| {
            let bounds = host::bounds(window);
            let scale = window.scale_factor();
            bounds_owner.update(cx, |owner, cx| {
                let prefs = owner.settings.pet_window;
                let area = host::work_area(window);
                let range = loki_metris_core::PetSizeRange::for_work_area(
                    f64::from(f32::from(area.size.width.min(area.size.height))),
                    prefs.layout,
                );
                let size = pet_size_from_resize(
                    prefs.layout,
                    f64::from(f32::from(bounds.size.width)),
                    f64::from(f32::from(bounds.size.height)),
                    prefs.pet_size,
                    range,
                );
                if !owner.applying && !owner.busy && !prefs.locked && size != prefs.pet_size {
                    owner.change(PetPreferenceChange::NativeSize(size), cx);
                }
                owner.moved(bounds, scale, cx);
            });
            this.hovered = false;
            cx.notify();
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
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
        let focus_observe = cx.on_focus_in(&focus, window, |this, _, cx| {
            this.keyboard_visible = true;
            cx.notify();
        });
        Self {
            owner,
            focus,
            hovered: false,
            keyboard_visible: false,
            players: HashMap::new(),
            last_wheel: None,
            _controller: controller,
            _visibility: visibility,
            _bounds: bounds,
            _activation: activation,
            _focus: focus_observe,
        }
    }
    /// 最后挂载位置停止时释放当前窗GPU全部帧，旧绘制闭包只保留弱引用。
    fn release_player(player: &Player, window: &mut Window, cx: &mut Context<Self>) {
        let decoded = player.view.update(cx, |view, cx| {
            let decoded = view.release_pixels(cx);
            view.set_mounted(false, window, cx);
            decoded
        });
        if let Some(decoded) = decoded {
            let _ = window.drop_image(decoded.image.clone());
        }
        cx.remove_asset::<MonitorImageDecoder>(&player.source);
    }
    /// 系统隐藏立即清空缓存和动画GPU帧，时间线保留在各位置实体。
    fn release_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for player in self.players.values() {
            Self::release_player(player, window, cx);
        }
    }
    /// 事件和控件共用字段意图，锁定只由core决定缩放是否允许。
    fn change(&mut self, change: PetPreferenceChange, cx: &mut Context<Self>) {
        self.owner.update(cx, |owner, cx| owner.change(change, cx));
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
            let player = self.players.remove(&id).unwrap();
            Self::release_player(&player, window, cx);
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
                        source,
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
        let vmin = f32::from(
            window
                .viewport_size()
                .width
                .min(window.viewport_size().height),
        );
        let radius = f32::from(self.owner.read(cx).settings.pet_window.pet_size) * 0.28;
        let mut tile = div()
            .id(("pet-position", usize::from(id)))
            .test_support()
            .relative()
            .size_full()
            .overflow_hidden()
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
    /// 控件独立阻断按下和右键传播，不触发桌面拖动或主窗双击。
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
        let previous = owner.t("monitor.pet.previousPage");
        let next = owner.t("monitor.pet.nextPage");
        let active = page
            .slots
            .iter()
            .map(|slot| slot.slot_index)
            .collect::<HashSet<_>>();
        for (index, player) in &self.players {
            if !active.contains(index) || !window.is_visible() {
                Self::release_player(player, window, cx);
            }
        }
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
        let vmin = f32::from(
            window
                .viewport_size()
                .width
                .min(window.viewport_size().height),
        );
        let a11y_owner = self.owner.clone();
        let a11y_press_owner = self.owner.clone();
        div()
            .id("pet-overlay")
            .test_support()
            .size_full()
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
            .on_hover(cx.listener(|this, hovered, _, cx| {
                this.hovered = *hovered;
                if !hovered {
                    this.keyboard_visible = false;
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    if event.click_count > 1 {
                        this.owner.update(cx, |owner, cx| owner.show_main(cx));
                    } else if !locked {
                        host::start_drag(window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.owner.update(cx, |owner, cx| owner.open_settings(cx));
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                this.keyboard_visible = true;
                cx.notify();
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
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .child(
                            gpui_kit::base::Button::new("pet-previous")
                                .accessibility_label(previous)
                                .size(px((vmin * 0.06).clamp(14., 20.)))
                                .text_size(px((vmin * 0.05).clamp(11., 16.)))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|button| button.bg(rgba(0xffffff24)))
                                .child("‹")
                                .on_click(cx.listener(|this, _, _, cx| {
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
                                .accessibility_label(next)
                                .size(px((vmin * 0.06).clamp(14., 20.)))
                                .text_size(px((vmin * 0.05).clamp(11., 16.)))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|button| button.bg(rgba(0xffffff24)))
                                .child("›")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.change(PetPreferenceChange::TurnPage(1), cx)
                                })),
                        ),
                )
            })
    }
}
