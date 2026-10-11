//! 真实主线程只读比较现代 selector 准入与 legacy AXValue 可写标志，不创建宿主或调用 setter。
#![allow(dead_code)]
#[path = "../upstream/src/context.rs"]
mod context;
#[path = "../upstream/src/filters.rs"]
mod filters;
include!(concat!(env!("OUT_DIR"), "/node.rs"));
#[path = "../upstream/src/util.rs"]
mod util;

use accesskit::{Action, ActionRequest, Node, NodeId, Role, Toggled, Tree, TreeId, TreeUpdate};
use objc2::{msg_send, rc::WeakId, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, ns_string};
use std::rc::Rc;

/// 探针仅允许查询，任何动作提交都表明越过测试边界。
struct NoopContext;
impl context::ActionHandlerNoMut for NoopContext {
    /// 读取准入与可写标志不应触发业务动作。
    fn do_action(&self, _: ActionRequest) {
        panic!("readonly probe unexpectedly dispatched an action");
    }
}

/// 与产品生成模块相同的节点，装配到空视图弱引用且不启动 NSApplication。
fn with_node(data: Node, check: impl FnOnce(&node::PlatformNode)) {
    let marker = MainThreadMarker::new().expect("readonly probe must run on real main thread");
    objc2::rc::autoreleasepool(|_| {
        let local = NodeId(1);
        let tree = accesskit_consumer::Tree::new(
            TreeUpdate {
                nodes: vec![(local, data)],
                tree: Some(Tree::new(local)),
                tree_id: TreeId::ROOT,
                focus: local,
            },
            false,
        );
        let id = tree.state().root_id();
        let context = context::Context::new(
            WeakId::<NSView>::default(), tree, Rc::new(NoopContext), marker,
        );
        let platform = node::PlatformNode::new(Rc::downgrade(&context), id);
        check(&platform);
    });
}

/// 静态样本覆盖不可写角色，以及明确声明 SetValue 的可编辑对照。
fn samples() -> Vec<(&'static str, Node, bool)> {
    let mut label = Node::new(Role::Label);
    label.set_label("Read-only label");
    label.set_value("Read-only label value");
    let mut heading = Node::new(Role::Heading);
    heading.set_label("Read-only heading");
    heading.set_level(2);
    let mut button = Node::new(Role::Button);
    button.set_label("Action button");
    button.add_action(Action::Click);
    let mut input = Node::new(Role::TextInput);
    input.set_value("Editable input");
    input.add_action(Action::SetValue);
    let mut switch = Node::new(Role::Switch);
    switch.set_toggled(Toggled::False);
    switch.add_action(Action::Click);
    let mut value_switch = switch.clone();
    value_switch.add_action(Action::SetValue);
    vec![
        ("Label", label, false),
        ("Heading", heading, false),
        ("Button", button, false),
        ("TextInput(SetValue)", input, true),
        ("Switch(Click)", switch, false),
        ("Switch(SetValue)", value_switch, true),
    ]
}

/// 自定义主线程 harness 保留实际结果，不把尚未修复的 legacy 行为固定为期望。
fn main() {
    let mut mismatches = Vec::new();
    let mut legacy_missing = Vec::new();
    for (name, data, expected_modern) in samples() {
        with_node(data, |platform| {
            // SAFETY: 仅查询平台节点继承的 BOOL 属性方法及 selector 准入，不调用任何 setter。
            let (modern, has_legacy, implemented): (bool, bool, bool) = unsafe {
                (
                    msg_send![platform, isAccessibilitySelectorAllowed: sel!(setAccessibilityValue:)],
                    msg_send![platform, respondsToSelector: sel!(accessibilityIsAttributeSettable:)],
                    msg_send![platform, respondsToSelector: sel!(setAccessibilityValue:)],
                )
            };
            let legacy: Option<bool> = has_legacy.then(|| {
                // SAFETY: 已确认 selector 存在，只读取 AXValue 属性可写标志。
                unsafe { msg_send![platform, accessibilityIsAttributeSettable: ns_string!("AXValue")] }
            });
            assert_eq!(modern, expected_modern, "unexpected modern setter policy for {name}");
            assert!(implemented, "product setter selector should be implemented for {name}");
            println!("{name}: modern_allowed={modern} legacy_AXValue_settable={legacy:?} setter_implemented={implemented}");
            if legacy.is_some_and(|value| value != modern) { mismatches.push(name); }
            if legacy.is_none() { legacy_missing.push(name); }
        });
    }
    println!("modern/legacy mismatches: {} {:?}", mismatches.len(), mismatches);
    println!("legacy selector unavailable: {} {:?}", legacy_missing.len(), legacy_missing);
}
