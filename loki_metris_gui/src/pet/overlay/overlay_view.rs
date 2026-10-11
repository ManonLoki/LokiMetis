// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：分页栏阻断左键拖动／双击；任意位置右键均沿用参考的桌宠设置入口。

impl Render for Overlay {
    /// 分页栏阻断左键拖动／双击；任意位置右键均沿用参考的桌宠设置入口。
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.read(cx);
        let page = owner.page();
        let locked = owner.settings.pet_window.locked;
        let passthrough = owner.mouse_state.confirmed() == Some(true);
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
            !passthrough && (self.hovered || (self.keyboard_visible && focused && window.is_window_active()));
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
                if !this.accepts_mouse(cx) { return; }
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
                    if !this.accepts_mouse(cx) { return; }
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
                    if !this.accepts_mouse(cx) { return; }
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
                    if !this.accepts_mouse(cx) { return; }
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
