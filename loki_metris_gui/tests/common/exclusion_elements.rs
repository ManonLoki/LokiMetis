// 精确排除回归共用的最小 Element 夹具：由集成测试与 sticky 单元测试 include!，各自提供 use 导入。

/// 真实父内容mask与owner Hitbox几何在排除scope内仍须完整。
#[derive(Clone)]
struct Observation {
    /// 登记的 Hitbox。
    hitbox: Hitbox,
    /// 绘制时的内容裁切。
    mask: ContentMask<Pixels>,
    /// 目标边界。
    bounds: Bounds<Pixels>,
}

/// 自有最小Element创建真实Hitbox、半透明quad及图片，并读取实际AX节点bounds。
struct PaintedTarget {
    /// 目标标签。
    label: &'static str,
    /// 每次绘制的观测记录。
    records: Rc<RefCell<Vec<Observation>>>,
    /// 最近读取的辅助节点边界。
    ax_bounds: Rc<RefCell<Option<gpui_kit::accesskit::Rect>>>,
    /// 目标绘制的图片；None 时只绘制 quad。
    image: Option<Arc<RenderImage>>,
}
impl IntoElement for PaintedTarget {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 无额外透明包装或假节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PaintedTarget {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段注册的命中区域，绘制时用于判定悬停。
    type PrepaintState = Hitbox;
    /// 同一真实控件拥有稳定元素身份。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        Some(self.label.into())
    }
    /// 测试元素无额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 辅助角色来自实际Element钩子，而不是手工构造TreeUpdate。
    fn a11y_role(&self) -> Option<Role> {
        Some(Role::Button)
    }
    /// 同一真实节点保留名称，范围由GPUI实际布局填写。
    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        node.set_label(self.label);
    }
    /// 固定逻辑100×100，仅用于明确观测半开边界与12px露出缝。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            size: size(px(100.).into(), px(100.).into()),
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }
    /// owner接收完整原Hitbox，Frame仅改变内部输入分片。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Hitbox {
        let mask = window.content_mask();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.records.borrow_mut().push(Observation {
            hitbox: hitbox.clone(),
            mask,
            bounds,
        });
        hitbox
    }
    /// 绘制真实半透明填充与原图；输入排除不得改变其bounds/UV/渲染mask。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut Hitbox,
        window: &mut Window,
        _: &mut App,
    ) {
        if let Some(image) = &self.image {
            window.paint_quad(fill(bounds, rgba(0xff000080)));
            window
                .paint_image(bounds, bounds, Corners::default(), image.clone(), 0, false)
                .unwrap();
        }
    }
    /// 原生实际AX节点bounds可读取，不增加任何观测辅助节点。
    fn a11y_synthetic_children(
        &mut self,
        _: &mut Hitbox,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        *self.ax_bounds.borrow_mut() = builder.parent_node().bounds();
    }
}

/// 只在实际child prepaint期间追加精确排除，保留正常布局/paint/AX调用。
struct ExclusionScope {
    /// 被包装的子元素。
    child: AnyElement,
    /// 子元素 prepaint 期间追加的排除矩形。
    excluded: Vec<Bounds<Pixels>>,
}
impl ExclusionScope {
    /// 每一层有自己的覆盖矩形列表，空列表不清除外层scope。
    fn new(child: impl IntoElement, excluded: Vec<Bounds<Pixels>>) -> Self {
        Self {
            child: child.into_any_element(),
            excluded,
        }
    }
}
impl IntoElement for ExclusionScope {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 不为scope制造可访问或焦点节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ExclusionScope {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段无需向绘制阶段传递状态。
    type PrepaintState = ();
    /// scope没有额外身份。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    /// scope没有额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 使用child实际LayoutId，不复制样式几何。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    /// 只影响实际输入区域，原内容mask留给图片可见性与绘制。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_hitbox_exclusions(&self.excluded, |window| self.child.prepaint(window, cx));
    }
    /// paint在scope之外执行，不能给半透明表面画实色遮罩。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// 对照legacy弹层和newparent的实际排除继承与跨帧几何replay。
struct DeferredTarget {
    /// 被包装的子元素。
    child: Option<AnyElement>,
    /// 使用 legacy 弹层路径。
    legacy: bool,
}
impl IntoElement for DeferredTarget {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// deferred无额外可访问节点。
    fn into_element(self) -> Self {
        self
    }
}
impl Element for DeferredTarget {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段无需向绘制阶段传递状态。
    type PrepaintState = ();
    /// 稳定身份属于实际child。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    /// 测试包装没有检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// deferred仍占真实正常流布局。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.as_mut().unwrap().request_layout(window, cx), ())
    }
    /// 两个实际公开入口必须保持不同的输入scope继承合同。
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let child = self.child.take().unwrap();
        let offset = window.element_offset();
        let mask = Some(window.content_mask());
        if self.legacy {
            window.defer_draw(child, offset, 1, mask);
        } else {
            window.defer_draw_in_parent(child, offset, 1, mask);
        }
    }
    /// 实际paint只由deferred流执行一次。
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}
