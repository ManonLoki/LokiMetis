//! slot/hero特例使用真实SpotlightCard，验证不等宽边框布局、缓存和CSS过渡终点。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Render, RenderImage, TestAppContext, WindowBounds,
    WindowOptions, WindowVisibility, base::TestSupportExt as _, point, rgb, size,
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

/// 只保留owner弱句柄供真实纹理回归读取，不改变组件生命周期。
type Owner = Rc<RefCell<Option<gpui_kit::WeakEntity<MaterialState>>>>;

/// radius/top accent可变探针不实例化业务壳层或文件服务。
struct Probe {
    /// 卡片圆角。
    radius: Pixels,
    /// 启用顶边强调色。
    accent: bool,
    /// 深色主题。
    dark: bool,
    /// 材质 owner 的观察。
    owner: Owner,
}

impl Render for Probe {
    /// 子元素原点同时验证border参与布局，不能用纯装饰的顶色条代替3px顶边。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let owner = self.owner.clone();
        let mut card = SpotlightCard::new("variant-card", self.dark).radius(self.radius);
        if self.accent {
            card = card.top_accent(px(3.), rgb(0xd7952c).into());
        }
        div().size_full().p(px(24.)).child(
            card.observe_material(move |state| {
                *owner.borrow_mut() = Some(state.downgrade());
            })
            .test_support()
            .w(px(320.))
            .h(px(180.))
            .p(px(20.))
            .child(div().id("variant-child").test_support().size(px(10.))),
        )
    }
}

/// 创建已初始化Kit的测试窗口，所有材质均从真实元素绘制。
fn open(cx: &mut TestAppContext, radius: Pixels) -> (AnyWindowHandle, Entity<Probe>, Owner) {
    cx.update(gpui_kit::init);
    let owner: Owner = Rc::new(RefCell::new(None));
    let (handle, probe) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(500.), px(300.)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| {
                cx.new(|_| Probe {
                    radius,
                    accent: true,
                    dark: false,
                    owner: owner.clone(),
                })
            },
        )
        .unwrap()
    });
    (handle, probe, owner)
}

/// owner顺序的最后一张是紧凑rim，光斑加入或退出也不改变这个约定。
fn rim(owner: &Owner, cx: &App) -> Arc<RenderImage> {
    owner
        .borrow()
        .as_ref()
        .unwrap()
        .upgrade()
        .unwrap()
        .read(cx)
        .audit()
        .1
        .last()
        .unwrap()
        .clone()
}

/// 图像顶部中间texel对应真实顶边，纹理守护边距不参与颜色断言。
fn top_pixel(image: &RenderImage) -> [u8; 4] {
    let width = image.size(0).width.0;
    let offset = (2 * width + width / 2) * 4;
    image.as_bytes(0).expect("原生材质具有实际BGRA帧")[offset as usize..offset as usize + 4]
        .try_into()
        .unwrap()
}

/// 两种产品圆角在1x/2x保持真实3px顶边，hover到达后顶色等于主border并复用静止纹理。
#[gpui_kit::test]
fn top_accent_affects_layout_and_transitions_for_slot_and_hero(cx: &mut TestAppContext) {
    for radius in [px(12.), px(20.)] {
        for scale in [1., 2.] {
            let (handle, probe, owner) = open(cx, radius);
            cx.simulate_window_scale_factor_change(handle, scale);
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let card = window.find("variant-card").bounds();
                let child = window.find("variant-child").bounds();
                assert_eq!(child.origin - card.origin, point(px(21.), px(23.)));
                let body = window
                    .painted_quads()
                    .into_iter()
                    .find(|quad| {
                        quad.bounds == card.scale(window.scale_factor())
                            && quad
                                .background
                                .as_solid()
                                .is_some_and(|color| (color.a - 0.90).abs() < 1e-6)
                    })
                    .unwrap();
                assert_eq!(
                    body.corner_radii.top_left,
                    radius.scale(window.scale_factor())
                );
                let initial = rim(&owner, cx);
                assert_eq!(top_pixel(&initial), [0x2c, 0x95, 0xd7, 255]);
                window.render_frame(cx);
                assert_eq!(rim(&owner, cx).id, initial.id);
                window.hover("variant-card", cx);
                assert_eq!(top_pixel(&rim(&owner, cx)), top_pixel(&initial));
            })
            .unwrap();
            cx.executor().advance_clock(Duration::from_millis(300));
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let current = rim(&owner, cx);
                let pixel = top_pixel(&current);
                for (actual, expected) in pixel[..3].iter().zip([157i16, 169, 211]) {
                    assert!((i16::from(*actual) - expected).abs() <= 1);
                }
                window.render_frame(cx);
                assert_eq!(rim(&owner, cx).id, current.id);
                probe.update(cx, |state, cx| {
                    state.accent = false;
                    cx.notify();
                });
                window.render_frame(cx);
                let card = window.find("variant-card").bounds();
                let child = window.find("variant-child").bounds();
                assert_eq!(child.origin - card.origin, point(px(21.), px(21.)));
            })
            .unwrap();
        }
    }
}

/// 真实accent卡隐藏时同时回收rim、shadow与径向图，不依赖另一个窗口帧。
#[gpui_kit::test]
fn accent_card_hiding_releases_rim_atlas_texture(cx: &mut TestAppContext) {
    let (handle, _, owner) = open(cx, px(12.));
    let images = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.hover("variant-card", cx);
            let images = owner
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .read(cx)
                .audit()
                .1;
            assert!(images.len() >= 3);
            assert!(
                images
                    .iter()
                    .all(|image| window.has_image_atlas_entry(image))
            );
            images
        })
        .unwrap();
    cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
    cx.update_window(handle, |_, window, _| {
        assert!(
            images
                .iter()
                .all(|image| !window.has_image_atlas_entry(image))
        );
    })
    .unwrap();
}

/// 已显示卡片切换主题首帧保留原surface，300ms准确到暗色端点且阴影重采样。
#[gpui_kit::test]
fn theme_change_animates_real_surface_and_shadow_continuously(cx: &mut TestAppContext) {
    let (handle, probe, owner) = open(cx, px(20.));
    let initial = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let image = owner
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .read(cx)
                .audit()
                .1[0]
                .clone();
            probe.update(cx, |state, cx| {
                state.dark = true;
                cx.notify();
            });
            window.render_frame(cx);
            let card = window.find("variant-card").bounds();
            assert!(window.painted_quads().iter().any(|quad| {
                quad.bounds == card.scale(window.scale_factor())
                    && quad
                        .background
                        .as_solid()
                        .is_some_and(|color| (color.a - 0.90).abs() < 1e-6)
            }));
            image
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("variant-card").bounds();
        let color = window
            .painted_quads()
            .into_iter()
            .find_map(|quad| {
                if quad.bounds == card.scale(window.scale_factor()) {
                    quad.background.as_solid()
                } else {
                    None
                }
            })
            .unwrap();
        assert!((color.a - 0.92).abs() < 1e-6);
        let expected: Hsla = rgb(0x1e1c1a).into();
        assert!((color.h - expected.h).abs() < 1e-6);
        let current = owner
            .borrow()
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap()
            .read(cx)
            .audit()
            .1[0]
            .clone();
        assert_ne!(initial.id, current.id);
        window.render_frame(cx);
        assert_eq!(
            owner
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .read(cx)
                .audit()
                .1[0]
                .id,
            current.id
        );
    })
    .unwrap();
}
