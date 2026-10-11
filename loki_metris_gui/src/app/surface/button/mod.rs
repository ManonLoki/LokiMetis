//! 历史可见按钮共享绘制与纯交互反馈，原生Button保留动作、焦点和辅助语义。

mod content;
mod palette;
mod veil;
mod veil_raster;
pub(in crate::app) use content::HistoricalButtonSize;
pub(in crate::app) use palette::colors;
use palette::{premultiply, unpremultiply};

use crate::app::surface::motion::CssMotion;
use gpui_kit::{
    AnyElement, App, AvailableSpace, BorderStyle, Bounds, ClickEvent, Context, Edges, Element,
    ElementId, Entity, FocusHandle, GlobalElementId, HoverListenerMode, InspectorElementId,
    InteractiveElement, IntoElement, LayoutId, MouseButton, ParentElement, Pixels, RenderOnce,
    Rgba, SharedString, StyleRefinement, Styled, Subscription, Window,
    component::{
        Disableable, Icon, Sizable, Size,
        button::{Button, ButtonCustomVariant, ButtonVariants},
    },
    point, px, quad, size, transparent_black,
};
use std::time::Duration;

#[cfg(test)]
mod content_tests;
#[cfg(test)]
mod native_input_tests;
#[cfg(all(test, target_os = "macos"))]
mod loading_metal_tests;
#[cfg(all(test, target_os = "macos"))]
mod veil_metal_tests;

/// 每种变体对应历史Sources实际使用的Mantine按钮，而非全局主题替换。
#[derive(Clone, Copy)]
pub(in crate::app) enum SurfaceButtonVariant {
    FilledBrand,
    FilledRed,
    LightBrand,
    LightRed,
    SubtleBrand,
    SubtleRed,
    Default,
}

/// 按钮只拥有短暂反馈；三个CSS属性独立过渡，状态不进入偏好或业务服务。
pub(in crate::app) struct ButtonMotion {
    hovered: bool,
    mouse_pressed: bool,
    keyboard_focus: Option<FocusHandle>,
    was_active: bool,
    background: CssMotion<4>,
    border: CssMotion<4>,
    translation: CssMotion<1>,
    content: content::LoadingMotion,
    content_running: bool,
    veil: veil::Veil,
    veil_active: bool,
    veil_geometry: Option<(Bounds<Pixels>, Pixels, bool)>,
    veil_cleanup_scheduled: bool,
    _subscriptions: Vec<Subscription>,
}
impl ButtonMotion {
    /// 首次绘制采用静态目标，不制造挂载动画。
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = cx.background_executor().now();
        let duration = Duration::from_millis(200);
        let release = cx.on_release_in(window, |state, window, _| state.veil.clear(window));
        let visibility = cx.observe_window_visibility(window, |state, visibility, window, cx| {
            if !visibility.is_visible() {
                state.veil_geometry = None;
                state.veil.clear(window);
            } else {
                cx.notify();
            }
        });
        Self {
            hovered: false,
            mouse_pressed: false,
            keyboard_focus: None,
            was_active: window.is_window_active(),
            background: CssMotion::with_duration(now, duration),
            border: CssMotion::with_duration(now, duration),
            translation: CssMotion::with_duration(now, duration),
            content: content::LoadingMotion::new(),
            content_running: false,
            veil: veil::Veil::default(),
            veil_active: false,
            veil_geometry: None,
            veil_cleanup_scheduled: false,
            _subscriptions: vec![release, visibility],
        }
    }
    /// 回调只捕获弱owner，卸载不会保活按钮或积累退休纹理。
    fn schedule_veil_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.veil_cleanup_scheduled || !self.veil.needs_cleanup() {
            return;
        }
        self.veil_cleanup_scheduled = true;
        let owner = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |state, _| {
                state.veil_cleanup_scheduled = false;
                state.veil.clear_retired(window);
            });
        });
    }
    /// 悬停只通知当前按钮；没有点击或传播副作用。
    fn hover(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.hovered != value {
            self.hovered = value;
            cx.notify();
        }
    }
    /// 指针按住与松开仅影响位置反馈，实际激活仍完全属于原生Button。
    fn pointer(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.mouse_pressed != value {
            self.mouse_pressed = value;
            cx.notify();
        }
    }
    /// 记录真实原生焦点，失焦后不能继续保留键盘按住反馈。
    fn keyboard(&mut self, focus: Option<FocusHandle>, cx: &mut Context<Self>) {
        if self.keyboard_focus != focus {
            self.keyboard_focus = focus;
            cx.notify();
        }
    }
}

/// 元素无额外辅助节点，内层继续使用原始控件ID与原生Button行为。
pub(in crate::app) struct SurfaceButton {
    button: Option<Button>,
    label: Option<SharedString>,
    explicit_name: bool,
    historical_size: Option<HistoricalButtonSize>,
    left_icon: Option<Icon>,
    left_icon_size: Pixels,
    id: ElementId,
    variant: SurfaceButtonVariant,
    dark: bool,
    radius: Pixels,
    disabled: bool,
    loading: bool,
    #[cfg(test)]
    motion_observer: Option<Box<dyn FnOnce(&Entity<ButtonMotion>)>>,
}
impl SurfaceButton {
    /// 控件ID须稳定且在父作用域唯一；状态由该原生元素生命周期限定。
    pub fn new(id: impl Into<ElementId>, variant: SurfaceButtonVariant, dark: bool) -> Self {
        let id = id.into();
        Self {
            button: Some(Button::new(id.clone())),
            label: None,
            explicit_name: false,
            historical_size: None,
            left_icon: None,
            left_icon_size: px(17.),
            id,
            variant,
            dark,
            radius: px(12.),
            disabled: false,
            loading: false,
            #[cfg(test)]
            motion_observer: None,
        }
    }
    /// 可见文字继续作为默认可访问名称。
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// 只有明确对齐的历史按钮采用内容层字号、内边距和加载动画。
    pub fn historical_size(mut self, size: HistoricalButtonSize) -> Self {
        self.historical_size = Some(size);
        self
    }
    /// 左图标参与真实布局；不通过字符或缺失资源占位。
    pub fn left_icon(mut self, icon: Icon) -> Self {
        self.left_icon = Some(icon);
        self
    }
    /// 个别历史工具栏图标为16px；默认17px保持已有来源页按钮尺寸。
    pub fn left_icon_size(mut self, size: Pixels) -> Self {
        self.left_icon_size = size;
        self
    }
    /// 行动作的完整名称仍包含真实脱敏别名。
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.explicit_name = true;
        self.button = self.button.map(|button| button.accessibility_label(label));
        self
    }
    /// 回调只绑定原生Button，不让周围行或卡片代理动作。
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.button = self.button.map(|button| button.on_click(handler));
        self
    }
    /// 原生组件阻止重复输入；历史模式绘制本地Loader，其余模式保留原指示器。
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self.button = self.button.map(|button| button.loading(loading));
        self
    }
    /// 半径与页面尺寸合同一致，绘制时按实际高度夹紧胶囊半径。
    pub fn rounded(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self.button = self.button.map(|button| button.rounded(radius));
        self
    }
}
impl Disableable for SurfaceButton {
    /// 禁用交给原Button处理鼠标、键盘和焦点序列。
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.button = self.button.map(|button| button.disabled(disabled));
        self
    }
}
impl Styled for SurfaceButton {
    /// 宽高、间距及文字继续应用到同一个原生按钮。
    fn style(&mut self) -> &mut StyleRefinement {
        self.button
            .as_mut()
            .expect("按钮布局前仍持有原生组件")
            .style()
    }
}
impl Sizable for SurfaceButton {
    /// 字号交给组件内容层，避免根字号被其内层默认尺寸覆盖。
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.button = self.button.map(|button| button.with_size(size));
        self
    }
}
impl ParentElement for SurfaceButton {
    /// 自定义内容仍由原生组件居中并参与辅助树。
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.button
            .as_mut()
            .expect("按钮布局前仍持有原生组件")
            .extend(children)
    }
}
impl IntoElement for SurfaceButton {
    type Element = Self;
    /// 直接进入原生元素生命周期，不增加代理容器或辅助节点。
    fn into_element(self) -> Self {
        self
    }
}

/// 预绘制缓存连续颜色和实际位移，绘制阶段不再改变命中几何。
pub(in crate::app) struct PaintState {
    background: Rgba,
    border: Rgba,
    bounds: Bounds<Pixels>,
    running: bool,
}

impl Element for SurfaceButton {
    type RequestLayoutState = (AnyElement, Entity<ButtonMotion>, Option<AnyElement>);
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
        let interactive = !(self.disabled || self.loading);
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
                    let _ = weak.update(cx, |state, cx| state.keyboard(focus, cx));
                });
            let weak = owner.downgrade();
            button.interactivity().capture_key_up(move |_, _, cx| {
                let _ = weak.update(cx, |state, cx| state.keyboard(None, cx));
            });
        }
        let mut inner = super::native_control::NativeControl::new(
            button.render(window, cx).into_element(),
            !interactive,
        ).into_any_element();
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
            state.translation.set_target(
                [if state.mouse_pressed || state.keyboard_focus.is_some() {
                    1.
                } else {
                    0.
                }],
                now,
            );
            let (background, bg_running) = state.background.sample(now);
            let (border, border_running) = state.border.sample(now);
            let (translation, moving) = state.translation.sample(now);
            let paint = PaintState {
                background: unpremultiply(background),
                border: unpremultiply(border),
                bounds: Bounds::new(
                    bounds.origin + point(px(0.), px(translation[0] as f32)),
                    bounds.size,
                ),
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
        if let Some(veil) = &mut request.2 {
            // 遮罩位于Kit content的overflow外，尺寸来自同一个实际border-box，不参与父布局。
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
        let offset = paint.bounds.origin - bounds.origin;
        window.with_element_offset(offset, |window| {
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
    }
}
