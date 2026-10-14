// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：稳定作用域隔离相邻按钮的短暂反馈owner。

impl Element for SurfaceButton {
    /// 内部按钮元素、按压动效 owner 与加载中遮罩元素（仅加载态存在）。
    type RequestLayoutState = (AnyElement, Entity<ButtonMotion>, Option<AnyElement>);
    /// 预绘制阶段算出的绘制参数，交给 paint 使用。
    type PrepaintState = PaintState;
    /// 稳定作用域隔离相邻按钮的短暂反馈owner。
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    /// 保留调用位置用于原生检查器。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 原Button生成真实布局、焦点与语义，适配器只注册无副作用的输入观察。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let owner = window.use_keyed_state("source-button-motion", cx, ButtonMotion::new);
        #[cfg(test)]
        if let Some(observer) = self.motion_observer.take() {
            observer(&owner);
        }
        if self.disabled || self.loading {
            // 先取消真实持键焦点，再清视觉记录，避免重新启用后keyup复活旧动作。
            let focus = owner.read(cx).keyboard_focus.clone();
            if focus.as_ref().is_some_and(|focus| focus.is_focused(window)) {
                window.blur(cx);
            }
            owner.update(cx, |state, cx| state.keyboard(None, cx));
        }
        let foreground = colors(
            self.variant,
            self.dark,
            self.disabled && !self.loading,
            false,
        )
        .2;
        let transparent = transparent_black();
        let mut veil_element = None;
        let mut button = self
            .button
            .take()
            .expect("原生组件仅布局一次")
            .disabled(self.disabled || self.loading)
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(transparent)
                    .hover(transparent)
                    .active(transparent)
                    .foreground(foreground.into()),
            )
            .bg(transparent)
            .border_1()
            .border_color(transparent)
            .text_color(foreground)
            .shadow_none();
        if let Some(size) = self.historical_size {
            let parameters = owner.update(cx, |state, cx| {
                let parameters = state
                    .content
                    .sample(self.loading, cx.background_executor().now());
                state.content_running = parameters.running;
                state.veil_active = parameters.veil_opacity() > 0.;
                parameters
            });
            let label = self.label.take().unwrap_or_default();
            if !self.explicit_name {
                button = button.accessibility_label(label.clone());
            }
            let has_icon = self.left_icon.is_some();
            veil_element = Some(veil::element(
                owner.downgrade(),
                parameters.veil_opacity(),
                parameters.veil_slide(),
            ));
            button = button
                .loading(false)
                .disabled(self.disabled || self.loading)
                .h(px(size.height()))
                .pl(px(if has_icon {
                    size.padding() / 1.5
                } else {
                    size.padding()
                }))
                .pr(px(size.padding()))
                .rounded(self.radius)
                .line_height(px(size.font_size()))
                .child(content::element(
                    label,
                    self.left_icon.take(),
                    self.left_icon_size,
                    size,
                    self.loading,
                    foreground,
                    parameters,
                ));
        } else if let Some(label) = self.label.take() {
            button = button.label(label);
        }
        if let Some(handler) = self.click_handler.take() {
            let weak = owner.downgrade();
            button = button.on_click(move |event, window, cx| {
                // 隐藏期间取消原生旧pending；新的真实down会恢复动作，AX仍走同一Button。
                if weak.upgrade().is_some_and(|owner| {
                    let state = owner.read(cx);
                    match event {
                        ClickEvent::Mouse(_) => !state.mouse_activation_cancelled,
                        ClickEvent::Keyboard(_) => !state.keyboard_activation_cancelled,
                        ClickEvent::Touch(_) => true,
                    }
                }) {
                    handler(event, window, cx);
                }
            });
        }
        let interactive = !(self.disabled || self.loading);
        if !interactive {
            super::native_control::mark_render_once_disabled(button.interactivity());
        }
        button = if interactive {
            button.cursor_pointer()
        } else {
            button.cursor_not_allowed()
        };
        let weak = owner.downgrade();
        button
            .interactivity()
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent);
        button.interactivity().on_hover(move |hovered, _, cx| {
            let _ = weak.update(cx, |state, cx| state.hover(*hovered, cx));
        });
        if interactive {
            let weak = owner.downgrade();
            button
                .interactivity()
                .capture_any_mouse_down(move |event, _, cx| {
                    if event.button == MouseButton::Left {
                        let _ = weak.update(cx, |state, cx| state.pointer(true, cx));
                    }
                });
            let weak = owner.downgrade();
            button
                .interactivity()
                .capture_any_mouse_up(move |event, _, cx| {
                    if event.button == MouseButton::Left {
                        let _ = weak.update(cx, |state, cx| state.pointer(false, cx));
                    }
                });
            let weak = owner.downgrade();
            button
                .interactivity()
                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |state, cx| state.pointer(false, cx));
                });
            let weak = owner.downgrade();
            button
                .interactivity()
                .capture_key_down(move |event, window, cx| {
                    let activation = matches!(event.keystroke.key.as_str(), "enter" | "space")
                        && !event.keystroke.modifiers.modified();
                    let focus = activation.then(|| window.focused(cx)).flatten();
                    let _ = weak.update(cx, |state, cx| {
                        // OS重复持键不能复活隐藏前的序列；只有新的真实按下解除键盘取消。
                        if event.is_held && state.keyboard_activation_cancelled { return; }
                        if focus.is_some() && !event.is_held { state.keyboard_activation_cancelled = false; }
                        state.keyboard(focus, cx);
                    });
                });
            let weak = owner.downgrade();
            button.interactivity().capture_key_up(move |_, _, cx| {
                let _ = weak.update(cx, |state, cx| state.keyboard(None, cx));
            });
        }
        let mut inner = button.render(window, cx).into_any_element();
        let layout = inner.request_layout(window, cx);
        (layout, (inner, owner, veil_element))
    }
    /// 位移同时作用于绘制、原生命中与辅助边界；保持原布局尺寸和周围内容位置。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> PaintState {
        let paint = request.1.update(cx, |state, cx| {
            let interactive = !(self.disabled || self.loading);
            let active = window.is_window_active();
            if !interactive || !window.is_visible() || (state.was_active && !active) {
                state.mouse_pressed = false;
                state.keyboard_focus = None;
            }
            if state
                .keyboard_focus
                .as_ref()
                .is_some_and(|focus| !focus.is_focused(window))
            {
                state.keyboard_focus = None;
            }
            state.was_active = active;
            let hovered = interactive && state.hovered;
            let (background, border, _) = colors(
                self.variant,
                self.dark,
                self.disabled && !self.loading,
                hovered,
            );
            let now = cx.background_executor().now();
            state.background.set_target(premultiply(background), now);
            state.border.set_target(premultiply(border), now);
            state.transform.set_target(
                if state.mouse_pressed || state.keyboard_focus.is_some() {
                    [1., 0.985]
                } else {
                    [0., 1.]
                },
                now,
            );
            let (background, bg_running) = state.background.sample(now);
            let (border, border_running) = state.border.sample(now);
            let (transform, moving) = state.transform.sample(now);
            let paint = PaintState {
                background: unpremultiply(background),
                border: unpremultiply(border),
                bounds,
                scale: transform[1] as f32,
                translation: point(px(0.), px(transform[0] as f32)),
                running: bg_running
                    || border_running
                    || moving
                    || state.content_running
                    || (self.historical_size.is_some() && self.loading),
            };
            if self.historical_size.is_some()
                && state.veil_active
                && window.is_visible()
                && !paint
                    .bounds
                    .intersect(&window.content_mask().bounds)
                    .is_empty()
            {
                let radius = self
                    .radius
                    .min(paint.bounds.size.height / 2.)
                    .min(paint.bounds.size.width / 2.);
                state.veil_geometry = Some((paint.bounds, radius, self.dark));
            } else {
                state.veil_geometry = None;
                state.veil.clear(window);
            }
            paint
        });
        window.with_element_transform(paint.bounds.center(), paint.scale, paint.translation, |window| {
            if let Some(veil) = &mut request.2 {
                // 遮罩保持原border-box纹理，整棵子树共用同一变换，按压不生成新尺寸纹理。
                veil.prepaint_as_root(
                    paint.bounds.origin,
                    size(
                        AvailableSpace::Definite(paint.bounds.size.width),
                        AvailableSpace::Definite(paint.bounds.size.height),
                    ),
                    window,
                    cx,
                );
            }
            request.0.prepaint(window, cx);
        });
        paint
    }
    /// 背景与边框位于原Button内容下方，焦点线、文字、加载状态继续由原生组件绘制。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        paint: &mut PaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_element_transform(paint.bounds.center(), paint.scale, paint.translation, |window| {
            let radius = self
                .radius
                .min(paint.bounds.size.height / 2.)
                .min(paint.bounds.size.width / 2.);
            let opacity = if self.historical_size.is_none() && self.loading && !self.disabled {
                0.8
            } else {
                1.
            };
            let mut background = paint.background;
            let mut border = paint.border;
            background.a *= opacity;
            border.a *= opacity;
            window.paint_quad(quad(
                paint.bounds,
                radius,
                background,
                Edges::all(px(1.)),
                border,
                BorderStyle::Solid,
            ));
            if let Some(veil) = &mut request.2 {
                veil.paint(window, cx);
            }
            if paint.running
                && window.is_visible()
                && !paint
                    .bounds
                    .intersect(&window.content_mask().bounds)
                    .is_empty()
            {
                window.request_animation_frame();
            }
            request.0.paint(window, cx);
        });
    }
}
