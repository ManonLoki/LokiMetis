//! 数据源按钮只替换绘制与纯交互反馈，原生Button保留动作、焦点、加载和辅助语义。

use crate::app::surface::motion::CssMotion;
use gpui_kit::{
    AnyElement, App, BorderStyle, Bounds, ClickEvent, Context, Edges, Element, ElementId, Entity,
    FocusHandle, GlobalElementId, HoverListenerMode, InspectorElementId, InteractiveElement,
    IntoElement, LayoutId, MouseButton, ParentElement, Pixels, RenderOnce, Rgba, SharedString,
    StyleRefinement, Styled, Window,
    component::{
        Disableable, Sizable, Size,
        button::{Button, ButtonCustomVariant, ButtonVariants},
    },
    point, px, quad, transparent_black,
};
use std::time::Duration;

/// 每种变体对应历史Sources实际使用的Mantine按钮，而非全局主题替换。
#[derive(Clone, Copy)]
pub(super) enum SourceButtonVariant {
    FilledBrand,
    FilledRed,
    LightBrand,
    LightRed,
    SubtleBrand,
    SubtleRed,
    Default,
}

/// 按钮只拥有短暂反馈；三个CSS属性独立过渡，状态不进入偏好或业务服务。
pub(super) struct ButtonMotion {
    hovered: bool,
    mouse_pressed: bool,
    keyboard_focus: Option<FocusHandle>,
    was_active: bool,
    background: CssMotion<4>,
    border: CssMotion<4>,
    translation: CssMotion<1>,
}
impl ButtonMotion {
    /// 首次绘制采用静态目标，不制造挂载动画。
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = cx.background_executor().now();
        let duration = Duration::from_millis(200);
        Self {
            hovered: false,
            mouse_pressed: false,
            keyboard_focus: None,
            was_active: window.is_window_active(),
            background: CssMotion::with_duration(now, duration),
            border: CssMotion::with_duration(now, duration),
            translation: CssMotion::with_duration(now, duration),
        }
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
pub(super) struct SourceButton {
    button: Option<Button>,
    id: ElementId,
    variant: SourceButtonVariant,
    dark: bool,
    radius: Pixels,
    disabled: bool,
    loading: bool,
}
impl SourceButton {
    /// 控件ID须稳定且在父作用域唯一；状态由该原生元素生命周期限定。
    pub fn new(id: impl Into<ElementId>, variant: SourceButtonVariant, dark: bool) -> Self {
        let id = id.into();
        Self {
            button: Some(Button::new(id.clone())),
            id,
            variant,
            dark,
            radius: px(12.),
            disabled: false,
            loading: false,
        }
    }
    /// 可见文字继续作为默认可访问名称。
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.button = self.button.map(|button| button.label(label));
        self
    }
    /// 行动作的完整名称仍包含真实脱敏别名。
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
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
    /// 加载继续由原生组件阻止重复输入并绘制其指示器。
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
impl Disableable for SourceButton {
    /// 禁用交给原Button处理鼠标、键盘和焦点序列。
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.button = self.button.map(|button| button.disabled(disabled));
        self
    }
}
impl Styled for SourceButton {
    /// 宽高、间距及文字继续应用到同一个原生按钮。
    fn style(&mut self) -> &mut StyleRefinement {
        self.button
            .as_mut()
            .expect("按钮布局前仍持有原生组件")
            .style()
    }
}
impl Sizable for SourceButton {
    /// 字号交给组件内容层，避免根字号被其内层默认尺寸覆盖。
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.button = self.button.map(|button| button.with_size(size));
        self
    }
}
impl ParentElement for SourceButton {
    /// 自定义内容仍由原生组件居中并参与辅助树。
    fn extend(&mut self, children: impl IntoIterator<Item = AnyElement>) {
        self.button
            .as_mut()
            .expect("按钮布局前仍持有原生组件")
            .extend(children)
    }
}
impl IntoElement for SourceButton {
    type Element = Self;
    /// 直接进入原生元素生命周期，不增加代理容器或辅助节点。
    fn into_element(self) -> Self {
        self
    }
}

/// 预绘制缓存连续颜色和实际位移，绘制阶段不再改变命中几何。
pub(super) struct PaintState {
    background: Rgba,
    border: Rgba,
    bounds: Bounds<Pixels>,
    running: bool,
}

impl Element for SourceButton {
    type RequestLayoutState = (AnyElement, Entity<ButtonMotion>);
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
        let foreground = colors(
            self.variant,
            self.dark,
            self.disabled && !self.loading,
            false,
        )
        .2;
        let transparent = transparent_black();
        let mut button = self
            .button
            .take()
            .expect("原生组件仅布局一次")
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
        let mut inner = button.render(window, cx).into_any_element();
        let layout = inner.request_layout(window, cx);
        (layout, (inner, owner))
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
            PaintState {
                background: unpremultiply(background),
                border: unpremultiply(border),
                bounds: Bounds::new(
                    bounds.origin + point(px(0.), px(translation[0] as f32)),
                    bounds.size,
                ),
                running: bg_running || border_running || moving,
            }
        });
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
        let opacity = if self.loading && !self.disabled {
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

/// 复刻历史AppThemeProvider与其实际Mantine 9.6解析；light混色值未传scheme，亮暗均取色阶6。
fn colors(
    variant: SourceButtonVariant,
    dark: bool,
    disabled: bool,
    hovered: bool,
) -> (Rgba, Rgba, Rgba) {
    let rgb = |hex| Rgba::from(gpui_kit::rgb(hex));
    let transparent = rgb(0).opacity(0.);
    if disabled {
        return (
            rgb(if dark { 0x2e2e2e } else { 0xe9ecef }),
            transparent,
            rgb(if dark { 0x696969 } else { 0xadb5bd }),
        );
    }
    let red = matches!(
        variant,
        SourceButtonVariant::FilledRed
            | SourceButtonVariant::LightRed
            | SourceButtonVariant::SubtleRed
    );
    let color = rgb(if red { 0xfa5252 } else { 0xc43b2d });
    let foreground = rgb(match (dark, red) {
        (false, false) => 0x6b1e18,
        (true, false) => 0xfff2ee,
        (false, true) => 0xc92a2a,
        (true, true) => 0xfff5f5,
    });
    match variant {
        SourceButtonVariant::FilledBrand | SourceButtonVariant::FilledRed => (
            rgb(match (red, dark, hovered) {
                (false, false, false) | (false, true, true) => 0xc43b2d,
                (false, false, true) => 0xa52f25,
                (false, true, false) => 0xd95a44,
                (true, false, false) | (true, true, true) => 0xfa5252,
                (true, false, true) => 0xf03e3e,
                (true, true, false) => 0xff6b6b,
            }),
            transparent,
            rgb(0xffffff),
        ),
        SourceButtonVariant::LightBrand | SourceButtonVariant::LightRed => (
            color.opacity(if hovered { 0.15 } else { 0.09 }),
            color.opacity(0.26),
            foreground,
        ),
        SourceButtonVariant::SubtleBrand | SourceButtonVariant::SubtleRed => (
            if !hovered {
                transparent
            } else if !dark {
                rgb(if red { 0xffc9c9 } else { 0xf8c2b1 })
            } else {
                let base = rgb(if red { 0xc92a2a } else { 0x6b1e18 });
                Rgba {
                    r: (base.r * 255. * 0.7).round() / 255.,
                    g: (base.g * 255. * 0.7).round() / 255.,
                    b: (base.b * 255. * 0.7).round() / 255.,
                    a: 1.,
                }
            },
            transparent,
            foreground,
        ),
        SourceButtonVariant::Default => (
            rgb(match (dark, hovered) {
                (false, false) => 0xffffff,
                (false, true) => 0xf8f9fa,
                (true, false) => 0x2e2e2e,
                (true, true) => 0x3b3b3b,
            }),
            rgb(if dark { 0x424242 } else { 0xced4da }),
            rgb(if dark { 0xffffff } else { 0x000000 }),
        ),
    }
}

/// 透明颜色按预乘sRGB插值，避免透明端点出现深色晕染。
fn premultiply(color: Rgba) -> [f64; 4] {
    [
        f64::from(color.r * color.a),
        f64::from(color.g * color.a),
        f64::from(color.b * color.a),
        f64::from(color.a),
    ]
}
/// GPUI quad接收非预乘颜色，零alpha端点保持精确透明。
fn unpremultiply(color: [f64; 4]) -> Rgba {
    if color[3] <= f64::EPSILON {
        return Rgba {
            r: 0.,
            g: 0.,
            b: 0.,
            a: 0.,
        };
    }
    Rgba {
        r: (color[0] / color[3]) as f32,
        g: (color[1] / color[3]) as f32,
        b: (color[2] / color[3]) as f32,
        a: color[3] as f32,
    }
}

#[cfg(test)]
mod tests;
