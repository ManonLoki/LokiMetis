//! 仅在独立测试进程主线程调用 AX selectors；不创建 App、窗口或视图，也不发送通知。
#![allow(dead_code)]
#[path = "../upstream/src/context.rs"]
mod context;
#[path = "../upstream/src/filters.rs"]
mod filters;
// 直接编译与产品库相同的生成节点模块，不加入生产测试包装。
include!(concat!(env!("OUT_DIR"), "/node.rs"));
#[path = "../upstream/src/util.rs"]
mod util;

use accesskit::{ActionRequest, Node, NodeId, Role, Tree, TreeId, TreeUpdate};
use objc2::{
    msg_send, msg_send_id,
    rc::{Id, WeakId},
    sel,
};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSArray, NSNumber, NSObject, NSString, ns_string};
use std::rc::Rc;

/// 测试上下文不派发任何用户动作。
struct Noop;
impl context::ActionHandlerNoMut for Noop {
    /// 只测试属性读取，不允许测试意外提交动作。
    fn do_action(&self, _: ActionRequest) {
        panic!("unexpected action");
    }
}

/// 用真实主线程标记和空视图弱引用装配节点，避免注册缓存或宿主生命周期副作用。
fn with_node(data: Node, check: impl FnOnce(&node::PlatformNode)) {
    let marker = MainThreadMarker::new().expect("probe must run on real main thread");
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
        let context =
            context::Context::new(WeakId::<NSView>::default(), tree, Rc::new(Noop), marker);
        let platform = node::PlatformNode::new(Rc::downgrade(&context), id);
        check(&platform);
    });
}

/// 经 Objective-C 的真实 role selector 读取平台角色。
fn role(platform: &node::PlatformNode) -> Id<NSString> {
    // SAFETY: 对象实现返回 NSString 的 NSAccessibility role getter。
    unsafe { msg_send_id![platform, accessibilityRole] }
}

/// 经真实 legacy selector 读取完整属性列表。
fn names(platform: &node::PlatformNode) -> Id<NSArray<NSString>> {
    // SAFETY: 对象继承 NSAccessibilityElement，getter 返回 NSString 数组。
    unsafe { msg_send_id![platform, accessibilityAttributeNames] }
}

/// 经真实 legacy selector 读取可空对象属性。
fn value(platform: &node::PlatformNode, attribute: &NSString) -> Option<Id<NSObject>> {
    // SAFETY: 与 NSObject NSAccessibility category 的对象返回类型一致。
    unsafe { msg_send_id![platform, accessibilityAttributeValue: attribute] }
}

/// 验证系统实际可读的 NSNumber 层级，避免只检查 Rust 分支。
fn level(platform: &node::PlatformNode) -> Option<usize> {
    // SAFETY: 正层级按协议返回 NSNumber，其余节点返回 nil。
    let result: Option<Id<NSNumber>> = unsafe {
        msg_send_id![platform, accessibilityAttributeValue: ns_string!("AXHeadingLevel")]
    };
    result.map(|number| number.as_usize())
}

/// 检查 native selector 的准入结果。
fn attribute_selector_allowed(platform: &node::PlatformNode) -> bool {
    // SAFETY: 对象实现 isAccessibilitySelectorAllowed: 的 BOOL 返回方法。
    unsafe {
        msg_send![platform, isAccessibilitySelectorAllowed: sel!(accessibilityAttributeValue:)]
    }
}

/// 二级、三级标题均返回原生角色、唯一层级声明和正确数字。
fn heading_positive_levels() {
    for expected in [2, 3] {
        let mut data = Node::new(Role::Heading);
        data.set_level(expected);
        with_node(data, |platform| {
            assert_eq!(role(platform).as_ref(), ns_string!("AXHeading"));
            let list = names(platform);
            assert_eq!(
                list.to_vec()
                    .iter()
                    .filter(|name| **name == ns_string!("AXHeadingLevel"))
                    .count(),
                1
            );
            assert_eq!(level(platform), Some(expected));
            // SAFETY: 正数标题层级的原生 Value 应为 NSNumber。
            let number: Option<Id<NSNumber>> = unsafe { msg_send_id![platform, accessibilityValue] };
            let number = number.expect("native Heading value");
            assert_eq!(number.as_usize(), expected);
            // SAFETY: NSNumber 的 objCType 返回对象生命周期内有效且以零结尾的编码。
            let encoding = unsafe { std::ffi::CStr::from_ptr(number.objCType().as_ptr()) };
            assert!(!matches!(encoding.to_bytes(), b"f" | b"d"), "Heading level must remain an integer");
            assert!(attribute_selector_allowed(platform));
        });
    }
}

/// 未提供层级和零层级不能声明或返回 AXHeadingLevel。
fn heading_missing_or_zero_level() {
    for expected in [None, Some(0)] {
        let mut data = Node::new(Role::Heading);
        if let Some(expected) = expected {
            data.set_level(expected);
        }
        with_node(data, |platform| {
            assert_eq!(role(platform).as_ref(), ns_string!("AXHeading"));
            assert!(
                !names(platform)
                    .to_vec()
                    .contains(&ns_string!("AXHeadingLevel"))
            );
            assert_eq!(level(platform), None);
            // SAFETY: 未提供可用值的节点仍返回 nil，层级不能泄露到其它角色。
            let native_value: Option<Id<NSObject>> = unsafe { msg_send_id![platform, accessibilityValue] };
            assert!(native_value.is_none());
            assert!(!attribute_selector_allowed(platform));
        });
    }
}

/// 非标题即使携带层级，也不能冒充标题或泄露层级属性。
fn non_heading_roles_unchanged() {
    for (input_role, native_role) in [(Role::Label, "AXStaticText"), (Role::Button, "AXButton")] {
        let mut data = Node::new(input_role);
        data.set_level(2);
        with_node(data, |platform| {
            assert_eq!(role(platform).to_string(), native_role);
            assert!(
                !names(platform)
                    .to_vec()
                    .contains(&ns_string!("AXHeadingLevel"))
            );
            assert_eq!(level(platform), None);
            // SAFETY: 未提供可用值的节点仍返回 nil，层级不能泄露到其它角色。
            let native_value: Option<Id<NSObject>> = unsafe { msg_send_id![platform, accessibilityValue] };
            assert!(native_value.is_none());
            assert!(!attribute_selector_allowed(platform));
        });
    }
}

/// 标题新增层级回退不覆盖原有显式文本、数值或开关状态。
fn heading_explicit_values_preserved() {
    let mut text = Node::new(Role::Heading);
    text.set_level(2);
    text.set_value("explicit heading value");
    with_node(text, |platform| {
        // SAFETY: 显式文本按上游约定返回 NSString。
        let actual: Option<Id<NSString>> = unsafe { msg_send_id![platform, accessibilityValue] };
        assert_eq!(actual.unwrap().to_string(), "explicit heading value");
    });
    let mut numeric = Node::new(Role::Heading);
    numeric.set_level(2);
    numeric.set_numeric_value(7.5);
    with_node(numeric, |platform| {
        // SAFETY: 显式数值按上游约定返回 NSNumber。
        let actual: Option<Id<NSNumber>> = unsafe { msg_send_id![platform, accessibilityValue] };
        assert_eq!(actual.unwrap().doubleValue(), 7.5);
    });
    let mut toggled = Node::new(Role::Heading);
    toggled.set_level(2);
    toggled.set_toggled(accesskit::Toggled::False);
    with_node(toggled, |platform| {
        // SAFETY: 显式开关状态按上游约定返回 NSNumber。
        let actual: Option<Id<NSNumber>> = unsafe { msg_send_id![platform, accessibilityValue] };
        assert!(!actual.unwrap().as_bool());
    });
}

/// 两种已有盲文属性仍返回 NSString，且 selector 准入保持成立。
fn braille_attributes_preserved() {
    let mut data = Node::new(Role::Label);
    data.set_braille_label("盲文标题");
    data.set_braille_role_description("盲文角色");
    with_node(data, |platform| {
        for (attribute, expected) in [
            ("AXBrailleLabel", "盲文标题"),
            ("AXBrailleRoleDescription", "盲文角色"),
        ] {
            // SAFETY: 已有盲文属性按协议返回 NSString。
            let result: Option<Id<NSString>> = unsafe {
                msg_send_id![platform, accessibilityAttributeValue: &*NSString::from_str(attribute)]
            };
            assert_eq!(result.unwrap().to_string(), expected);
        }
        assert!(attribute_selector_allowed(platform));
    });
}

/// Heading 同时有盲文时，两种值可并存而不覆盖层级。
fn heading_and_braille_coexist() {
    let mut data = Node::new(Role::Heading);
    data.set_level(3);
    data.set_braille_label("Heading braille");
    with_node(data, |platform| {
        assert_eq!(level(platform), Some(3));
        // SAFETY: 已有盲文属性按协议返回 NSString。
        let braille: Option<Id<NSString>> = unsafe {
            msg_send_id![platform, accessibilityAttributeValue: ns_string!("AXBrailleLabel")]
        };
        assert_eq!(braille.unwrap().to_string(), "Heading braille");
    });
}

/// 比较真实父类列表和标准属性值，保留系统实际行为而不假设 legacy 桥总会生成字符串。
fn standard_parent_attribute_fallback() {
    for input_role in [Role::Heading, Role::Label, Role::Button] {
        let mut data = Node::new(input_role);
        data.set_level(2);
        with_node(data, |platform| {
            // SAFETY: 在相同对象上直接调用声明的父类方法，读取其原始属性列表。
            let inherited: Option<Id<NSArray<NSString>>> =
                unsafe { msg_send_id![super(platform), accessibilityAttributeNames] };
            let mut expected: Vec<String> = inherited
                .as_ref()
                .map(|list| list.to_vec().iter().map(|name| name.to_string()).collect())
                .unwrap_or_default();
            if input_role == Role::Heading && !expected.iter().any(|name| name == "AXHeadingLevel")
            {
                expected.push("AXHeadingLevel".to_string());
            }
            let actual: Vec<String> = names(platform)
                .to_vec()
                .iter()
                .map(|name| name.to_string())
                .collect();
            assert_eq!(actual, expected);
            // SAFETY: 标准 legacy 属性返回可空 NSObject，显式与同一父类调用比较。
            let parent: Option<Id<NSObject>> = unsafe {
                msg_send_id![super(platform), accessibilityAttributeValue: ns_string!("AXRole")]
            };
            let actual = value(platform, ns_string!("AXRole"));
            match (actual, parent) {
                (None, None) => {}
                (Some(actual), Some(parent)) => {
                    // SAFETY: NSObject 的 isEqual: 接受任意 Objective-C 对象并返回 BOOL。
                    let equal: bool = unsafe { msg_send![&*actual, isEqual: &*parent] };
                    assert!(equal);
                }
                _ => panic!("parent attribute fallback changed"),
            }
        });
    }
}

/// 未知属性不产生伪造的标题层级值。
fn unsupported_attribute_remains_empty() {
    let mut data = Node::new(Role::Heading);
    data.set_level(2);
    with_node(data, |platform| {
        assert!(value(platform, ns_string!("AXLokiMetrisUnsupportedAttribute")).is_none());
    });
}

/// 自定义 harness 保持主线程执行且逐项报告，不启动 NSApplication 或 GUI。
fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let cases: &[(&str, fn())] = &[
        ("heading_positive_levels", heading_positive_levels),
        (
            "heading_missing_or_zero_level",
            heading_missing_or_zero_level,
        ),
        ("non_heading_roles_unchanged", non_heading_roles_unchanged),
        ("heading_explicit_values_preserved", heading_explicit_values_preserved),
        ("braille_attributes_preserved", braille_attributes_preserved),
        ("heading_and_braille_coexist", heading_and_braille_coexist),
        (
            "standard_parent_attribute_fallback",
            standard_parent_attribute_fallback,
        ),
        (
            "unsupported_attribute_remains_empty",
            unsupported_attribute_remains_empty,
        ),
    ];
    let mut passed = 0;
    for (name, test) in cases
        .iter()
        .filter(|(name, _)| filter.is_empty() || name.contains(&filter))
    {
        test();
        println!("test {name} ... ok");
        passed += 1;
    }
    assert!(passed > 0, "test filter must select a non-empty suite");
    println!("result: {passed} passed; 0 failed (main-thread native AX selector probe)");
}
