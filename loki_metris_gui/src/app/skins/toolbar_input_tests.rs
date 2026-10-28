//! 按下/松开分帧检查原生输入、空心焦点绘制与真实辅助树，避免直接调用动作。

use super::*;
use gpui_kit::{
    AppContext, Element, InputEvent, KeyDownEvent, KeyUpEvent, Keystroke, Role, TestAppContext,
    WindowVisibility, accesskit, test::TestWindowExt,
};

/// 保留完整的原生事件顺序，既能观察持键帧，也能验证keyup仅激活一次。
fn key(window: &mut Window, cx: &mut App, key: &'static str, down: bool) {
    let keystroke = Keystroke::parse(key).unwrap();
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

/// Space和Enter持键只下压1px；松键恢复并恰好激活一次，不保留动画帧。
#[gpui_kit::test]
fn refresh_keyboard_press_moves_one_pixel_then_activates_once(cx: &mut TestAppContext) {
    let (handle, probe) = super::tests::open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        assert_eq!(window.find("refresh").focused(), Some(true));
        let original = window.find("refresh").bounds();
        for (index, pressed) in ["space", "enter"].into_iter().enumerate() {
            key(window, cx, pressed, true);
            let held = window.find("refresh").bounds();
            assert_eq!(held.origin.y, original.origin.y + px(1.));
            assert_eq!(held.size, original.size);
            assert_eq!(probe.read(cx).clicks[1], index);
            key(window, cx, pressed, false);
            assert_eq!(window.find("refresh").bounds(), original);
            assert_eq!(probe.read(cx).clicks[1], index + 1);
        }
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(window.simulate_next_frame(cx), 0);
    })
    .unwrap();
}

/// 焦点离开再返回不能复活持键；禁用与隐藏也必须立即清除反馈。
#[gpui_kit::test]
fn refresh_keyboard_press_cancels_on_focus_aba_disable_and_hide(cx: &mut TestAppContext) {
    let (handle, probe) = super::tests::open(cx);
    let original = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.focus_next(cx);
            window.render_frame(cx);
            window.focus_next(cx);
            window.render_frame(cx);
            let original = window.find("refresh").bounds();
            key(window, cx, "space", true);
            window.focus_next(cx);
            window.render_frame(cx);
            window.focus_prev(cx);
            window.render_frame(cx);
            assert_eq!(window.find("refresh").focused(), Some(true));
            assert_eq!(window.find("refresh").bounds(), original);
            key(window, cx, "space", false);
            assert_eq!(probe.read(cx).clicks[1], 0);
            key(window, cx, "enter", true);
            probe.update(cx, |probe, cx| {
                probe.disabled = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(window.find("refresh").bounds(), original);
            key(window, cx, "enter", false);
            assert_eq!(probe.read(cx).clicks[1], 0);
            probe.update(cx, |probe, cx| {
                probe.disabled = false;
                cx.notify();
            });
            window.render_frame(cx);
            // 禁用会取消焦点；重新聚焦后也必须开始新的持键。
            window.focus_next(cx);
            window.render_frame(cx);
            window.focus_next(cx);
            window.render_frame(cx);
            assert_eq!(window.find("refresh").focused(), Some(true));
            key(window, cx, "space", true);
            assert_eq!(
                window.find("refresh").bounds().origin.y,
                original.origin.y + px(1.)
            );
            original
        })
        .unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("refresh").bounds(), original);
    })
    .unwrap();
}

/// 未选中透明标签的焦点环只能绘制边框，不能给整个按钮内部填色。
#[gpui_kit::test]
fn host_keyboard_focus_ring_has_transparent_center(cx: &mut TestAppContext) {
    let (handle, _) = super::tests::open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        assert_eq!(window.find("host").focused(), Some(true));
        window.press("a", cx);
        window.render_frame(cx);
        let bounds = window.find("host").bounds();
        let quads = window.painted_quads();
        let ring = quads
            .iter()
            .find(|quad| {
                quad.bounds == bounds.dilate(px(3.)).scale(window.scale_factor())
                    && quad.border_widths.top.0 == 3. * window.scale_factor()
            })
            .expect("实际绘制必须包含3px向外焦点边框");
        assert_eq!(Rgba::from(ring.background.as_solid().unwrap()).a, 0.);
        assert!(Rgba::from(ring.border_color).a > 0.);
        assert!(!quads.iter().any(|quad| {
            quad.bounds == bounds.scale(window.scale_factor())
                && quad
                    .background
                    .as_solid()
                    .is_some_and(|color| Rgba::from(color).a > 0.)
        }));
    })
    .unwrap();
}

/// 原生节点转发名称、角色及disabled；实际产品帧无重复节点或禁用激活动作。
#[gpui_kit::test]
fn disabled_native_controls_export_a11y_without_duplicate_nodes(cx: &mut TestAppContext) {
    let (handle, probe) = super::tests::open(cx);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    cx.update_window(handle, |_, window, cx| {
        for disabled in [false, true] {
            let focus = cx.focus_handle();
            let host = NativeControl::new(
                Button::new("a11y-host")
                    .role(Role::Tab)
                    .aria_selected(true)
                    .accessibility_label("WorkBuddy")
                    .track_focus(&focus)
                    .disabled(disabled)
                    .render(window, cx)
                    .into_element(),
                disabled,
            );
            let mut node = accesskit::Node::new(host.a11y_role().unwrap());
            host.write_a11y_info(&mut node);
            assert_eq!(node.role(), Role::Tab);
            assert_eq!(node.label(), Some("WorkBuddy"));
            assert_eq!(node.is_selected(), Some(true));
            assert_eq!(node.is_disabled(), disabled);
            let refresh = NativeControl::new(
                default_icon_button("a11y-refresh", disabled, cx)
                    .track_focus(&focus)
                    .accessibility_label("Refresh")
                    .render(window, cx)
                    .into_element(),
                disabled,
            );
            let mut node = accesskit::Node::new(refresh.a11y_role().unwrap());
            refresh.write_a11y_info(&mut node);
            assert_eq!(node.label(), Some("Refresh"));
            assert_eq!(node.is_disabled(), disabled);
            probe.update(cx, |probe, cx| {
                probe.disabled = disabled;
                cx.notify();
            });
            window.render_frame(cx);
            let tree: serde_json::Value =
                serde_json::from_str(&window.debug_a11y_tree_json().unwrap()).unwrap();
            for id in ["host", "refresh"] {
                let expected = format!("{:?}", gpui_kit::ElementId::from(id));
                let nodes: Vec<_> = tree["nodes"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter(|node| node["element_id"] == expected)
                    .collect();
                assert_eq!(nodes.len(), 1, "原生节点必须唯一：{id}");
                let empty = Vec::new();
                let actions = nodes[0]["aria"]["on_action"].as_array().unwrap_or(&empty);
                assert_eq!(actions.iter().any(|action| action == "Click"), !disabled);
                assert_eq!(actions.iter().any(|action| action == "Focus"), !disabled);
            }
        }
    })
    .unwrap();
}

/// 禁用期间不松键，再启用并恢复焦点，也不能复活原生pending激活。
#[gpui_kit::test]
fn refresh_disable_then_reenable_cancels_native_pending_activation(cx: &mut TestAppContext) {
    let (handle, probe) = super::tests::open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        window.focus_next(cx);
        window.render_frame(cx);
        key(window, cx, "space", true);
        probe.update(cx, |probe, cx| {
            probe.disabled = true;
            cx.notify();
        });
        window.render_frame(cx);
        probe.update(cx, |probe, cx| {
            probe.disabled = false;
            cx.notify();
        });
        window.render_frame(cx);
        if window.find("refresh").focused() != Some(true) {
            window.focus_next(cx);
            window.focus_next(cx);
            window.render_frame(cx);
        }
        assert_eq!(window.find("refresh").focused(), Some(true));
        key(window, cx, "space", false);
        assert_eq!(
            probe.read(cx).clicks[1],
            0,
            "禁用必须取消原持键，重新聚焦也不能激活"
        );
    })
    .unwrap();
}
