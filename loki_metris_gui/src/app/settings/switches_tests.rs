// 本文件是 switches.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        AnyWindowHandle, AppContext, Bounds, Context, Entity, FocusHandle, IntoElement, Quad,
        Render, Role, TestAppContext, Window, WindowBounds, WindowOptions,
        base::TestSupportExt as _, div, point, size, test::TestWindowExt,
    };

    /// 隔离探针只保存受控开关及回调次数，不初始化原生服务或写入偏好。
    struct SwitchProbe {
        /// 开关状态。
        checked: bool,
        /// 开关禁用。
        disabled: bool,
        /// 变化回调次数。
        changes: usize,
        /// 焦点锚点。
        anchor: FocusHandle,
    }

    impl Render for SwitchProbe {
        /// 卡片没有代理点击；实际生产开关通过同一布尔回执更新内存状态。
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let changed = cx.listener(|this, next: &bool, _, cx| {
                this.checked = *next;
                this.changes += 1;
                cx.notify();
            });
            div()
                .size_full()
                .p(px(20.))
                .tab_group()
                .child(
                    div()
                        .id("switch-probe-anchor")
                        .absolute()
                        .size_0()
                        .track_focus(&self.anchor)
                        .tab_stop(false),
                )
                .child(
                    div()
                        .id("switch-probe-card")
                        .test_support()
                        .w(px(260.))
                        .h(px(100.))
                        .p(px(16.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().child("Notifications"))
                        .child(
                            settings_switch(
                                "switch-probe-control",
                                "Notifications".into(),
                                self.checked,
                                self.disabled,
                                cx,
                            )
                            .on_change(move |next, _, window, cx| changed(&next, window, cx)),
                        ),
                )
        }
    }

    /// 使用 Kit 实际根窗口和主题，只把初始焦点放到不参与 Tab 的测试锚点。
    fn open_probe(
        cx: &mut TestAppContext,
        checked: bool,
        disabled: bool,
    ) -> (AnyWindowHandle, Entity<SwitchProbe>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(320.), px(160.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let anchor = cx.focus_handle();
                    window.focus(&anchor, cx);
                    cx.new(|_| SwitchProbe {
                        checked,
                        disabled,
                        changes: 0,
                        anchor,
                    })
                },
            )
            .expect("创建隔离开关窗口")
        })
    }

    /// 从实际绘制结果识别三个圆角填充，物理像素取整允许小圆相差一个像素。
    fn visual_quads(window: &Window) -> (Quad, Quad, Quad) {
        let scale = window.scale_factor();
        let quads = window.painted_quads();
        let tracks = quads
            .iter()
            .filter(|quad| quad.bounds.size == size(px(38.).scale(scale), px(20.).scale(scale)))
            .collect::<Vec<_>>();
        assert_eq!(tracks.len(), 1, "轨道必须按38×20外框绘制");
        let thumbs = quads
            .iter()
            .filter(|quad| quad.bounds.size == size(px(14.).scale(scale), px(14.).scale(scale)))
            .collect::<Vec<_>>();
        assert_eq!(thumbs.len(), 1, "滑块必须按14×14外框绘制");
        let dots = quads
            .iter()
            .filter(|quad| {
                (quad.bounds.size.width.0 - 5.6 * scale).abs() <= 1.
                    && (quad.bounds.size.height.0 - 5.6 * scale).abs() <= 1.
            })
            .collect::<Vec<_>>();
        assert_eq!(dots.len(), 1, "滑块中央必须实际绘制5.6px指示圆");
        (*tracks[0], *thumbs[0], *dots[0])
    }

    /// 同时断言外框、滑块和指示圆，焦点及动画不得改变尺寸与垂直居中。
    fn assert_geometry(window: &Window, checked: bool) -> Bounds<gpui_kit::Pixels> {
        let bounds = window.find("switch-probe-control").bounds();
        assert_eq!(bounds.size, size(px(38.), px(20.)));
        let scale = window.scale_factor();
        let (track, thumb, dot) = visual_quads(window);
        assert_eq!(track.bounds, bounds.scale(scale));
        assert_eq!(
            thumb.bounds.origin,
            point(
                (bounds.origin.x + px(if checked { 21.5 } else { 2.5 })).scale(scale),
                (bounds.origin.y + px(3.)).scale(scale),
            ),
        );
        let thumb_center = thumb.bounds.center();
        let dot_center = dot.bounds.center();
        assert!((thumb_center.x.0 - dot_center.x.0).abs() <= 1.);
        assert!((thumb_center.y.0 - dot_center.y.0).abs() <= 1.);
        assert!(
            (dot.corner_radii.top_left.0 - dot.bounds.size.width.0 / 2.).abs() <= 1.,
            "原生像素取整后指示圆半径仍应对应直径一半：{:?}",
            dot.corner_radii,
        );
        bounds
    }

    /// 读取实际绘制的滑块相对位置，保留原生像素取整结果供时间回归使用。
    fn thumb_x(window: &Window) -> f32 {
        let (track, thumb, _) = visual_quads(window);
        (thumb.bounds.origin.x.0 - track.bounds.origin.x.0) / window.scale_factor()
    }

    /// 浮点插值终点允许计算舍入误差，四个颜色通道都必须到达预期值。
    fn assert_color(actual: Option<Hsla>, expected: Hsla) {
        let actual = actual.expect("实际绘制必须为纯色填充");
        assert!(
            [
                (actual.h, expected.h),
                (actual.s, expected.s),
                (actual.l, expected.l),
                (actual.a, expected.a)
            ]
            .into_iter()
            .all(|(actual, expected)| (actual - expected).abs() < 0.000001),
            "绘制颜色未到达目标：{actual:?} != {expected:?}",
        );
    }

    /// 推进虚拟时钟并重绘同一原生窗口，不创建实际系统服务。
    fn advance(cx: &mut TestAppContext, handle: AnyWindowHandle, millis: u64) {
        cx.executor().advance_clock(Duration::from_millis(millis));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }

    /// 点击卡片留白不触发保存；只有开关本身点击一次才回传并更新受控值。
    #[gpui_kit::test]
    fn card_background_is_inert_and_control_click_toggles_once(cx: &mut TestAppContext) {
        let (handle, probe) = open_probe(cx, false, false);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let control = window.find("switch-probe-control");
            assert_eq!(control.role(), Some(Role::Switch));
            assert_eq!(control.label(), Some("Notifications"));
            assert_eq!(control.checked(), Some(false));
            window.click_at("switch-probe-card", point(px(4.), px(4.)), cx);
            assert_eq!(probe.read(cx).changes, 0);
            assert!(!probe.read(cx).checked);
            window.click("switch-probe-control", cx);
            assert_eq!(probe.read(cx).changes, 1);
            assert!(probe.read(cx).checked);
            assert_eq!(window.find("switch-probe-control").checked(), Some(true));
        })
        .unwrap();
    }

    /// 已聚焦控件变为禁用后，点击与完整的空格、回车按下抬起均不得回传。
    #[gpui_kit::test]
    fn disabled_switch_ignores_pointer_and_keyboard_activation(cx: &mut TestAppContext) {
        for checked in [false, true] {
            let (handle, probe) = open_probe(cx, checked, false);
            cx.update_window(handle, |_, window, cx| {
                window.press("tab", cx);
                assert_eq!(window.find("switch-probe-control").focused(), Some(true));
                probe.update(cx, |this, cx| {
                    this.disabled = true;
                    cx.notify();
                });
            })
            .unwrap();
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.click("switch-probe-control", cx);
                window.press("space", cx);
                window.press("enter", cx);
                assert_eq!(probe.read(cx).changes, 0);
                assert_eq!(probe.read(cx).checked, checked);
                assert_eq!(window.find("switch-probe-control").checked(), Some(checked));
                assert_geometry(window, checked);
            })
            .unwrap();
        }
    }

    /// Tab进入实际开关，空格与回车各完成一次原生按下抬起并更新受控值。
    #[gpui_kit::test]
    fn tab_space_and_enter_toggle_controlled_value_once_each(cx: &mut TestAppContext) {
        let (handle, probe) = open_probe(cx, false, false);
        cx.update_window(handle, |_, window, cx| {
            window.press("tab", cx);
            assert_eq!(window.find("switch-probe-control").focused(), Some(true));
            assert_eq!(probe.read(cx).changes, 0);
            window.press("space", cx);
            assert!(probe.read(cx).checked);
            assert_eq!(probe.read(cx).changes, 1);
            assert_eq!(window.find("switch-probe-control").checked(), Some(true));
            window.press("enter", cx);
            assert!(!probe.read(cx).checked);
            assert_eq!(probe.read(cx).changes, 2);
            assert_eq!(window.find("switch-probe-control").checked(), Some(false));
        })
        .unwrap();
    }

    /// 焦点轮廓和受控值变化不改变38×20几何，滑块只在两个参考位置之间移动。
    #[gpui_kit::test]
    fn focus_and_checked_state_preserve_track_and_thumb_geometry(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (handle, _probe) = open_probe(cx, false, false);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let before = assert_geometry(window, false);
            window.press("tab", cx);
            assert_eq!(window.find("switch-probe-control").focused(), Some(true));
            assert_eq!(assert_geometry(window, false), before);
            window.press("space", cx);
            assert_eq!(assert_geometry(window, true), before);
            window.press("enter", cx);
            assert_eq!(assert_geometry(window, false), before);
        })
        .unwrap();
    }

    /// 开关初值直接落在对应端点，普通与禁用状态都保留参考圆点和滑块颜色。
    #[gpui_kit::test]
    fn initial_values_and_disabled_indicator_are_painted_without_motion(cx: &mut TestAppContext) {
        for checked in [false, true] {
            for disabled in [false, true] {
                let (handle, _) = open_probe(cx, checked, disabled);
                let before = cx
                    .update_window(handle, |_, window, cx| {
                        window.render_frame(cx);
                        assert_geometry(window, checked);
                        let (track, thumb, dot) = visual_quads(window);
                        assert_eq!(track.background.as_solid(), dot.background.as_solid());
                        let dark = cx.theme().mode.is_dark();
                        let expected_thumb: Hsla = rgb(if disabled {
                            if dark { 0x696969 } else { 0xf8f9fa }
                        } else {
                            0xffffff
                        })
                        .into();
                        assert_eq!(thumb.background.as_solid(), Some(expected_thumb));
                        if disabled {
                            let expected: Hsla = rgb(if dark { 0x2e2e2e } else { 0xe9ecef }).into();
                            assert_eq!(dot.background.as_solid(), Some(expected));
                        }
                        thumb_x(window)
                    })
                    .unwrap();
                advance(cx, handle, 75);
                cx.update_window(handle, |_, window, _| {
                    assert_geometry(window, checked);
                    assert_eq!(thumb_x(window), before);
                })
                .unwrap();
            }
        }
    }

    /// 点击立即更新语义和中央圆，轨道与滑块在75ms中间态后于150ms到达终点。
    #[gpui_kit::test]
    fn switch_motion_reaches_eased_midpoint_and_finishes_at_150ms(cx: &mut TestAppContext) {
        let (handle, _) = open_probe(cx, false, false);
        let off = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let off = visual_quads(window).0.background.as_solid().unwrap();
                window.click("switch-probe-control", cx);
                assert_eq!(window.find("switch-probe-control").checked(), Some(true));
                assert_geometry(window, false);
                let (track, _, dot) = visual_quads(window);
                assert_eq!(track.background.as_solid(), Some(off));
                assert_eq!(dot.background.as_solid(), Some(cx.theme().primary));
                off
            })
            .unwrap();
        advance(cx, handle, 75);
        cx.update_window(handle, |_, window, cx| {
            let x = thumb_x(window);
            assert!(x > 12. && x < 21.5, "ease过半时应超过路程一半：{x}");
            let (track, _, dot) = visual_quads(window);
            assert_ne!(track.background.as_solid(), Some(off));
            assert_ne!(track.background.as_solid(), Some(cx.theme().primary));
            assert_eq!(dot.background.as_solid(), Some(cx.theme().primary));
            assert_eq!(
                window.find("switch-probe-control").bounds().size,
                size(px(38.), px(20.))
            );
        })
        .unwrap();
        advance(cx, handle, 75);
        cx.update_window(handle, |_, window, cx| {
            assert_geometry(window, true);
            assert_color(
                visual_quads(window).0.background.as_solid(),
                cx.theme().primary,
            );
        })
        .unwrap();
    }

    /// 动画中途反向沿当前采样位置返回，圆点立即切回目标色而轨道连续过渡。
    #[gpui_kit::test]
    fn rapid_reverse_continues_from_current_position_and_color(cx: &mut TestAppContext) {
        let (handle, _) = open_probe(cx, false, false);
        let off = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let off = visual_quads(window).0.background.as_solid().unwrap();
                window.click("switch-probe-control", cx);
                off
            })
            .unwrap();
        advance(cx, handle, 60);
        let before = cx
            .update_window(handle, |_, window, cx| {
                let x = thumb_x(window);
                let color = visual_quads(window).0.background.as_solid();
                assert!(x > 2.5 && x < 21.5);
                window.click("switch-probe-control", cx);
                assert_eq!(thumb_x(window), x, "反向时不得跳回端点");
                let (track, _, dot) = visual_quads(window);
                assert_eq!(track.background.as_solid(), color);
                assert_eq!(dot.background.as_solid(), Some(off));
                x
            })
            .unwrap();
        advance(cx, handle, 30);
        cx.update_window(handle, |_, window, _| {
            assert!(thumb_x(window) < before && thumb_x(window) > 2.5);
        })
        .unwrap();
        advance(cx, handle, 150);
        cx.update_window(handle, |_, window, _| {
            assert_geometry(window, false);
            assert_color(visual_quads(window).0.background.as_solid(), off);
        })
        .unwrap();
    }

    /// 系统减少动态效果在运动中生效，当前和后续目标都立即到位。
    #[gpui_kit::test]
    fn reduced_motion_finishes_active_and_future_switch_transitions(cx: &mut TestAppContext) {
        let (handle, _) = open_probe(cx, false, false);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("switch-probe-control", cx);
        })
        .unwrap();
        advance(cx, handle, 60);
        cx.update_window(handle, |_, window, _| {
            assert!(thumb_x(window) > 2.5 && thumb_x(window) < 21.5);
        })
        .unwrap();
        cx.update(|cx| cx.set_reduce_motion(true));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_geometry(window, true);
            assert_color(
                visual_quads(window).0.background.as_solid(),
                cx.theme().primary,
            );
            window.click("switch-probe-control", cx);
            assert_geometry(window, false);
        })
        .unwrap();
    }
}
