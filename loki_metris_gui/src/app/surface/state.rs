//! 每张卡片的owner保存当前材质与CSS时间线，关闭、隐藏和卸载都回收图集。

#[cfg(test)]
#[path = "shadow_owner_tests.rs"]
mod shadow_owner_tests;

#[cfg(test)]
#[path = "rim_owner_tests.rs"]
mod rim_owner_tests;

use super::{
    decoration::{CardDecoration, CardPriority},
    Geometry, MaterialKind, StaticMaterial, palette::MaterialMotion, rim::Rim, shadow::Shadow,
    spotlight::Spotlight, textures::Retired,
};
use gpui_kit::{
    BorderStyle, Bounds, BoxShadow, Context, Corners, Edges, Pixels, Point, Subscription, Window,
    point, px, quad, transparent_black,
};

/// 所有纹理都由一个元素owner拥有，后帧任务只保存弱引用。
pub(in crate::app) struct MaterialState {
    /// 指针相对卡片的位置。
    pub pointer: Point<Pixels>,
    /// 卡片本帧可见。
    pub visible: bool,
    /// 指针悬停。
    hovered: bool,
    /// 装饰悬停独立于静态材质，静态卡不因此获得聚光或材质阴影动画。
    decoration_hovered: bool,
    /// 材质颜色与阴影过渡。
    motion: MaterialMotion,
    /// 装饰光轨仅记录可见活动时间，不分配纹理或后台任务。
    decoration: CardDecoration,
    /// 阴影纹理。
    shadow: Shadow,
    /// 聚光纹理。
    spotlight: Spotlight,
    /// 边框与顶边纹理。
    rim: Rim,
    /// 待下一帧回收的旧纹理。
    retired: Retired,
    /// 已安排帧后清理。
    cleanup_scheduled: bool,
    /// 窗口与外观订阅。
    _subscriptions: Vec<Subscription>,
}

impl MaterialState {
    /// 创建两个释放钩子，不启动定时器、后台任务或宿主服务。
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let release = cx.on_release_in(window, |state, window, _| state.clear(window));
        let visibility = cx.observe_window_visibility(window, |state, visibility, window, cx| {
            if !visibility.is_visible() {
                state.visible = false;
                state.clear(window);
            } else {
                cx.notify();
            }
        });
        let activation = cx.observe_window_activation(window, |state, window, cx| {
            if !window.is_window_active() { state.decoration.pause(); }
            cx.notify();
        });
        Self {
            pointer: window.mouse_position(),
            visible: false,
            hovered: false,
            decoration_hovered: false,
            motion: MaterialMotion::new(cx.background_executor().now()),
            decoration: CardDecoration::default(),
            shadow: Shadow::default(),
            spotlight: Spotlight::default(),
            rim: Rim::default(),
            retired: Retired::default(),
            cleanup_scheduled: false,
            _subscriptions: vec![release, visibility, activation],
        }
    }

    /// hover事件和光斑直接响应同一原生命中，不因键盘输入模式被隐藏。
    pub fn hover(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.hovered == hovered && self.decoration_hovered == hovered {
            return;
        }
        self.hovered = hovered;
        self.decoration_hovered = hovered;
        self.pointer = window.mouse_position();
        cx.notify();
    }

    /// 只在可见且有实际绘制区域时采样动画，静止状态复用当前图像ID。
    pub fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        dark: bool,
        geometry: Geometry,
        material: MaterialKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if bounds.is_empty() {
            self.clear(window);
            return;
        }
        if !self.visible {
            return;
        }
        let bounds = Bounds::from_corners(
            point(
                window.pixel_snap(bounds.origin.x),
                window.pixel_snap(bounds.origin.y),
            ),
            point(
                window.pixel_snap(bounds.right()),
                window.pixel_snap(bounds.bottom()),
            ),
        );
        if bounds.is_empty() {
            self.clear(window);
            return;
        }
        let now = window.frame_animation_time();
        let sample = match material {
            MaterialKind::Spotlight => {
                self.motion
                    .sample(dark, self.hovered, geometry.top_accent, now)
            }
            MaterialKind::Static(mode) => {
                self.hovered = false;
                self.motion
                    .sample_static(dark, mode == StaticMaterial::Table, now)
            }
        };
        let full_panel = material != MaterialKind::Static(StaticMaterial::FilterBar);
        if full_panel {
            self.shadow.paint(
                bounds,
                geometry.radius,
                sample.shadow,
                &mut self.retired,
                window,
                cx,
            );
        } else {
            self.shadow.retire(&mut self.retired, window);
        }
        window.paint_quad(quad(
            bounds,
            geometry.radius,
            sample.surface,
            Edges::default(),
            transparent_black(),
            BorderStyle::Solid,
        ));
        if material == MaterialKind::Spotlight && self.hovered {
            self.spotlight.paint(
                bounds,
                self.pointer,
                dark,
                geometry.radius,
                geometry.top_width,
                &mut self.retired,
                window,
            );
        } else {
            self.spotlight.retire(&mut self.retired, window);
        }
        if geometry.top_accent.is_some() && material == MaterialKind::Spotlight {
            self.rim
                .paint(bounds, geometry, &sample, &mut self.retired, window);
        } else {
            self.rim.retire(&mut self.retired, window);
            if full_panel {
                window.paint_inset_shadows(
                    bounds.dilate(-px(1.)),
                    Corners::all((geometry.radius - px(1.)).max(px(0.))),
                    &[BoxShadow {
                        color: sample.highlight,
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(0.),
                        spread_radius: px(0.),
                        inset: true,
                    }],
                );
            }
            window.paint_quad(quad(
                bounds,
                geometry.radius,
                transparent_black(),
                px(1.),
                sample.border,
                BorderStyle::Solid,
            ));
        }
        if sample.running {
            window.request_animation_frame();
        }
        self.schedule_cleanup(window, cx);
    }

    /// 清理上一代不持有当前实体强引用，连续重绘也最多排一个回调。
    fn schedule_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cleanup_scheduled || self.retired.is_empty() {
            return;
        }
        self.cleanup_scheduled = true;
        let owner = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = owner.update(cx, |state, _| {
                state.cleanup_scheduled = false;
                state.retired.clear(window);
            });
        });
    }

    /// 边缘在子内容之后绘制，图片不会遮住彩边；不增加命中区或修改子控件语义。
    pub fn decorate(
        &mut self,
        bounds: Bounds<Pixels>,
        radius: Pixels,
        priority: CardPriority,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible = self.visible
            && !bounds.intersect(&window.content_mask().bounds).is_empty();
        if !visible {
            self.decoration.pause();
            return;
        }
        let moving = visible && window.is_visible() && window.is_window_active() && !cx.reduce_motion();
        self.decoration.paint(bounds, radius, priority, dark, self.decoration_hovered, moving, window);
    }

    /// 当前与退休两代同时清空；窗口不可见时不依赖后续帧完成释放。
    pub fn clear(&mut self, window: &mut Window) {
        self.decoration.pause();
        self.shadow.clear(window);
        self.spotlight.clear(window);
        self.rim.clear(window);
        self.retired.clear(window);
    }

    /// 元素回归读取实际当前材质，不保留owner强引用或改变绘制行为。
    #[cfg(test)]
    pub fn audit(&self) -> (bool, Vec<std::sync::Arc<gpui_kit::RenderImage>>) {
        (
            self.hovered,
            self.shadow
                .image()
                .into_iter()
                .chain(self.spotlight.images())
                .chain(self.rim.image())
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        AnyWindowHandle, AppContext, Entity, IntoElement, Render, RenderImage, TestAppContext,
        WindowOptions, WindowVisibility, canvas, prelude::*, test::TestWindowExt,
    };
    use std::sync::Arc;

    /// owner以外的绘制闭包只有弱引用，卸载时不延长资源生命周期。
    struct Probe {
        /// 被探测的材质 owner；卸载后为 None。
        owner: Option<Entity<MaterialState>>,
    }

    impl Render for Probe {
        /// 测试直接调用生产paint，生成真实阴影及至少一个圆角光斑。
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let owner = self.owner.as_ref().map(Entity::downgrade);
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    if let Some(owner) = owner {
                        let _ = owner.update(cx, |state, cx| {
                            state.paint(
                                bounds,
                                false,
                                Geometry {
                                    radius: px(20.),
                                    top_width: px(1.),
                                    top_accent: None,
                                },
                                MaterialKind::Spotlight,
                                window,
                                cx,
                            );
                        });
                    }
                },
            )
            .w(px(400.))
            .h(px(200.))
        }
    }

    /// 实体订阅和绘制函数都使用生产实现，只省去业务壳层。
    fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let owner = cx.new(|cx| {
                    let mut state = MaterialState::new(window, cx);
                    state.visible = true;
                    state.hovered = true;
                    state.pointer = point(px(10.), px(10.));
                    state
                });
                cx.new(|_| Probe { owner: Some(owner) })
            })
            .unwrap()
        })
    }

    /// 捕获真实材质ID，不保留owner实体的强引用。
    fn images(probe: &Entity<Probe>, cx: &gpui_kit::App) -> Vec<Arc<RenderImage>> {
        let state = probe.read(cx).owner.as_ref().unwrap().read(cx);
        state
            .shadow
            .image()
            .into_iter()
            .chain(state.spotlight.images())
            .collect()
    }

    /// 不产生后续帧也必须清理GPU；再次显示只通知owner而不恢复过期图。
    #[gpui_kit::test]
    fn hidden_window_releases_every_current_material_texture(cx: &mut TestAppContext) {
        let (handle, probe) = open(cx);
        let before = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let images = images(&probe, cx);
                assert!(images.len() >= 3, "阴影、径向主体与圆角应真实上传");
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
                before
                    .iter()
                    .all(|image| !window.has_image_atlas_entry(image))
            );
        })
        .unwrap();
        cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
        cx.update_window(handle, |_, window, cx| {
            assert!(images(&probe, cx).is_empty());
            assert!(
                before
                    .iter()
                    .all(|image| !window.has_image_atlas_entry(image))
            );
        })
        .unwrap();
    }

    /// owner被移除时释放当前和待回收图，弱引用不会把已卸载卡片保活。
    #[gpui_kit::test]
    fn owner_release_removes_current_and_retired_atlas_entries(cx: &mut TestAppContext) {
        let (handle, probe) = open(cx);
        let (before, weak) = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let before = images(&probe, cx);
                let weak = probe.read(cx).owner.as_ref().unwrap().downgrade();
                probe.update(cx, |probe, cx| {
                    probe.owner = None;
                    cx.notify();
                });
                window.render_frame(cx);
                (before, weak)
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            assert!(weak.upgrade().is_none());
            assert!(
                before
                    .iter()
                    .all(|image| !window.has_image_atlas_entry(image))
            );
            assert!(probe.read(cx).owner.is_none());
        })
        .unwrap();
    }
}
