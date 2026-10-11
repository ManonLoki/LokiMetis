//! 真实元素回归不启动应用宿主或文件服务，只验证卡片输入与实际材质绘制。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Render, Role, TestAppContext, WindowBounds,
    WindowOptions,
    base::{Button, TestSupportExt as _},
    point, size,
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc};

/// 每张真实卡片的测试弱句柄；帧间刷新该句柄不能延长已卸载材质生命周期。
type Owners = Rc<RefCell<[Option<gpui_kit::WeakEntity<MaterialState>>; 2]>>;

/// 同一父容器内的两张卡片共享局部state键，但必须由各自ElementId形成独立路径。
struct MultiProbe {
    shown: [bool; 2],
    owners: Owners,
    dark: bool,
}

impl Render for MultiProbe {
    /// 两张卡片使用完全相同材质和几何；hover差异只能来自状态隔离。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(24.)).flex().gap(px(24.)).children(
            ["surface-a", "surface-b"]
                .into_iter()
                .enumerate()
                .filter_map(|(index, id)| {
                    if !self.shown[index] {
                        return None;
                    }
                    let owners = self.owners.clone();
                    Some(
                        SpotlightCard::new(id, self.dark)
                            .observe_material(move |owner| {
                                owners.borrow_mut()[index] = Some(owner.downgrade());
                            })
                            .test_support()
                            .w(px(240.))
                            .h(px(180.))
                            .flex_shrink_0(),
                    )
                }),
        )
    }
}

/// 卡片只承载一个原生按钮，点击留白不应回传子控件动作。
struct Probe {
    clicks: usize,
    shown: bool,
    dark: bool,
}

impl Render for Probe {
    /// 普通外层背景提供离开hover的测试区域，卡片复用生产renderer。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("surface-outside")
            .test_support()
            .size_full()
            .p(px(24.))
            .child(div().children(self.shown.then(|| {
                SpotlightCard::new("surface-card", self.dark)
                    .test_support()
                    .w(px(400.))
                    .h(px(200.))
                    .p(px(24.))
                    .role(Role::Group)
                    .aria_label("Material card")
                    .child(
                        Button::new("surface-control")
                            .child("Child action")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clicks += 1;
                                cx.notify();
                            })),
                    )
            })))
    }
}

/// Kit测试窗口使用真实元素生命周期，不实例化Shell和系统服务。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(600.), px(350.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| Probe {
                    clicks: 0,
                    shown: true,
                    dark: false,
                })
            },
        )
        .unwrap()
    })
}

/// 原生容器传递语义与子输入，实际surface保留参考半透明色和20px圆角。
#[gpui_kit::test]
fn card_material_keeps_semantics_geometry_and_child_input(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("surface-card");
        assert_eq!(card.role(), Some(Role::Group));
        assert_eq!(card.label(), Some("Material card"));
        assert_eq!(card.bounds().size, size(px(400.), px(200.)));
        let quads = window.painted_quads();
        let material = quads.iter().find(|quad| {
            quad.bounds == card.bounds().scale(window.scale_factor())
                && quad
                    .background
                    .as_solid()
                    .is_some_and(|color| (color.a - 0.90).abs() < 1e-6)
        });
        assert!(material.is_some(), "实际绘制必须包含90%品牌surface");
        let material = material.unwrap();
        assert_eq!(
            material.corner_radii.top_left,
            px(20.).scale(window.scale_factor())
        );
        window.click_at("surface-card", point(px(4.), px(4.)), cx);
        assert_eq!(probe.read(cx).clicks, 0);
        window.click("surface-control", cx);
        assert_eq!(probe.read(cx).clicks, 1);
        window.hover("surface-card", cx);
    })
    .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        probe.update(cx, |state, cx| {
            state.shown = false;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.try_find("surface-card").is_none());
    })
    .unwrap();
}

/// 独立 owner 共享相同阴影，hover 仍隔离，邻居卸载不删除保留卡片的当前图。
#[gpui_kit::test]
fn sibling_cards_isolate_hover_textures_and_component_release(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let owners: Owners = Rc::new(RefCell::new([None, None]));
    let (handle, probe) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(600.), px(350.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| MultiProbe {
                    shown: [true, true],
                    owners: owners.clone(),
                    dark: false,
                })
            },
        )
        .unwrap()
    });
    let (weak_a, weak_b) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let weak_a = owners.borrow()[0].clone().unwrap();
            let weak_b = owners.borrow()[1].clone().unwrap();
            let a = weak_a.upgrade().unwrap();
            let b = weak_b.upgrade().unwrap();
            assert_ne!(a.entity_id(), b.entity_id());
            let (_, images_a) = a.read(cx).audit();
            let (_, images_b) = b.read(cx).audit();
            assert_eq!((images_a.len(), images_b.len()), (1, 1));
            assert_eq!(images_a[0].id, images_b[0].id);
            window.hover("surface-a", cx);
            assert!(a.read(cx).audit().0);
            assert!(!b.read(cx).audit().0);
            assert!(a.read(cx).audit().1.len() > 1);
            assert_eq!(b.read(cx).audit().1[0].id, images_b[0].id);
            (weak_a, weak_b)
        })
        .unwrap();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    let (removed, kept) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let a = weak_a.upgrade().unwrap();
            let b = weak_b.upgrade().unwrap();
            let removed = a.read(cx).audit().1;
            let kept = b.read(cx).audit().1[0].clone();
            drop(a);
            drop(b);
            probe.update(cx, |state, cx| {
                state.shown[0] = false;
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            (removed, kept)
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(
            weak_a.upgrade().is_none(),
            "完整卡片卸载必须释放keyed owner"
        );
        assert!(
            removed
                .iter()
                .all(|image| !window.has_image_atlas_entry(image))
        );
        assert_eq!(weak_b.upgrade().unwrap().read(cx).audit().1[0].id, kept.id);
        assert!(window.has_image_atlas_entry(&kept));
    })
    .unwrap();
}

/// 两个真实卡片在中间主题帧同步共享，反转和终点仍保持原300ms时间线。
#[gpui_kit::test]
fn sibling_theme_frames_share_exact_shadow_samples(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let owners: Owners = Rc::new(RefCell::new([None, None]));
    let (handle, probe) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(600.), px(350.))))),
        ..Default::default()
    }, cx, |_, cx| cx.new(|_| MultiProbe { shown: [true, true], owners: owners.clone(), dark: false })).unwrap());
    let images = |cx: &App| {
        std::array::from_fn::<_, 2, _>(|index| owners.borrow()[index].as_ref().unwrap().upgrade().unwrap().read(cx).audit().1[0].clone())
    };
    let first = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let first = images(cx);
        assert_eq!(first[0].id, first[1].id);
        probe.update(cx, |probe, cx| { probe.dark = true; cx.notify(); });
        window.render_frame(cx);
        assert_eq!(images(cx)[0].id, first[0].id, "切换起点保留原完整CSS参数");
        first[0].clone()
    }).unwrap();
    for millis in [75, 75] {
        cx.executor().advance_clock(std::time::Duration::from_millis(millis));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let frame = images(cx);
            assert_eq!(frame[0].id, frame[1].id, "同一真实draw必须共享完整中间样本");
            assert_ne!(frame[0].id, first.id);
        }).unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        probe.update(cx, |probe, cx| { probe.dark = false; cx.notify(); });
        window.render_frame(cx);
    }).unwrap();
    cx.executor().advance_clock(std::time::Duration::from_millis(40));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let reversed = images(cx);
        assert_eq!(reversed[0].id, reversed[1].id);
        assert_ne!(reversed[0].id, first.id);
    }).unwrap();
}
