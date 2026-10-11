//! 历史品牌卡片的原生材质容器；布局、子控件及辅助语义仍交给 GPUI Div。

pub(in crate::app) mod motion;
mod palette;
mod raster;
mod rim;
mod shadow;
mod spotlight;
mod state;
mod textures;
mod table_row;
mod button;

pub(in crate::app) use table_row::BrandTableRow;
pub(in crate::app) use button::{HistoricalButtonSize, SurfaceButton, SurfaceButtonVariant, colors as surface_button_colors};

use gpui_kit::{
    A11ySubtreeBuilder, AnyElement, App, Bounds, Div, Element, ElementId, Entity, GlobalElementId,
    Hitbox, HoverListenerMode, Hsla, InspectorElementId, InteractiveElement, Interactivity,
    IntoElement, LayoutId, ParentElement, Pixels, Stateful, StatefulInteractiveElement,
    StyleRefinement, Styled, Window, accesskit, div, px,
};
use state::MaterialState;

/// 当前产品圆角与顶边几何；颜色改变不改变布局，宽度始终进入真实border-box。
#[derive(Clone, Copy)]
pub(in crate::app) struct Geometry {
    radius: Pixels,
    top_width: Pixels,
    top_accent: Option<Hsla>,
}

/// 历史静态类各自决定阴影与主题过渡，不把所有Paper或筛选条视为同一种卡片。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum StaticMaterial {
    Panel,
    Table,
    FilterBar,
}

/// 材质模式独立于布局，只有明确的spot-panel允许注册内部指针反馈。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MaterialKind {
    Spotlight,
    Static(StaticMaterial),
}

/// 默认r20、1px边框和品牌材质；显式radius/top_accent复用历史hero及slot合同。
/// 背景、圆角、边框、阴影及材质hover由组件保留；请勿用同名style覆盖材质。
pub(in crate::app) struct MaterialCard<const SPOTLIGHT: bool> {
    inner: Stateful<Div>,
    dark: bool,
    geometry: Geometry,
    material: MaterialKind,
    #[cfg(test)]
    owner_observer: Option<Box<dyn FnOnce(&Entity<MaterialState>)>>,
}

/// 需要历史hover光斑的卡片使用同一原生容器与完整材质。
pub(in crate::app) type SpotlightCard = MaterialCard<true>;

/// 静态品牌卡片复用布局、语义和资源生命周期，但不接管指针与子控件动作。
pub(in crate::app) type StaticCard = MaterialCard<false>;

impl SpotlightCard {
    /// ID须在父元素内稳定且唯一；主题值由当前宿主传入，不绑定Shell类型。
    pub fn new(id: impl Into<ElementId>, dark: bool) -> Self {
        Self::build(id, dark, MaterialKind::Spotlight)
    }

    /// CSS顶accent的宽度参与内容原点，hover只过渡颜色并保持同一边框几何。
    pub fn top_accent(mut self, width: Pixels, color: Hsla) -> Self {
        let value = f32::from(width);
        assert!(
            value.is_finite() && (0. ..=20.).contains(&value),
            "品牌卡片顶边只支持当前产品0..20px范围"
        );
        self.geometry.top_width = width;
        self.geometry.top_accent = Some(color);
        self.inner.style().border_widths.top = Some(width.into());
        self
    }
}

impl StaticCard {
    /// 调用者显式选择历史CSS类，padding和子布局仍由调用者设置。
    pub fn new(id: impl Into<ElementId>, dark: bool, material: StaticMaterial) -> Self {
        Self::build(id, dark, MaterialKind::Static(material))
    }
}

impl<const SPOTLIGHT: bool> MaterialCard<SPOTLIGHT> {
    /// 两类卡片共享唯一布局与绘制入口，区别只在声明过的材质合同。
    fn build(id: impl Into<ElementId>, dark: bool, material: MaterialKind) -> Self {
        Self {
            inner: div()
                .id(id)
                .relative()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .rounded(px(20.)),
            dark,
            material,
            geometry: Geometry {
                radius: px(20.),
                top_width: px(1.),
                top_accent: None,
            },
            #[cfg(test)]
            owner_observer: None,
        }
    }

    /// 当前产品圆角范围为0..20px；其负spread投影始终为矩形Gaussian，不接受虚假的大半径支持。
    pub fn radius(mut self, radius: Pixels) -> Self {
        let value = f32::from(radius);
        assert!(
            value.is_finite() && (0. ..=20.).contains(&value),
            "品牌卡片圆角只支持当前产品0..20px范围"
        );
        self.geometry.radius = radius;
        self.inner = self.inner.rounded(radius);
        self
    }

    /// 测试只接收owner句柄的瞬时借用，生产构建不保留观察接口或额外状态。
    #[cfg(test)]
    fn observe_material(mut self, observer: impl FnOnce(&Entity<MaterialState>) + 'static) -> Self {
        self.owner_observer = Some(Box::new(observer));
        self
    }
}

impl<const SPOTLIGHT: bool> Styled for MaterialCard<SPOTLIGHT> {
    /// 容器保留Div的尺寸、间距及文本样式扩展方式。
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.style()
    }
}

impl<const SPOTLIGHT: bool> ParentElement for MaterialCard<SPOTLIGHT> {
    /// 子控件使用原生输入与布局，不给整张卡片增加代理点击。
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements);
    }
}

impl<const SPOTLIGHT: bool> InteractiveElement for MaterialCard<SPOTLIGHT> {
    /// 可附加语义、焦点和点击；仅spot材质拥有内部hover时间线。
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.interactivity()
    }
}

impl<const SPOTLIGHT: bool> StatefulInteractiveElement for MaterialCard<SPOTLIGHT> {}

impl<const SPOTLIGHT: bool> IntoElement for MaterialCard<SPOTLIGHT> {
    type Element = Self;
    /// 容器本身就是实际参与布局和绘制的元素。
    fn into_element(self) -> Self {
        self
    }
}

impl<const SPOTLIGHT: bool> Element for MaterialCard<SPOTLIGHT> {
    type RequestLayoutState = (
        <Stateful<Div> as Element>::RequestLayoutState,
        Entity<MaterialState>,
    );
    type PrepaintState = Option<Hitbox>;

    /// 材质和子控件共享外层稳定ID，测试支持与辅助树仍识别真实卡片。
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.inner)
    }
    /// 保留Div构造来源，不伪造绘制节点的检查位置。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }
    /// 默认卡片是普通容器；调用方显式提供的语义继续有效。
    fn a11y_role(&self) -> Option<accesskit::Role> {
        self.inner.a11y_role()
    }
    /// 名称与状态完全复用原生Div的辅助属性。
    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.inner.write_a11y_info(node);
    }
    /// synthetic children不被材质层阻挡或复制。
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.inner, prepaint, builder);
    }

    /// keyed owner只在连续显示帧中存在，卸载自动移除GPU图集条目。
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = window.use_keyed_state("brand-material", cx, MaterialState::new);
        #[cfg(test)]
        if let Some(observer) = self.owner_observer.take() {
            observer(&state);
        }
        if SPOTLIGHT {
            let weak = state.downgrade();
            self.inner
                .interactivity()
                .hover_listener_mode(HoverListenerMode::InputModalityIndependent);
            self.inner
                .interactivity()
                .on_hover(move |hovered, window, cx| {
                    let _ = weak.update(cx, |state, cx| state.hover(*hovered, window, cx));
                });
            let weak = state.downgrade();
            self.inner
                .interactivity()
                .on_mouse_move(move |event, _, cx| {
                    let _ = weak.update(cx, |state, cx| {
                        if state.pointer != event.position {
                            state.pointer = event.position;
                            cx.notify();
                        }
                    });
                });
        }
        let (layout, frame) = self.inner.request_layout(id, inspector, window, cx);
        (layout, (frame, state))
    }

    /// 离屏时不创建材质纹理，已分配资源也立即释放。
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let hitbox = self
            .inner
            .prepaint(id, inspector, bounds, &mut request.0, window, cx);
        let hidden = self.inner.style().visibility == Some(gpui_kit::Visibility::Hidden);
        let visible = !hidden
            && window.is_visible()
            && !bounds
                .dilate(px(100.))
                .intersect(&window.content_mask().bounds)
                .is_empty();
        request.1.update(cx, |state, _| {
            state.visible = visible;
            if !visible {
                state.clear(window);
            }
        });
        hitbox
    }

    /// 绘制材质再交回Div绘制内容，光斑不成为输入目标。
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request.1.update(cx, |state, cx| {
            state.paint(bounds, self.dark, self.geometry, self.material, window, cx)
        });
        self.inner
            .paint(id, inspector, bounds, &mut request.0, prepaint, window, cx);
    }
}

#[cfg(test)]
mod static_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod variants_tests;
#[cfg(test)]
mod table_row_tests;
