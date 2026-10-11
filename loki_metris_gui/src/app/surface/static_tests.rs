//! 静态品牌材质使用真实输入、原生绘制和GPU图集观察验证，不启动产品宿主服务。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Render, RenderImage, Rgba, Role, TestAppContext,
    WindowBounds, WindowOptions, WindowVisibility,
    base::{Button, TestSupportExt as _},
    component::input::{Input, InputState},
    point, size,
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

/// 弱句柄仅用于观察真实owner和图集，不延长组件卸载后的生命周期。
type Owners = Rc<RefCell<[Option<gpui_kit::WeakEntity<MaterialState>>; 3]>>;

/// 同一原生视图同时展示三个CSS模式，输入实体跨主题重绘保持稳定。
struct Probe {
    dark: bool,
    shown: [bool; 3],
    clicks: [usize; 3],
    inputs: [Entity<InputState>; 3],
    owners: Owners,
}

impl Render for Probe {
    /// 留白没有动作代理；按钮和文本输入分别拥有自己的事件与辅助语义。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(24.)).flex().gap(px(20.)).children(
            [
                StaticMaterial::Panel,
                StaticMaterial::Table,
                StaticMaterial::FilterBar,
            ]
            .into_iter()
            .enumerate()
            .filter_map(|(index, mode)| {
                if !self.shown[index] {
                    return None;
                }
                let owners = self.owners.clone();
                Some(
                    StaticCard::new(("static-card", index), self.dark, mode)
                        .observe_material(move |owner| {
                            owners.borrow_mut()[index] = Some(owner.downgrade())
                        })
                        .test_support()
                        .w(px(320.))
                        .h(px(220.))
                        .flex_shrink_0()
                        .p(px(20.))
                        .gap_4()
                        .role(Role::Group)
                        .aria_label(format!("Static {mode:?}"))
                        .child(
                            div()
                                .id(("static-input", index))
                                .test_support()
                                .h(px(36.))
                                .child(
                                    Input::new(&self.inputs[index])
                                        .aria_label(format!("Draft {index}")),
                                ),
                        )
                        .child(
                            Button::new(("static-action", index))
                                .child("Child action")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.clicks[index] += 1;
                                    cx.notify();
                                })),
                        ),
                )
            }),
        )
    }
}

/// Kit只负责原生输入与绘制，不创建扫描器、设置宿主或读取真实用户路径。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>, Owners) {
    cx.update(gpui_kit::init);
    let owners: Owners = Rc::new(RefCell::new([None, None, None]));
    let (handle, probe) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1100.), px(350.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| Probe {
                    dark: false,
                    shown: [true; 3],
                    clicks: [0; 3],
                    inputs: std::array::from_fn(|_| cx.new(|cx| InputState::new(window, cx))),
                    owners: owners.clone(),
                })
            },
        )
        .unwrap()
    });
    (handle, probe, owners)
}

/// 只读取实际card bounds上的半透明surface，不能把邻居或控件底色算作材质。
fn surface(index: usize, window: &Window) -> Rgba {
    let card = window
        .find(("static-card", index))
        .bounds()
        .scale(window.scale_factor());
    let quad = window
        .painted_quads()
        .into_iter()
        .find(|quad| {
            quad.bounds == card
                && quad
                    .background
                    .as_solid()
                    .is_some_and(|color| color.a > 0.5)
        })
        .expect("真实静态品牌surface必须被绘制");
    assert_eq!(
        quad.corner_radii.top_left,
        px(20.).scale(window.scale_factor())
    );
    quad.background.as_solid().unwrap().into()
}

/// 观察当前真正上传的图像；返回值只在断言期间保存CPU引用。
fn images(index: usize, owners: &Owners, cx: &App) -> Vec<Arc<RenderImage>> {
    owners.borrow()[index]
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap()
        .read(cx)
        .audit()
        .1
}

/// 三种模式都没有材质hover目标，留白不执行按钮且真实文本输入仍接收键盘。
#[gpui_kit::test]
fn static_modes_keep_hover_inert_and_actual_child_input(cx: &mut TestAppContext) {
    let (handle, probe, owners) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for index in 0..3 {
            let card = window.find(("static-card", index));
            assert_eq!(card.role(), Some(Role::Group));
            let original = surface(index, window);
            let before = images(index, &owners, cx);
            assert_eq!(before.len(), usize::from(index != 2));
            window.hover(("static-card", index), cx);
            window.render_frame(cx);
            assert_eq!(surface(index, window), original);
            let owner = owners.borrow()[index].as_ref().unwrap().upgrade().unwrap();
            assert!(!owner.read(cx).audit().0);
            assert_eq!(
                images(index, &owners, cx)
                    .iter()
                    .map(|image| image.id)
                    .collect::<Vec<_>>(),
                before.iter().map(|image| image.id).collect::<Vec<_>>()
            );
            window.click_at(("static-card", index), point(px(4.), px(4.)), cx);
            assert_eq!(probe.read(cx).clicks[index], 0);
            window.click(("static-action", index), cx);
            assert_eq!(probe.read(cx).clicks[index], 1);
            window.click(("static-input", index), cx);
            window.input("draft", cx);
            assert_eq!(
                probe.read(cx).inputs[index].read(cx).value().as_str(),
                "draft"
            );
            assert_eq!(probe.read(cx).clicks[index], 1);
        }
        assert!(
            images(2, &owners, cx).is_empty(),
            "筛选条不能上传阴影或光斑"
        );
    })
    .unwrap();
}

/// table主题变化从当前颜色连续反向，其他静态类立即切换，正在编辑的文本不被重置。
#[gpui_kit::test]
fn static_theme_contracts_keep_input_and_table_transition_continuous(cx: &mut TestAppContext) {
    let (handle, probe, _) = open(cx);
    let light = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(("static-input", 1usize), cx);
            window.input("pending", cx);
            let light = surface(1, window);
            probe.update(cx, |state, cx| {
                state.dark = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(surface(1, window), light);
            for index in [0, 2] {
                let dark = surface(index, window);
                assert!((dark.a - 0.92).abs() < 1e-6);
                assert!((dark.r - 30. / 255.).abs() < 1e-6);
            }
            light
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(80));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let middle = surface(1, window);
        assert!(middle.r > 30. / 255. && middle.r < light.r);
        assert!(middle.a > light.a && middle.a < 0.92);
        probe.update(cx, |state, cx| {
            state.dark = false;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(surface(1, window), middle);
        assert_eq!(surface(0, window), light);
        assert_eq!(surface(2, window), light);
        window.input("!", cx);
        assert_eq!(
            probe.read(cx).inputs[1].read(cx).value().as_str(),
            "pending!"
        );
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(surface(1, window), light);
        assert_eq!(
            probe.read(cx).inputs[1].read(cx).value().as_str(),
            "pending!"
        );
    })
    .unwrap();
}

/// 真实 atlas 在隐藏时立即清空，共享阴影只有最后一个 owner 卸载才释放。
#[gpui_kit::test]
fn static_cards_release_atlas_on_hide_and_independent_unmount(cx: &mut TestAppContext) {
    let (handle, probe, owners) = open(cx);
    let first = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let first: Vec<_> = (0..3)
                .flat_map(|index| images(index, &owners, cx))
                .collect();
            assert_eq!(first.len(), 2);
            assert!(
                first
                    .iter()
                    .all(|image| window.has_image_atlas_entry(image))
            );
            first
        })
        .unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.update_window(handle, |_, window, _| {
        assert!(
            first
                .iter()
                .all(|image| !window.has_image_atlas_entry(image))
        );
    })
    .unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
    let (removed_owner, removed, kept) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let removed_owner = owners.borrow()[0].clone().unwrap();
            let removed = images(0, &owners, cx)[0].clone();
            let kept = images(1, &owners, cx)[0].clone();
            assert_eq!(removed.id, kept.id);
            probe.update(cx, |state, cx| {
                state.shown[0] = false;
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            (removed_owner, removed, kept)
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(removed_owner.upgrade().is_none());
        assert!(window.has_image_atlas_entry(&removed));
        assert!(window.has_image_atlas_entry(&kept));
        assert_eq!(images(1, &owners, cx)[0].id, kept.id);
        assert!(images(2, &owners, cx).is_empty());
        probe.update(cx, |state, cx| { state.shown[1] = false; cx.notify(); });
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, _| {
        assert!(!window.has_image_atlas_entry(&kept));
    }).unwrap();
}
