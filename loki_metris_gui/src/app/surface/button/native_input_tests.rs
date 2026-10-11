//! 共享按钮在加载或禁用期间取消原生持键，辅助动作与实际可交互状态一致。

use super::*;
use gpui_kit::{
    AppContext, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke, Render, TestAppContext,
    WindowBounds, WindowOptions, div, prelude::FluentBuilder, test::TestWindowExt,
};

/// 对比历史内容层及普通内容层，使用同一真实原生按钮和动作计数。
struct Probe {
    historical: bool,
    disabled: bool,
    loading: bool,
    clicks: usize,
}
impl Render for Probe {
    /// 不连接业务服务；只有真实keyup或点击能增加计数。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().tab_group().child(
            SurfaceButton::new("action", SurfaceButtonVariant::Default, false)
                .when(self.historical, |button| {
                    button.historical_size(HistoricalButtonSize::Small)
                })
                .label("导入")
                .disabled(self.disabled)
                .loading(self.loading)
                .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
        ).child(SurfaceButton::new("neighbor", SurfaceButtonVariant::Default, false).label("相邻按钮"))
    }
}

/// 读取绘制后唯一真实节点，避免初始空树或仅动作缺失冒充原生disabled状态。
fn button_node(window: &Window, id: &str) -> serde_json::Value {
    let tree: serde_json::Value =
        serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
    let nodes: Vec<_> = tree["nodes"].as_object().unwrap().values()
        .filter(|node| node["element_id"] == format!("Name({id:?})")).collect();
    assert_eq!(nodes.len(), 1, "原按钮必须只有一个真实节点：{id}");
    assert_eq!(nodes[0]["aria"]["role"], "Button");
    nodes[0].clone()
}

/// 单独派发按下或松开，保留原生pending状态而非直接调用动作回调。
fn key(window: &mut Window, cx: &mut App, down: bool) {
    let keystroke = Keystroke::parse("space").unwrap();
    if down {
        window.dispatch_event(
            KeyDownEvent {
                keystroke,
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    } else {
        window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
    }
    window.render_frame(cx);
}

/// 两种内容模式、禁用/加载四种情形均撤销旧持键，恢复后新的按键仍只激活一次。
#[gpui_kit::test]
fn shared_button_disable_or_loading_cancels_pending_keyboard_and_a11y_actions(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    for historical in [false, true] {
        for loading in [false, true] {
            let (handle, probe) = cx.update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            size(px(300.), px(160.)),
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |_, cx| {
                        cx.new(|_| Probe {
                            historical,
                            disabled: false,
                            loading: false,
                            clicks: 0,
                        })
                    },
                )
                .unwrap()
            });
            assert!(cx.simulate_accessibility_activation(handle).is_some());
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.focus_next(cx);
                window.render_frame(cx);
                let original = button_node(window, "action");
                assert_eq!(original["aria"]["disabled"], false);
                key(window, cx, true);
                assert_eq!(probe.read(cx).clicks, 0);
                probe.update(cx, |probe, cx| {
                    probe.disabled = !loading;
                    probe.loading = loading;
                    cx.notify();
                });
                window.render_frame(cx);
                let disabled = button_node(window, "action");
                assert_eq!(disabled["accesskit_id"], original["accesskit_id"]);
                assert_eq!(disabled["aria"]["disabled"], true, "禁用和加载都应标记真实节点");
                assert_eq!(button_node(window, "neighbor")["aria"]["disabled"], false);
                if let Some(actions) = disabled["aria"]["on_action"].as_array() {
                    assert!(
                        !actions
                            .iter()
                            .any(|action| action == "Click" || action == "Focus")
                    );
                }
                probe.update(cx, |probe, cx| {
                    probe.disabled = false;
                    probe.loading = false;
                    cx.notify();
                });
                window.render_frame(cx);
                let restored = button_node(window, "action");
                assert_eq!(restored["accesskit_id"], original["accesskit_id"]);
                assert_eq!(restored["aria"]["disabled"], false);
                window.focus_next(cx);
                window.render_frame(cx);
                key(window, cx, false);
                assert_eq!(probe.read(cx).clicks, 0, "状态恢复不能激活旧持键");
                window.press("space", cx);
                assert_eq!(probe.read(cx).clicks, 1);
            })
            .unwrap();
        }
    }
}
