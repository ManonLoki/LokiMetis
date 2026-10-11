// 本文件是 sticky_api_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：真实激活/停用回调跨帧切换，相关缓存重建、其它缓存仍复用，且无旧帧 token。；最小直接 opt-in 缓存必须覆盖 inactive→active→inactive→reactive 完整循环。；旧帧可达范围必须发现稍后 legacy deferred 内创建的 opt-in，而不是只扫描直接条目。

/// 真实激活/停用回调跨帧切换，相关缓存重建、其它缓存仍复用，且无旧帧 token。
fn cache_cycle(cx: &mut TestAppContext, kind: usize, nested: bool) {
    let (handle, relevant, outer, unrelated) = cache_setup(cx, kind, nested);
    for _ in 0..2 {
        assert!(cx.simulate_accessibility_activation(handle).is_some());
        cx.update_window(handle, |_, window, cx| {
            window.draw(cx).clear(cx);
            let before = (relevant.get(), outer.get(), unrelated.get());
            for step in 1..=3 {
                window.draw(cx).clear(cx);
                let tree = ActualTree::capture(window);
                assert_eq!(
                    tree.labels("cached-body"),
                    [
                        "cached-body",
                        "cache-before",
                        "cache-deferred",
                        "cache-after"
                    ]
                );
                assert_eq!(
                    relevant.get(),
                    before.0 + step,
                    "active相关缓存每帧须实际重建AX"
                );
                assert_eq!(
                    unrelated.get(),
                    before.2,
                    "未含opt-in的legacy缓存不可全局失效"
                );
                if nested {
                    assert_eq!(outer.get(), before.1 + step, "实际cached祖先同样须重建");
                }
            }
        })
        .unwrap();
        cx.simulate_accessibility_deactivation(handle);
        cx.update_window(handle, |_, window, cx| {
            window.draw(cx).clear(cx);
            assert!(!window.is_a11y_active());
            let before = (relevant.get(), outer.get(), unrelated.get());
            for _ in 0..3 {
                window.draw(cx).clear(cx);
            }
            assert_eq!(
                (relevant.get(), outer.get(), unrelated.get()),
                before,
                "deactivation后应恢复原缓存replay"
            );
        })
        .unwrap();
    }
}

/// 最小直接 opt-in 缓存必须覆盖 inactive→active→inactive→reactive 完整循环。
#[gpui_kit::test]
fn monitor_sticky_api_cache_activation_is_narrow(cx: &mut TestAppContext) {
    cache_cycle(cx, 0, false);
}

/// 旧帧可达范围必须发现稍后 legacy deferred 内创建的 opt-in，而不是只扫描直接条目。
#[gpui_kit::test]
fn monitor_sticky_api_cache_follows_legacy_deferred_descendant(cx: &mut TestAppContext) {
    cache_cycle(cx, 1, false);
}

/// 嵌套真实 cached 父不能复用后代仍需重建的旧 prepaint range。
#[gpui_kit::test]
fn monitor_sticky_api_cache_marks_nested_cached_ancestors(cx: &mut TestAppContext) {
    cache_cycle(cx, 1, true);
}

/// 旧浮层 API 不捕获原AX父；新API只改变显式调用，不能默改现有 Dialog/Popover。
struct LegacyRoot;
impl Render for LegacyRoot {
    /// 同一父下并排安排legacy和opt-in，用真实树同时证明两者边界。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut legacy = DeferredProbe::new(leaf("legacy-popup"), 1);
        legacy.legacy = true;
        div()
            .id("legacy-parent")
            .role(Role::Group)
            .aria_label("legacy-parent")
            .child(leaf("legacy-before"))
            .child(legacy)
            .child(DeferredProbe::new(leaf("parent-child"), 2))
            .child(leaf("legacy-after"))
    }
}

/// 默认浮层AX仍位于窗口根，新父子序列不吞并legacy节点。
#[gpui_kit::test]
fn monitor_sticky_api_legacy_defer_keeps_popup_semantics(cx: &mut TestAppContext) {
    let handle = cx.add_window(|_, _| LegacyRoot);
    assert!(
        cx.simulate_accessibility_activation(handle.into())
            .is_some()
    );
    cx.update_window(handle.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
        let tree = ActualTree::capture(window);
        assert_eq!(
            tree.labels("legacy-parent"),
            [
                "legacy-parent",
                "legacy-before",
                "parent-child",
                "legacy-after"
            ]
        );
        let popup = tree.id("legacy-popup");
        let root = tree.0["root"].as_str().unwrap();
        assert!(
            tree.0["nodes"][root]["children"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id.as_str() == Some(&popup))
        );
    })
    .unwrap();
}
