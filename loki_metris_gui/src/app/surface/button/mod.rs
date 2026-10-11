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
    InteractiveElement, IntoElement, LayoutId, MouseButton, ParentElement, Pixels, Point, RenderOnce,
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
#[cfg(test)]
mod press_tests;
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
    mouse_activation_cancelled: bool,
    keyboard_activation_cancelled: bool,
    keyboard_focus: Option<FocusHandle>,
    was_active: bool,
    background: CssMotion<4>,
    border: CssMotion<4>,
    transform: CssMotion<2>,
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
                state.mouse_activation_cancelled = true;
                state.keyboard_activation_cancelled = true;
                state.mouse_pressed = false;
                state.keyboard_focus = None;
                state.transform.jump_to([0., 1.], cx.background_executor().now());
                state.veil_geometry = None;
                state.veil.clear(window);
            } else {
                cx.notify();
            }
        });
        Self {
            hovered: false,
            mouse_pressed: false,
            mouse_activation_cancelled: false,
            keyboard_activation_cancelled: false,
            keyboard_focus: None,
            was_active: window.is_window_active(),
            background: CssMotion::with_duration(now, duration),
            border: CssMotion::with_duration(now, duration),
            transform: CssMotion::with_duration(now, duration),
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
        if value { self.mouse_activation_cancelled = false; }
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
    click_handler: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
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
            click_handler: None,
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
        self.click_handler = Some(Box::new(handler));
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

/// 预绘制缓存连续颜色和中心仿射，绘制及命中共享同一时间线。
pub(in crate::app) struct PaintState {
    background: Rgba,
    border: Rgba,
    bounds: Bounds<Pixels>,
    scale: f32,
    translation: Point<Pixels>,
    running: bool,
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - button_scope.rs：稳定作用域隔离相邻按钮的短暂反馈owner。
include!("button_scope.rs");
