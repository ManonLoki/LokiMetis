//! 真实 Window/AccessKit 回归：预约顺序、取消、嵌套和窄缓存生命周期。
//! 交由 Monitor 测试所有者移入独立模块；不创建原生窗口或系统服务。

use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ContentMask, Context, Element, Empty, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Render, Role, StyleRefinement,
    TestAppContext, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::{cell::Cell, collections::HashSet, rc::Rc};

/// 无身份包装只移动子项的实际 prepaint/paint，保留子 LayoutId 与输入所有权。
struct DeferredProbe {
    child: Option<AnyElement>,
    priority: usize,
    legacy: bool,
    cancel: bool,
    mask: Option<ContentMask<Pixels>>,
}

impl DeferredProbe {
    /// 默认启用新父上下文路径，测试可显式选择 legacy 或取消预约。
    fn new(child: impl IntoElement, priority: usize) -> Self {
        Self {
            child: Some(child.into_any_element()),
            priority,
            legacy: false,
            cancel: false,
            mask: None,
        }
    }
}

impl IntoElement for DeferredProbe {
    /// 自身即参与布局与绘制的元素，不再额外包装。
    type Element = Self;
    /// 包装本身不增加原生节点或样式盒。
    fn into_element(self) -> Self {
        self
    }
}

impl Element for DeferredProbe {
    /// 布局阶段无需向后续阶段传递状态。
    type RequestLayoutState = ();
    /// 预绘制阶段无需向绘制阶段传递状态。
    type PrepaintState = ();
    /// 不给预约创建额外可访问身份。
    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }
    /// 本地测试包装没有额外检查器来源。
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    /// 使用真实子项的布局，不能用另一套位置状态代替原生布局。
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.as_mut().unwrap().request_layout(window, cx), ())
    }
    /// 可取消事务只取消尚未执行的 deferred；实际节点不会提前 prepaint。
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
        let mask = self.mask.or(Some(window.content_mask()));
        let schedule = |window: &mut Window| {
            if self.legacy {
                window.defer_draw(child, offset, self.priority, mask);
            } else {
                window.defer_draw_in_parent(child, offset, self.priority, mask);
            }
        };
        if self.cancel {
            let cancelled: Result<(), ()> = window.transact(|window| {
                schedule(window);
                Err(())
            });
            assert!(cancelled.is_err());
        } else {
            schedule(window);
        }
    }
    /// 原生 deferred 流拥有子项 paint，包装不得重复绘制或登记输入。
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

/// 节点的 label 与 role 来自真实 GPUI Element，而不是测试快照属性。
fn leaf(label: &'static str) -> AnyElement {
    div()
        .id(label)
        .role(Role::Button)
        .aria_label(label)
        .size(px(20.))
        .into_any_element()
}

/// 此树仅解析已经完成的真实 AccessKit TreeUpdate。
struct ActualTree(Value);

impl ActualTree {
    /// 在实际帧结束后捕获当前辅助树，激活前不得使用旧 debug 数据。
    fn capture(window: &Window) -> Self {
        assert!(window.is_a11y_active());
        Self(serde_json::from_str(&window.debug_a11y_tree_json().expect("缺少实际 AX 帧")).unwrap())
    }
    /// 每个实际 label 必须唯一，不把重复或丢失节点归为成功。
    fn id(&self, label: &str) -> String {
        let ids: Vec<_> = self.0["nodes"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, node)| node["aria"]["label"].as_str() == Some(label))
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(ids.len(), 1, "真实 AX label 应恰有一个：{label}");
        ids[0].clone()
    }
    /// DFS 必须跟随真实 children 顺序，并拒绝重复引用或环。
    fn walk(&self, id: &str, seen: &mut HashSet<String>, labels: &mut Vec<String>) {
        assert!(seen.insert(id.to_owned()), "AX 不能重复引用同一节点");
        let node = &self.0["nodes"][id];
        assert!(node.is_object(), "children 引用了不存在的 AX 节点");
        if let Some(label) = node["aria"]["label"].as_str() {
            labels.push(label.to_owned());
        }
        if let Some(children) = node["children"].as_array() {
            for child in children {
                self.walk(child.as_str().unwrap(), seen, labels);
            }
        }
    }
    /// 验证完整子树，不只比较几个相对位置而遗漏额外占位节点。
    fn labels(&self, parent: &str) -> Vec<String> {
        let mut labels = Vec::new();
        self.walk(&self.id(parent), &mut HashSet::new(), &mut labels);
        labels
    }
    /// Hidden 必须保留在真实共同祖先，而非逐个伪造给控件。
    fn hidden(&self, label: &str) -> bool {
        self.0["nodes"][self.id(label)]["aria"]["hidden"]
            .as_bool()
            .unwrap()
    }
}

/// 静态探针保留每一帧同一构造合同，输入事件与辅助节点仍只登记一次。
struct OrderRoot {
    nested: bool,
}

impl Render for OrderRoot {
    /// 相反 priority 仅控制真正绘制层级，不得改变原文档子序列。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut cancelled = DeferredProbe::new(leaf("cancelled"), 2);
        cancelled.cancel = true;
        let first = if self.nested {
            div()
                .child(leaf("first-before"))
                .child(DeferredProbe::new(leaf("nested-one"), 9))
                .child(DeferredProbe::new(leaf("nested-two"), 1))
                .child(
                    div()
                        .id("nested-group")
                        .role(Role::Group)
                        .aria_label("nested-group")
                        .child(DeferredProbe::new(leaf("group-leaf"), 0)),
                )
                .child(leaf("first-after"))
                .into_any_element()
        } else {
            leaf("first")
        };
        div()
            .id("ordered-parent")
            .role(Role::Group)
            .aria_label("ordered-parent")
            .size_full()
            .flex()
            .flex_col()
            .child(leaf("before"))
            .child(DeferredProbe::new(first, 20))
            .child(cancelled)
            .child(DeferredProbe::new(Empty, 3))
            .child(DeferredProbe::new(leaf("second"), 1))
            .child(leaf("after"))
            .a11y_synthetic_children(|builder| builder.parent_node().set_hidden())
    }
}

/// 同父连续预约捕获时 children.len 相同，空与取消槽不能留下最终辅助节点。
#[gpui_kit::test]
fn monitor_sticky_api_adjacent_reverse_priority_empty_cancel_and_hidden(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| OrderRoot { nested: false });
    assert!(
        cx.simulate_accessibility_activation(handle.into())
            .is_some()
    );
    cx.update_window(handle.into(), |_, window, cx| {
        for _ in 0..3 {
            window.draw(cx).clear(cx);
            let tree = ActualTree::capture(window);
            assert!(tree.hidden("ordered-parent"));
            assert_eq!(
                tree.labels("ordered-parent"),
                ["ordered-parent", "before", "first", "second", "after"]
            );
            assert_eq!(
                tree.0["nodes"].as_object().unwrap().len(),
                6,
                "只能有窗口、父节点和四个真实控件"
            );
        }
    })
    .unwrap();
}

/// 同父嵌套预约与独立嵌套父节点均保持自然顺序，跨帧不复用上一帧槽身份。
#[gpui_kit::test]
fn monitor_sticky_api_nested_reservations_preserve_full_order(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| OrderRoot { nested: true });
    assert!(
        cx.simulate_accessibility_activation(handle.into())
            .is_some()
    );
    cx.update_window(handle.into(), |_, window, cx| {
        for _ in 0..3 {
            window.draw(cx).clear(cx);
            let tree = ActualTree::capture(window);
            assert_eq!(
                tree.labels("ordered-parent"),
                [
                    "ordered-parent",
                    "before",
                    "first-before",
                    "nested-one",
                    "nested-two",
                    "nested-group",
                    "group-leaf",
                    "first-after",
                    "second",
                    "after"
                ]
            );
            assert_eq!(tree.labels("nested-group"), ["nested-group", "group-leaf"]);
            assert_eq!(tree.0["nodes"].as_object().unwrap().len(), 11);
        }
    })
    .unwrap();
}

/// 仅用于观测真实 Render 次数的无节点缓存，证明 legacy 路径未全局失效。
struct UnrelatedCache {
    renders: Rc<Cell<usize>>,
}
impl Render for UnrelatedCache {
    /// 不产生 AX 节点，避免把既有普通缓存 AX 限制混入新预约语义。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        Empty
    }
}

/// 缓存视图可直接预约、在 legacy deferred 后代预约，或再包一层真实缓存父。
struct CacheNode {
    renders: Rc<Cell<usize>>,
    kind: usize,
    child: Option<gpui_kit::Entity<CacheNode>>,
}
impl Render for CacheNode {
    /// 保留同一实体与布局；真实计数区分重建和 prepaint/paint range replay。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        if let Some(child) = &self.child {
            return div()
                .id("cached-outer")
                .role(Role::Group)
                .aria_label("cached-outer")
                .child(
                    child
                        .clone()
                        .cached(StyleRefinement::default().size(px(100.))),
                )
                .into_any_element();
        }
        let content = div()
            .id("cached-body")
            .role(Role::Group)
            .aria_label("cached-body")
            .child(leaf("cache-before"))
            .child(DeferredProbe::new(leaf("cache-deferred"), 3))
            .child(leaf("cache-after"));
        if self.kind == 1 {
            let mut legacy = DeferredProbe::new(content, 1);
            legacy.legacy = true;
            legacy.into_any_element()
        } else {
            content.into_any_element()
        }
    }
}

/// 根本身正常重绘，相关缓存与无关缓存拥有不同实体和独立生命周期。
struct CacheRoot {
    relevant: gpui_kit::Entity<CacheNode>,
    unrelated: gpui_kit::Entity<UnrelatedCache>,
}
impl Render for CacheRoot {
    /// 外层不调用 refresh 或 notify；所有帧缓存是否复用由 GPUI 自己决定。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("cache-root")
            .role(Role::Group)
            .aria_label("cache-root")
            .size_full()
            .flex()
            .flex_col()
            .child(
                self.relevant
                    .clone()
                    .cached(StyleRefinement::default().size(px(120.))),
            )
            .child(
                self.unrelated
                    .clone()
                    .cached(StyleRefinement::default().size(px(20.))),
            )
    }
}

/// 共享的重绘计数器。
type Counter = Rc<Cell<usize>>;

/// 新窗口在实际 inactive 帧先形成缓存；后续不能用强制 refresh 的 render_frame。
fn cache_setup(
    cx: &mut TestAppContext,
    kind: usize,
    nested: bool,
) -> (gpui_kit::AnyWindowHandle, Counter, Counter, Counter) {
    let relevant = Rc::new(Cell::new(0));
    let outer = Rc::new(Cell::new(0));
    let unrelated = Rc::new(Cell::new(0));
    let captures = (relevant.clone(), outer.clone(), unrelated.clone());
    let handle = cx.add_window(move |_, cx| {
        let child = cx.new(|_| CacheNode {
            renders: relevant,
            kind,
            child: None,
        });
        let main = if nested {
            cx.new(|_| CacheNode {
                renders: outer,
                kind: 0,
                child: Some(child),
            })
        } else {
            child
        };
        CacheRoot {
            relevant: main,
            unrelated: cx.new(|_| UnrelatedCache { renders: unrelated }),
        }
    });
    cx.update_window(handle.into(), |_, window, cx| {
        assert!(!window.is_a11y_active());
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    assert!(captures.0.get() >= 1);
    let before = (captures.0.get(), captures.1.get(), captures.2.get());
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .unwrap();
    assert_eq!(
        (captures.0.get(), captures.1.get(), captures.2.get()),
        before,
        "inactive 必须真实复用缓存"
    );
    (handle.into(), captures.0, captures.1, captures.2)
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - sticky_api_tests_cache_cycle.rs：真实激活/停用回调跨帧切换，相关缓存重建、其它缓存仍复用，且无旧帧 token。；最小直接 opt-in 缓存必须覆盖 inactive→active→inactive→reactive 完整循环。；旧帧可达范围必须发现稍后 legacy deferred 内创建的 opt-in，而不是只扫描直接条目。
include!("sticky_api_tests_cache_cycle.rs");
