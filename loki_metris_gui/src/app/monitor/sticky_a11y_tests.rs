//! 从真实 AccessKit TreeUpdate 的 debug 序列化验证层级、文档顺序与模态隐藏。

use super::sticky_tests::setup;
use gpui_kit::{AppContext, ElementId, ScrollDelta, TestAppContext, Window, point, px, test::TestWindowExt};
use serde_json::Value;

/// 仅读取 GPUI 已构造的真实树，不从 test-support 元素属性推导辅助树。
struct ActualTree(Value);

/// Windows Label 名称来自真实 value；计数和事件变化不能只更新不可朗读的 label。
#[gpui_kit::test]
fn monitor_actual_a11y_labels_publish_dynamic_values(cx: &mut TestAppContext) {
    use crate::monitor::listener::{HookRelayLastEvent, HookRelayStatus};
    let (handle, shell, _writer) = setup(cx, 900.);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    for (locale, received, failed) in [("zh-CN", 5, 0), ("en-US", 42, 3)] {
        shell.update(cx, |this, cx| {
            this.locale = locale;
            this.monitor.route = super::state::Route::Workbench;
            this.monitor.status = Some(HookRelayStatus {
                received_count: received,
                failed_count: failed,
                last_event: Some(HookRelayLastEvent { tool: loki_metris_core::AiTool::Codex, hook_type: "SessionEnd".into() }),
                ..Default::default()
            });
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let tree = ActualTree::capture(window);
            for (element, suffix) in [("monitor-received", received.to_string()), ("monitor-failed", failed.to_string()), ("monitor-last-event", "Codex / SessionEnd".into())] {
                let aria = &tree.0["nodes"][tree.id(element)]["aria"];
                assert_eq!(aria["role"], "Label");
                let label = aria["label"].as_str().unwrap();
                assert!(!label.is_empty());
                assert!(label.ends_with(&suffix));
                assert_eq!(aria["value"].as_str(), Some(label), "{element} 的 Windows UIA Name 不能为空");
            }
        }).unwrap();
    }
}

impl ActualTree {
    /// 节点来自当前已完成的原生 Window 帧。
    fn capture(window: &Window) -> Self {
        assert!(window.is_a11y_active(), "真实辅助树必须已激活");
        Self(serde_json::from_str(&window.debug_a11y_tree_json().expect("真实 AccessKit 帧缺失")).unwrap())
    }

    /// 原生控件只应登记一次；使用实际 GPUI 调试溯源 ID 找到其 NodeId。
    fn id(&self, element: &str) -> &str {
        let expected = format!("{:?}", ElementId::from(element.to_owned()));
        let found: Vec<_> = self.0["nodes"].as_object().unwrap().iter()
            .filter(|(_, node)| node["element_id"].as_str() == Some(expected.as_str()))
            .map(|(id, _)| id.as_str()).collect();
        assert_eq!(found.len(), 1, "真实辅助树须恰有一个 {element} 节点");
        found[0]
    }

    /// Dialog role 来自产品现有真实 surface，不依赖焦点陷阱包装转发角色。
    fn role_id(&self, role: &str) -> &str {
        let found: Vec<_> = self.0["nodes"].as_object().unwrap().iter()
            .filter(|(_, node)| node["aria"]["role"].as_str() == Some(role))
            .map(|(id, _)| id.as_str()).collect();
        assert_eq!(found.len(), 1, "真实辅助树须恰有一个 {role} 节点；实际role：{:?}", self.roles());
        found[0]
    }

    /// 失败时报告真实原生角色，用于区分树丢失与控件采用更具体的 AccessKit role。
    fn roles(&self) -> std::collections::BTreeSet<&str> {
        self.0["nodes"].as_object().unwrap().values().filter_map(|node| node["aria"]["role"].as_str()).collect()
    }

    /// 按真实父节点 children 顺序遍历，不依赖生成节点的完成顺序或 JSON map 排序。
    fn walk(&self, id: &str, found: &mut Vec<String>) {
        assert!(!found.iter().any(|prior| prior == id), "辅助树不能有重复引用或环");
        found.push(id.to_owned());
        if let Some(children) = self.0["nodes"][id]["children"].as_array() {
            for child in children { self.walk(child.as_str().unwrap(), found); }
        }
    }

    /// 当前节点真实子树，用于验证 Hidden 是共同祖先而非单独复制给叶子。
    fn subtree(&self, id: &str) -> Vec<String> {
        let mut found = Vec::new();
        self.walk(id, &mut found);
        found
    }
}

/// Sticky 的绘制优先级不得改变 AX 文档顺序，滚动前后均保留每个原生控件一次。
#[gpui_kit::test]
fn monitor_sticky_actual_a11y_preserves_parent_and_document_order(cx: &mut TestAppContext) {
    let (handle, _, _writer) = setup(cx, 700.);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    cx.update_window(handle, |_, window, cx| {
        for scroll in [0., -180.] {
            window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(scroll))), cx);
            window.render_frame(cx);
            let tree = ActualTree::capture(window);
            let underlay = tree.id("application-underlay");
            assert_eq!(tree.0["nodes"][underlay]["aria"]["hidden"], false);
            let document = tree.subtree(underlay);
            let mut positions = Vec::new();
            for element in ["monitor-workbench-tab", "monitor-management-tab", "monitor-images-tab", "monitor-hooks-tab", "monitor-agent-Codex", "monitor-picker-trigger-idle", "monitor-picker-trigger-asking", "monitor-profile-save"] {
                let id = tree.id(element);
                positions.push(document.iter().position(|node| node == id).unwrap_or_else(|| panic!("{element} 逃出实际页面辅助父节点")));
            }
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "真实 AX children 必须保持页头→正文→保存：{positions:?}");
            let editors: Vec<_> = tree.0["nodes"].as_object().unwrap().iter()
                .filter(|(_, node)| node["aria"]["role"] == "MultilineTextInput").map(|(id, _)| id).collect();
            assert_eq!(editors.len(), 4, "四个原生 Textarea 各自只能有一个实际编辑辅助节点；实际role：{:?}", tree.roles());
            for editor in editors {
                let at = document.iter().position(|id| id == editor).expect("真实编辑节点不得逃出 underlay");
                assert!(at > positions[3] && at < positions[7], "真实编辑节点应保持正文位置");
            }
        }
    }).unwrap();
}

/// 原生 Dialog 打开时 Sticky 仍在真实 Hidden underlay 下；Dialog 自身不得被隐藏。
#[gpui_kit::test]
fn monitor_sticky_actual_a11y_remains_inside_hidden_modal_underlay(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, 700.);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    cx.update_window(handle, |_, window, cx| {
        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-180.))), cx);
        shell.update(cx, |shell, cx| shell.open_release_notes(window, cx));
        window.render_frame(cx);
        let tree = ActualTree::capture(window);
        let underlay = tree.id("application-underlay");
        assert_eq!(tree.0["nodes"][underlay]["aria"]["hidden"], true);
        let background = tree.subtree(underlay);
        for element in ["monitor-workbench-tab", "monitor-management-tab", "monitor-images-tab", "monitor-hooks-tab", "monitor-profile-save"] {
            assert!(background.iter().any(|id| id == tree.id(element)), "{element} 不能逃出实际 Hidden 父节点");
        }
        assert!(!background.iter().any(|id| id == tree.id("release-notes-close")), "真实模态关闭按钮须在Hidden背景外");
        let dialog = tree.role_id("Dialog");
        assert!(!background.iter().any(|id| id == dialog));
        assert_eq!(tree.0["nodes"][dialog]["aria"]["hidden"], false);
        assert_eq!(tree.0["nodes"][dialog]["aria"]["role"], "Dialog");
        assert!(tree.subtree(dialog).iter().any(|id| id == tree.id("release-notes-close")));
        window.press("escape", cx);
        window.render_frame(cx);
        let restored = ActualTree::capture(window);
        let underlay = restored.id("application-underlay");
        assert_eq!(restored.0["nodes"][underlay]["aria"]["hidden"], false);
        let document = restored.subtree(underlay);
        assert!(document.iter().any(|id| id == restored.id("monitor-profile-save")));
    }).unwrap();
}
